//! Domain-filtering HTTPS CONNECT proxy and socat bridge.
//!
//! When [`NetMode::AllowedDomains`](crate::policy::NetMode::AllowedDomains) is
//! active, [`ProxyBridge`] starts a lightweight HTTP proxy on a Unix domain
//! socket and a socat process that bridges it into the bwrap namespace.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::detect::detect_socat;
use crate::error::SandboxError;

/// A running proxy bridge that filters network requests by domain.
pub struct ProxyBridge {
    /// Path to the host-side Unix domain socket.
    socket_path: PathBuf,
    /// Loopback port inside the sandbox.
    sandbox_port: u16,
    /// Socat child process.
    socat_child: Option<Child>,
    /// Proxy server task handle.
    proxy_handle: Option<tokio::task::JoinHandle<()>>,
    /// Denied domains collected during execution.
    denied_log: Arc<Mutex<Vec<String>>>,
    /// Shutdown signal sender.
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ProxyBridge {
    /// Start the proxy bridge with the given domain allowlist.
    ///
    /// 1. Starts a TCP-based HTTP CONNECT proxy on a random loopback port.
    /// 2. Starts socat to bridge from a Unix domain socket to that port.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::SocatNotFound`] or [`SandboxError::ProxyFailed`].
    pub async fn start(allowed_domains: Vec<String>) -> Result<Self, SandboxError> {
        let socat_path = detect_socat()?;

        // Bind proxy to a random loopback port.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| SandboxError::ProxyFailed(format!("failed to bind proxy: {e}")))?;
        let proxy_addr = listener
            .local_addr()
            .map_err(|e| SandboxError::ProxyFailed(format!("failed to get proxy addr: {e}")))?;

        // Choose a second random port for the sandbox side.
        let sandbox_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| SandboxError::ProxyFailed(format!("failed to bind sandbox port: {e}")))?;
        let sandbox_port = sandbox_listener.local_addr().unwrap().port();
        drop(sandbox_listener); // Free the port for socat.

        // UDS path for the bridge.
        let socket_path =
            std::env::temp_dir().join(format!("sober-proxy-{}.sock", uuid::Uuid::now_v7()));

        // Start socat: bridge sandbox_port -> proxy_addr.
        let socat_child = tokio::process::Command::new(&socat_path)
            .arg(format!(
                "TCP-LISTEN:{sandbox_port},bind=127.0.0.1,fork,reuseaddr"
            ))
            .arg(format!("TCP:127.0.0.1:{}", proxy_addr.port()))
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SandboxError::ProxyFailed(format!("failed to start socat: {e}")))?;

        let denied_log = Arc::new(Mutex::new(Vec::new()));
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        // Start the proxy server.
        let denied_clone = denied_log.clone();
        let allowed = Arc::new(allowed_domains);
        let proxy_handle = tokio::spawn(async move {
            run_proxy(listener, allowed, denied_clone, shutdown_rx).await;
        });

        info!(
            proxy_port = proxy_addr.port(),
            sandbox_port,
            socket = %socket_path.display(),
            "proxy bridge started"
        );

        Ok(Self {
            socket_path,
            sandbox_port,
            socat_child: Some(socat_child),
            proxy_handle: Some(proxy_handle),
            denied_log,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Path to the host-side Unix domain socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Loopback port inside the sandbox.
    pub fn sandbox_port(&self) -> u16 {
        self.sandbox_port
    }

    /// Stop the proxy bridge and return the list of denied domains.
    pub async fn stop(mut self) -> Result<Vec<String>, SandboxError> {
        // Send shutdown signal.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Kill socat.
        if let Some(ref mut child) = self.socat_child {
            let _ = child.kill().await;
        }

        // Wait for proxy task to finish.
        if let Some(handle) = self.proxy_handle.take() {
            let _ = handle.await;
        }

        // Clean up socket file.
        let _ = tokio::fs::remove_file(&self.socket_path).await;

        let denied = self.denied_log.lock().await.clone();
        Ok(denied)
    }
}

/// Run the HTTP CONNECT proxy server.
async fn run_proxy(
    listener: TcpListener,
    allowed_domains: Arc<Vec<String>>,
    denied_log: Arc<Mutex<Vec<String>>>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, addr)) => {
                        let allowed = allowed_domains.clone();
                        let denied = denied_log.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, &allowed, &denied).await {
                                debug!(error = %e, "proxy connection error");
                            }
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "proxy accept error");
                    }
                }
            }
            _ = &mut shutdown_rx => {
                debug!("proxy received shutdown signal");
                break;
            }
        }
    }
}

/// Handle a single proxy connection.
///
/// Supports HTTP CONNECT (for HTTPS) and plain HTTP forwarding.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    allowed_domains: &[String],
    denied_log: &Mutex<Vec<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncReadExt;

    // Read the initial request line.
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");

    debug!(addr = %addr, request = %first_line, "proxy request");

    if first_line.starts_with("CONNECT ") {
        // HTTPS CONNECT: "CONNECT host:port HTTP/1.1"
        let target = first_line.split_whitespace().nth(1).unwrap_or("");
        let host = target.split(':').next().unwrap_or(target);

        if is_domain_allowed(host, allowed_domains) {
            // Establish tunnel.
            let port: u16 = target
                .split(':')
                .nth(1)
                .and_then(|p| p.parse().ok())
                .unwrap_or(443);

            match tokio::net::TcpStream::connect((host, port)).await {
                Ok(mut upstream) => {
                    stream
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await?;
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                }
                Err(e) => {
                    let msg = format!("HTTP/1.1 502 Bad Gateway\r\n\r\n{e}");
                    stream.write_all(msg.as_bytes()).await?;
                }
            }
        } else {
            denied_log.lock().await.push(host.to_owned());
            stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\nDomain not in allowlist\r\n")
                .await?;
        }
    } else {
        // Plain HTTP request — extract Host header.
        let host = request
            .lines()
            .find(|l| l.to_lowercase().starts_with("host:"))
            .and_then(|l| l.split(':').nth(1))
            .map(|h| h.trim())
            .unwrap_or("");

        if is_domain_allowed(host, allowed_domains) {
            // For plain HTTP, we'd need to forward the full request.
            // Simplified: just reject with 501 since HTTPS is the common case.
            stream
                .write_all(b"HTTP/1.1 501 Not Implemented\r\n\r\nPlain HTTP forwarding not supported, use HTTPS\r\n")
                .await?;
        } else {
            denied_log.lock().await.push(host.to_owned());
            stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\nDomain not in allowlist\r\n")
                .await?;
        }
    }

    Ok(())
}

/// Check if a domain is in the allowlist.
///
/// Supports wildcard `"*"` to allow all domains.
fn is_domain_allowed(domain: &str, allowed: &[String]) -> bool {
    if allowed.iter().any(|d| d == "*") {
        return true;
    }
    allowed.iter().any(|d| d == domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_allowed_exact_match() {
        let allowed = vec!["example.com".into(), "api.openai.com".into()];
        assert!(is_domain_allowed("example.com", &allowed));
        assert!(is_domain_allowed("api.openai.com", &allowed));
        assert!(!is_domain_allowed("evil.com", &allowed));
    }

    #[test]
    fn domain_allowed_wildcard() {
        let allowed = vec!["*".into()];
        assert!(is_domain_allowed("anything.com", &allowed));
        assert!(is_domain_allowed("evil.com", &allowed));
    }

    #[test]
    fn domain_allowed_empty_list() {
        let allowed: Vec<String> = vec![];
        assert!(!is_domain_allowed("example.com", &allowed));
    }
}
