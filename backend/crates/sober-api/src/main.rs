//! Sober API — HTTP/WebSocket gateway.
//!
//! Entry point for the `sober-api` binary. Starts the HTTP server with
//! all routes, middleware, and optionally an admin Unix socket.

use std::net::SocketAddr;
use std::time::Duration;

use http::Method;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use sober_api::admin;
use sober_api::middleware::rate_limit::{RateLimitConfig, RateLimitLayer, RateLimitScope};
use sober_api::routes;
use sober_api::state::AppState;
use sober_core::config::{AppConfig, Environment};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::load_from_env()?;

    init_tracing(&config);

    let state = AppState::new(config.clone()).await?;

    // Build the router with all middleware.
    let app = routes::build_router(state.clone());

    // Apply global middleware stack (outermost = first to run).
    let cors = build_cors(&config);
    let rate_limit = RateLimitLayer::new(
        RateLimitConfig {
            max_requests: 60,
            window: Duration::from_secs(60),
        },
        RateLimitScope::User,
    );

    let app = app
        .layer(cors)
        .layer(rate_limit)
        .layer(TraceLayer::new_for_http())
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

/// Initializes the tracing subscriber.
fn init_tracing(config: &AppConfig) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sober_api=debug,tower_http=debug,info"));

    match config.environment {
        Environment::Production => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .init();
        }
        Environment::Development => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
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
