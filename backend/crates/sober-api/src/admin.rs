//! Unix domain socket listener for admin operations.
//!
//! Exposes a minimal router (health check only for v1) over a Unix
//! socket. Access controlled by filesystem permissions — no auth.

use std::path::Path;

use axum::Router;
use axum::routing::get;
use hyper_util::rt::TokioIo;
use sober_core::types::ApiResponse;
use tokio::net::UnixListener;
use tracing::{error, info};

/// Starts the admin socket listener at the given path.
///
/// Serves until the provided cancellation future resolves.
pub async fn serve_admin_socket(socket_path: &Path, shutdown: tokio::sync::watch::Receiver<()>) {
    // Remove stale socket file if it exists.
    let _ = std::fs::remove_file(socket_path);

    // Ensure parent directory exists.
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            error!(error = %e, path = %socket_path.display(), "failed to bind admin socket");
            return;
        }
    };

    info!(path = %socket_path.display(), "admin socket listening");

    let app = Router::new().route("/health", get(admin_health));

    loop {
        let mut shutdown = shutdown.clone();

        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!(error = %e, "failed to accept admin connection");
                        continue;
                    }
                };

                let app = app.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = hyper::service::service_fn(move |req| {
                        let app = app.clone();
                        async move {
                            Ok::<_, std::convert::Infallible>(
                                tower::ServiceExt::oneshot(app, req).await.unwrap(),
                            )
                        }
                    });

                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await
                    {
                        error!(error = %e, "admin connection error");
                    }
                });
            }
            _ = shutdown.changed() => {
                info!("admin socket shutting down");
                break;
            }
        }
    }

    // Clean up socket file.
    let _ = std::fs::remove_file(socket_path);
}

/// Admin health check handler (no auth required).
async fn admin_health() -> ApiResponse<serde_json::Value> {
    ApiResponse::new(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
