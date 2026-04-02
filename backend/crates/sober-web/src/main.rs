//! Sober Web — serves the SvelteKit frontend and reverse-proxies API/WebSocket
//! traffic to `sober-api`.
//!
//! Static files can be embedded at compile time (via `rust-embed`) or served
//! from disk at runtime when `STATIC_DIR` is set.

use std::net::SocketAddr;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderValue, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rust_embed::Embed;
use sober_core::config::AppConfig;
use tokio::net::TcpListener;
use tokio::signal;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{error, info, instrument};

/// Static files built by SvelteKit (`pnpm build` → `frontend/build/`).
///
/// In release builds these are embedded in the binary. When `STATIC_DIR` is
/// set the embedded files are ignored and the directory is served instead.
#[derive(Embed)]
#[folder = "../../../frontend/build/"]
struct StaticAssets;

/// Shared state for the reverse proxy.
#[derive(Clone)]
struct ProxyState {
    /// HTTP client used to forward requests to `sober-api`.
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    /// Upstream base URL, e.g. `http://sober-api:3000`.
    api_upstream: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let environment = match std::env::var("SOBER_ENV").as_deref() {
        Ok("production") => sober_core::config::Environment::Production,
        _ => sober_core::config::Environment::Development,
    };
    let _telemetry =
        sober_core::init_telemetry(environment, "sober_web=info,sqlx::query=warn,info");

    let config = AppConfig::load_unvalidated()?;

    let host = config.web.host.clone();
    let port = config.web.port;
    let api_upstream = config.web.api_upstream_url.clone();
    let static_dir = config
        .web
        .static_dir
        .as_ref()
        .map(|p| p.display().to_string());

    let proxy_state = ProxyState {
        client: Client::builder(TokioExecutor::new()).build_http(),
        api_upstream,
    };

    info!(
        static_mode = if static_dir.is_some() {
            "disk"
        } else {
            "embedded"
        },
        "building router"
    );

    let app = build_router(proxy_state, static_dir.as_deref());

    let addr = SocketAddr::new(host.parse()?, port);
    let listener = TcpListener::bind(addr).await?;
    info!(address = %addr, "sober-web listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("sober-web shut down");
    Ok(())
}

/// Builds the full router with API proxy and static file serving.
fn build_router(state: ProxyState, static_dir: Option<&str>) -> Router {
    // Reverse-proxy routes: /api/* goes to sober-api, /api/v1/ws gets dedicated WS proxy.
    // Specific route before wildcard so axum matches it first.
    let app = Router::new()
        .route(
            "/api/v1/ws",
            axum::routing::get(ws_reverse_proxy).with_state(state.clone()),
        )
        .route(
            "/api/{*rest}",
            axum::routing::any(reverse_proxy).with_state(state),
        );

    // Static file serving: disk or embedded.
    if let Some(dir) = static_dir {
        info!(dir, "serving static files from disk");
        let dir_owned = dir.to_owned();
        let spa_fallback = tower::service_fn(move |_req: Request<Body>| {
            let dir = dir_owned.clone();
            async move {
                let index = std::path::Path::new(&dir).join("index.html");
                let body = tokio::fs::read(index).await.unwrap_or_default();
                Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .header("content-type", "text/html")
                        .body(Body::from(body))
                        .unwrap(),
                )
            }
        });
        app.fallback_service(ServeDir::new(dir).fallback(spa_fallback))
            .layer(TraceLayer::new_for_http())
    } else {
        info!("serving embedded static files");
        app.fallback(serve_embedded)
            .layer(TraceLayer::new_for_http())
    }
}

/// Reverse-proxy handler: forwards the request to `sober-api`, preserving the
/// original URI path (including `/api` prefix).
#[instrument(skip_all, fields(upstream.path = %original_uri.path()))]
async fn reverse_proxy(
    State(state): State<ProxyState>,
    original_uri: axum::extract::OriginalUri,
    mut req: Request<Body>,
) -> Result<Response, StatusCode> {
    let path_and_query = original_uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let upstream_uri = format!("{}{}", state.api_upstream, path_and_query);
    *req.uri_mut() = upstream_uri
        .parse::<Uri>()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    state
        .client
        .request(req)
        .await
        .map(|resp| resp.map(Body::new))
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

/// WebSocket reverse proxy: upgrades the client connection, connects to the
/// upstream `sober-api` WebSocket, and pipes messages between the two.
#[instrument(skip_all)]
async fn ws_reverse_proxy(
    State(state): State<ProxyState>,
    headers: http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let cookie = headers.get(http::header::COOKIE).cloned();
    let upstream = state.api_upstream.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(e) = proxy_websocket(socket, &upstream, cookie).await {
            error!(error = %e, "WebSocket proxy error");
        }
    })
}

/// Pipes WebSocket messages between the client and upstream `sober-api`.
async fn proxy_websocket(
    client_socket: WebSocket,
    upstream_url: &str,
    cookie: Option<HeaderValue>,
) -> Result<(), anyhow::Error> {
    let ws_url = format!(
        "{}/api/v1/ws",
        upstream_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1)
    );

    // Build request from URI string so tungstenite auto-adds WebSocket
    // handshake headers (Upgrade, Connection, Sec-WebSocket-Key, etc.).
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = ws_url.into_client_request()?;
    if let Some(cookie) = &cookie {
        request
            .headers_mut()
            .insert(http::header::COOKIE, cookie.clone());
    }

    let (upstream_socket, _) = tokio_tungstenite::connect_async(request).await?;

    let (mut client_tx, mut client_rx) = client_socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream_socket.split();

    let client_to_upstream = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            let tung_msg = match msg {
                WsMessage::Text(t) => TungsteniteMessage::text(t.to_string()),
                WsMessage::Binary(b) => TungsteniteMessage::binary(b.to_vec()),
                WsMessage::Ping(p) => TungsteniteMessage::Ping(p.to_vec().into()),
                WsMessage::Pong(p) => TungsteniteMessage::Pong(p.to_vec().into()),
                WsMessage::Close(_) => TungsteniteMessage::Close(None),
            };
            if upstream_tx.send(tung_msg).await.is_err() {
                break;
            }
        }
    };

    let upstream_to_client = async {
        while let Some(Ok(msg)) = upstream_rx.next().await {
            let axum_msg = match msg {
                TungsteniteMessage::Text(t) => WsMessage::Text(t.to_string().into()),
                TungsteniteMessage::Binary(b) => WsMessage::Binary(b.to_vec().into()),
                TungsteniteMessage::Ping(p) => WsMessage::Ping(p.to_vec().into()),
                TungsteniteMessage::Pong(p) => WsMessage::Pong(p.to_vec().into()),
                TungsteniteMessage::Close(_) => WsMessage::Close(None),
                TungsteniteMessage::Frame(_) => continue,
            };
            if client_tx.send(axum_msg).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        () = client_to_upstream => {}
        () = upstream_to_client => {}
    }

    Ok(())
}

/// Serves a file from the embedded `StaticAssets`, with SPA fallback.
async fn serve_embedded(uri: axum::extract::OriginalUri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact path first.
    if let Some(file) = <StaticAssets as Embed>::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        return Response::builder()
            .header("content-type", mime)
            .body(Body::from(file.data.to_vec()))
            .unwrap();
    }

    // SPA fallback: serve index.html for non-file routes.
    match <StaticAssets as Embed>::get("index.html") {
        Some(index) => Response::builder()
            .header("content-type", "text/html")
            .body(Body::from(index.data.to_vec()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

/// Waits for SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received SIGINT"),
        () = terminate => info!("received SIGTERM"),
    }
}
