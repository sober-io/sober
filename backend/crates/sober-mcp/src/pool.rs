//! Connection pool managing multiple MCP server connections.
//!
//! [`McpPool`] tracks per-server state including failure counts and
//! handles reconnection with cooldown periods.

use std::collections::HashMap;
use std::sync::Arc;

use sober_core::types::McpServerId;
use sober_sandbox::SandboxPolicy;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::adapter::{McpResourceAdapter, McpToolAdapter};
use crate::client::McpClient;
use crate::config::McpConfig;
use crate::error::McpError;
use crate::types::{McpResourceInfo, McpServerRunConfig, McpToolInfo};

/// State of a managed MCP server connection.
#[derive(Debug)]
enum ServerState {
    /// Connected and initialized.
    Connected,
    /// Disconnected due to failure.
    Disconnected {
        /// Number of consecutive failures.
        consecutive_failures: u32,
        /// When the last failure occurred.
        last_failure: std::time::Instant,
    },
}

/// Entry for a single managed MCP server.
struct ServerEntry {
    /// Runtime config for the server.
    run_config: McpServerRunConfig,
    /// Sandbox policy for the server process.
    sandbox_policy: SandboxPolicy,
    /// Current connection state.
    state: ServerState,
    /// Shared client connection (if connected).
    client: Option<Arc<Mutex<McpClient>>>,
    /// Discovered tools.
    tools: Vec<McpToolInfo>,
    /// Discovered resources.
    resources: Vec<McpResourceInfo>,
}

/// Pool managing multiple MCP server connections with crash recovery.
///
/// Each server is identified by its [`McpServerId`]. The pool tracks
/// connection state, failure counts, and manages reconnection attempts
/// with configurable cooldown periods.
pub struct McpPool {
    servers: HashMap<McpServerId, ServerEntry>,
    config: McpConfig,
}

impl McpPool {
    /// Create a new empty connection pool.
    pub fn new(config: McpConfig) -> Self {
        Self {
            servers: HashMap::new(),
            config,
        }
    }

    /// Register MCP servers for a user and connect to them.
    ///
    /// Each server configuration is spawned, initialized, and its tools/resources
    /// discovered. Servers that fail to connect are recorded as disconnected
    /// but do not prevent other servers from connecting.
    ///
    /// # Errors
    ///
    /// Individual server connection failures are logged but do not fail the
    /// overall operation. The method only returns an error if no servers
    /// were provided.
    pub async fn connect_servers(
        &mut self,
        servers: Vec<(McpServerId, McpServerRunConfig, SandboxPolicy)>,
    ) -> Vec<McpError> {
        let mut errors = Vec::new();

        for (id, run_config, sandbox_policy) in servers {
            debug!(server = %run_config.name, "connecting to MCP server");

            match McpClient::connect(&run_config, sandbox_policy.clone(), self.config.clone()).await
            {
                Ok(mut client) => {
                    if let Err(e) = client.initialize().await {
                        warn!(
                            server = %run_config.name,
                            error = %e,
                            "MCP initialize failed"
                        );
                        self.servers.insert(
                            id,
                            ServerEntry {
                                run_config,
                                sandbox_policy,
                                state: ServerState::Disconnected {
                                    consecutive_failures: 1,
                                    last_failure: std::time::Instant::now(),
                                },
                                client: None,
                                tools: vec![],
                                resources: vec![],
                            },
                        );
                        errors.push(e);
                        continue;
                    }

                    let client = Arc::new(Mutex::new(client));

                    self.servers.insert(
                        id,
                        ServerEntry {
                            run_config,
                            sandbox_policy,
                            state: ServerState::Connected,
                            client: Some(client),
                            tools: vec![],
                            resources: vec![],
                        },
                    );

                    info!(server_id = %id, "MCP server connected and initialized");
                }
                Err(e) => {
                    warn!(
                        server = %run_config.name,
                        error = %e,
                        "failed to connect to MCP server"
                    );
                    self.servers.insert(
                        id,
                        ServerEntry {
                            run_config,
                            sandbox_policy,
                            state: ServerState::Disconnected {
                                consecutive_failures: 1,
                                last_failure: std::time::Instant::now(),
                            },
                            client: None,
                            tools: vec![],
                            resources: vec![],
                        },
                    );
                    errors.push(e);
                }
            }
        }

        errors
    }

    /// Discover tools and resources from all connected servers.
    ///
    /// Updates the internal tool/resource lists for each server. Returns
    /// the total count of tools and resources discovered.
    pub async fn discover(&mut self) -> (usize, usize) {
        let mut total_tools = 0;
        let mut total_resources = 0;

        let server_ids: Vec<McpServerId> = self.servers.keys().copied().collect();

        for id in server_ids {
            let entry = match self.servers.get(&id) {
                Some(e) => e,
                None => continue,
            };

            let client = match &entry.client {
                Some(c) => Arc::clone(c),
                None => continue,
            };

            // Discover tools.
            let tools = {
                let mut c = client.lock().await;
                match c.list_tools().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(server_id = %id, error = %e, "failed to list tools");
                        vec![]
                    }
                }
            };

            // Discover resources.
            let resources = {
                let mut c = client.lock().await;
                match c.list_resources().await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(server_id = %id, error = %e, "failed to list resources");
                        vec![]
                    }
                }
            };

            total_tools += tools.len();
            total_resources += resources.len();

            if let Some(entry) = self.servers.get_mut(&id) {
                debug!(
                    server_id = %id,
                    tools = tools.len(),
                    resources = resources.len(),
                    "discovered MCP capabilities"
                );
                entry.tools = tools;
                entry.resources = resources;
            }
        }

        (total_tools, total_resources)
    }

    /// Get tool adapters for all connected servers.
    ///
    /// Returns adapters that implement the [`Tool`](sober_core::types::Tool)
    /// trait, ready for use by the agent.
    #[must_use]
    pub fn tool_adapters(&self) -> Vec<McpToolAdapter> {
        let mut adapters = Vec::new();

        for entry in self.servers.values() {
            let client = match &entry.client {
                Some(c) => Arc::clone(c),
                None => continue,
            };

            for tool in &entry.tools {
                adapters.push(McpToolAdapter::new(
                    tool.clone(),
                    entry.run_config.name.clone(),
                    Arc::clone(&client),
                ));
            }
        }

        adapters
    }

    /// Get resource adapters for all connected servers.
    #[must_use]
    pub fn resource_adapters(&self) -> Vec<McpResourceAdapter> {
        let mut adapters = Vec::new();

        for entry in self.servers.values() {
            let client = match &entry.client {
                Some(c) => Arc::clone(c),
                None => continue,
            };

            for resource in &entry.resources {
                adapters.push(McpResourceAdapter::new(
                    resource.clone(),
                    entry.run_config.name.clone(),
                    Arc::clone(&client),
                ));
            }
        }

        adapters
    }

    /// Record a failure for a server, incrementing its consecutive failure count.
    ///
    /// If the failure count exceeds `max_consecutive_failures`, the server
    /// is marked as disconnected and its client is dropped.
    pub async fn record_failure(&mut self, server_id: &McpServerId) {
        if let Some(entry) = self.servers.get_mut(server_id) {
            match &mut entry.state {
                ServerState::Connected => {
                    warn!(server_id = %server_id, "marking server as disconnected after failure");
                    entry.state = ServerState::Disconnected {
                        consecutive_failures: 1,
                        last_failure: std::time::Instant::now(),
                    };

                    // Shut down the client.
                    if let Some(client) = entry.client.take() {
                        let mut c = client.lock().await;
                        c.shutdown().await;
                    }

                    entry.tools.clear();
                    entry.resources.clear();
                }
                ServerState::Disconnected {
                    consecutive_failures,
                    last_failure,
                } => {
                    *consecutive_failures += 1;
                    *last_failure = std::time::Instant::now();
                    warn!(
                        server_id = %server_id,
                        failures = *consecutive_failures,
                        "recorded additional failure for disconnected server"
                    );
                }
            }
        }
    }

    /// Record a successful operation for a server, resetting its failure count.
    pub fn record_success(&mut self, server_id: &McpServerId) {
        if let Some(entry) = self.servers.get_mut(server_id)
            && !matches!(entry.state, ServerState::Connected)
        {
            debug!(server_id = %server_id, "server recovered, marking as connected");
            entry.state = ServerState::Connected;
        }
    }

    /// Attempt to reconnect a disconnected server.
    ///
    /// Respects the configured cooldown period between reconnection attempts.
    ///
    /// # Errors
    ///
    /// Returns an error if the server is not found or reconnection fails.
    pub async fn try_reconnect(&mut self, server_id: &McpServerId) -> Result<(), McpError> {
        let entry = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpError::ServerUnavailable(format!("server {server_id} not found")))?;

        // Check cooldown.
        if let ServerState::Disconnected {
            last_failure,
            consecutive_failures,
        } = &entry.state
        {
            let elapsed = last_failure.elapsed();
            let cooldown = std::time::Duration::from_secs(self.config.restart_cooldown_secs);

            if elapsed < cooldown {
                return Err(McpError::ServerUnavailable(format!(
                    "server {server_id} in cooldown ({} failures, {}s remaining)",
                    consecutive_failures,
                    (cooldown - elapsed).as_secs()
                )));
            }
        } else {
            // Already connected.
            return Ok(());
        }

        // Clone what we need before mutably borrowing.
        let run_config = entry.run_config.clone();
        let sandbox_policy = entry.sandbox_policy.clone();
        let config = self.config.clone();

        info!(server_id = %server_id, server = %run_config.name, "attempting reconnection");

        match McpClient::connect(&run_config, sandbox_policy, config).await {
            Ok(mut client) => {
                if let Err(e) = client.initialize().await {
                    error!(server_id = %server_id, error = %e, "reconnect initialize failed");
                    if let Some(entry) = self.servers.get_mut(server_id)
                        && let ServerState::Disconnected {
                            consecutive_failures,
                            last_failure,
                        } = &mut entry.state
                    {
                        *consecutive_failures += 1;
                        *last_failure = std::time::Instant::now();
                    }
                    return Err(e);
                }

                let client = Arc::new(Mutex::new(client));

                if let Some(entry) = self.servers.get_mut(server_id) {
                    entry.state = ServerState::Connected;
                    entry.client = Some(client);
                    info!(server_id = %server_id, "reconnection successful");
                }

                Ok(())
            }
            Err(e) => {
                error!(server_id = %server_id, error = %e, "reconnection failed");
                if let Some(entry) = self.servers.get_mut(server_id)
                    && let ServerState::Disconnected {
                        consecutive_failures,
                        last_failure,
                    } = &mut entry.state
                {
                    *consecutive_failures += 1;
                    *last_failure = std::time::Instant::now();
                }
                Err(e)
            }
        }
    }

    /// Shut down all connected servers and clear the pool.
    pub async fn shutdown(&mut self) {
        for entry in self.servers.values_mut() {
            if let Some(client) = entry.client.take() {
                let mut c = client.lock().await;
                c.shutdown().await;
            }
        }

        self.servers.clear();
        info!("MCP pool shut down");
    }

    /// Returns the number of registered servers.
    #[must_use]
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// Returns the number of currently connected servers.
    #[must_use]
    pub fn connected_count(&self) -> usize {
        self.servers
            .values()
            .filter(|e| matches!(e.state, ServerState::Connected))
            .count()
    }

    /// Returns `true` if the specified server is currently connected.
    #[must_use]
    pub fn is_connected(&self, server_id: &McpServerId) -> bool {
        self.servers
            .get(server_id)
            .is_some_and(|e| matches!(e.state, ServerState::Connected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pool_is_empty() {
        let pool = McpPool::new(McpConfig::default());
        assert_eq!(pool.server_count(), 0);
        assert_eq!(pool.connected_count(), 0);
    }

    #[test]
    fn tool_adapters_empty_when_no_servers() {
        let pool = McpPool::new(McpConfig::default());
        assert!(pool.tool_adapters().is_empty());
    }

    #[test]
    fn resource_adapters_empty_when_no_servers() {
        let pool = McpPool::new(McpConfig::default());
        assert!(pool.resource_adapters().is_empty());
    }
}
