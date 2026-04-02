//! Sober API — HTTP/WebSocket gateway.
//!
//! Entry point for the `sober-api` binary. Starts the HTTP server with
//! all routes, middleware, and optionally an admin Unix socket.

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::MatchedPath;
use axum::routing::get;
use axum_core::body::Body;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use http::{Method, Response};
use sober_api::admin;
use sober_api::middleware::metrics::HttpMetricsLayer;
use sober_api::middleware::rate_limit::{RateLimitConfig, RateLimitLayer};
use sober_api::routes;
use sober_api::state::AppState;
use sober_core::MetricsEndpoint;
use sober_core::config::{AppConfig, Environment};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::{Span, info, info_span};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::load()?;

    let telemetry =
        sober_core::init_telemetry(config.environment, "sober_api=debug,tower_http=debug,info");

    let state = AppState::new(config.clone()).await?;

    // Spawn the background subscription to agent conversation updates.
    sober_api::subscribe::spawn_subscription(
        state.agent_client.clone(),
        state.connections.clone(),
        state.user_connections.clone(),
        state.db.clone(),
    );

    // Build the router with all middleware.
    let app = routes::build_router(state.clone())
        .route(
            "/metrics",
            get(MetricsEndpoint(telemetry.prometheus.clone())),
        )
        .layer(HttpMetricsLayer::new());

    // Apply global middleware stack (outermost = first to run).
    let cors = build_cors(&config);
    let rate_limit = RateLimitLayer::new(RateLimitConfig {
        max_requests: config.server.rate_limit_max_requests,
        window: Duration::from_secs(config.server.rate_limit_window_secs),
    });

    let app = app
        .layer(cors)
        .layer(rate_limit)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &http::Request<Body>| {
                    let matched_path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(|p| p.as_str().to_owned());

                    info_span!(
                        "http_request",
                        http.method = %request.method(),
                        http.route = matched_path.as_deref().unwrap_or(""),
                        http.status_code = tracing::field::Empty,
                        user.id = tracing::field::Empty,
                        request.id = tracing::field::Empty,
                        otel.status_code = tracing::field::Empty,
                        error.type_ = tracing::field::Empty,
                        error.message = tracing::field::Empty,
                    )
                })
                .on_response(
                    |response: &Response<Body>, latency: Duration, span: &Span| {
                        let status = response.status().as_u16();
                        span.record("http.status_code", status);

                        if status >= 500 {
                            tracing::error!(
                                latency_ms = latency.as_millis() as u64,
                                "request failed"
                            );
                        } else if status >= 400 {
                            tracing::warn!(latency_ms = latency.as_millis() as u64, "client error");
                        } else {
                            tracing::info!(
                                latency_ms = latency.as_millis() as u64,
                                "request completed"
                            );
                        }
                    },
                )
                .on_failure(
                    |error: tower_http::classify::ServerErrorsFailureClass,
                     latency: Duration,
                     _span: &Span| {
                        tracing::error!(
                            %error,
                            latency_ms = latency.as_millis() as u64,
                            "request error"
                        );
                    },
                ),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id());

    // Optionally start admin socket.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    let admin_handle = {
        let admin_path = config.admin.socket_path.clone();
        tokio::spawn(async move {
            admin::serve_admin_socket(&admin_path, shutdown_rx).await;
        })
    };

    // Bind TCP listener.
    let addr = SocketAddr::new(
        config.server.host.parse().expect("valid host"),
        config.server.port,
    );
    let listener = TcpListener::bind(addr).await?;
    info!(address = %addr, "sober-api listening");

    // Serve with graceful shutdown.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("shutting down");
    let _ = shutdown_tx.send(());
    admin_handle.abort();

    Ok(())
}

/// Builds the CORS layer.
fn build_cors(config: &AppConfig) -> CorsLayer {
    match config.environment {
        Environment::Development => CorsLayer::very_permissive(),
        Environment::Production => CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
            .allow_headers([AUTHORIZATION, CONTENT_TYPE])
            .allow_credentials(true),
    }
}

/// Waits for a shutdown signal (SIGTERM or SIGINT).
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
