//! Configuration validation, display, and generation commands.

use anyhow::{Context, Result};
use sober_core::config::AppConfig;

/// Validate that all required configuration is present.
///
/// Loads configuration from TOML + env overlay and runs full validation.
pub fn validate() -> Result<()> {
    AppConfig::load().context("configuration validation failed")?;
    println!("Configuration is valid.");
    Ok(())
}

/// Display the resolved configuration with secrets redacted.
///
/// When `source` is true, each line is annotated with `(default)` — full
/// source tracking (default/toml/env) is a future enhancement.
pub fn show(source: bool) -> Result<()> {
    let config = AppConfig::load_unvalidated().context("failed to load configuration")?;

    let suffix = if source { " (default)" } else { "" };

    println!("=== Sõber Configuration ===\n");

    // Environment
    println!("[environment]");
    println!(
        "  mode                      = {:?}{suffix}",
        config.environment
    );

    // Database
    println!("\n[database]");
    println!(
        "  url                       = {}{suffix}",
        redact_url(&config.database.url)
    );
    println!(
        "  max_connections           = {}{suffix}",
        config.database.max_connections
    );

    // Qdrant
    println!("\n[qdrant]");
    println!(
        "  url                       = {}{suffix}",
        config.qdrant.url
    );
    println!(
        "  api_key                   = {}{suffix}",
        redact_opt(&config.qdrant.api_key)
    );

    // LLM
    println!("\n[llm]");
    println!(
        "  base_url                  = {}{suffix}",
        config.llm.base_url
    );
    println!(
        "  api_key                   = {}{suffix}",
        redact_opt(&config.llm.api_key)
    );
    println!("  model                     = {}{suffix}", config.llm.model);
    println!(
        "  max_tokens                = {}{suffix}",
        config.llm.max_tokens
    );
    println!(
        "  embedding_model           = {}{suffix}",
        config.llm.embedding_model
    );
    println!(
        "  embedding_dim             = {}{suffix}",
        config.llm.embedding_dim
    );

    // Server
    println!("\n[server]");
    println!(
        "  host                      = {}{suffix}",
        config.server.host
    );
    println!(
        "  port                      = {}{suffix}",
        config.server.port
    );
    println!(
        "  rate_limit_max_requests   = {}{suffix}",
        config.server.rate_limit_max_requests
    );
    println!(
        "  rate_limit_window_secs    = {}{suffix}",
        config.server.rate_limit_window_secs
    );

    // Auth
    println!("\n[auth]");
    println!(
        "  session_secret            = {}{suffix}",
        redact_opt(&config.auth.session_secret)
    );
    println!(
        "  session_ttl_seconds       = {}{suffix}",
        config.auth.session_ttl_seconds
    );

    // Crypto
    println!("\n[crypto]");
    println!(
        "  master_encryption_key     = {}{suffix}",
        redact_opt(&config.crypto.master_encryption_key)
    );

    // SearXNG
    println!("\n[searxng]");
    println!(
        "  url                       = {}{suffix}",
        config.searxng.url
    );

    // Admin
    println!("\n[admin]");
    println!(
        "  socket_path               = {}{suffix}",
        config.admin.socket_path.display()
    );

    // Agent
    println!("\n[agent]");
    println!(
        "  socket_path               = {}{suffix}",
        config.agent.socket_path.display()
    );
    println!(
        "  metrics_port              = {}{suffix}",
        config.agent.metrics_port
    );
    println!(
        "  workspace_root            = {}{suffix}",
        config.workspace_root.display()
    );
    println!(
        "  sandbox_profile           = {}{suffix}",
        config.agent.sandbox_profile
    );

    // Scheduler
    println!("\n[scheduler]");
    println!(
        "  tick_interval_secs        = {}{suffix}",
        config.scheduler.tick_interval_secs
    );
    println!(
        "  agent_socket_path         = {}{suffix}",
        config.scheduler.agent_socket_path.display()
    );
    println!(
        "  socket_path               = {}{suffix}",
        config.scheduler.socket_path.display()
    );
    println!(
        "  max_concurrent_jobs       = {}{suffix}",
        config.scheduler.max_concurrent_jobs
    );
    println!(
        "  metrics_port              = {}{suffix}",
        config.scheduler.metrics_port
    );
    println!(
        "  workspace_root            = {}{suffix}",
        config.workspace_root.display()
    );
    println!(
        "  sandbox_profile           = {}{suffix}",
        config.scheduler.sandbox_profile
    );

    // Web
    println!("\n[web]");
    println!("  host                      = {}{suffix}", config.web.host);
    println!("  port                      = {}{suffix}", config.web.port);
    println!(
        "  api_upstream_url          = {}{suffix}",
        config.web.api_upstream_url
    );
    println!(
        "  static_dir                = {}{suffix}",
        config
            .web
            .static_dir
            .as_ref()
            .map_or("(not set)".to_owned(), |p| p.display().to_string())
    );

    // MCP
    println!("\n[mcp]");
    println!(
        "  request_timeout_secs      = {}{suffix}",
        config.mcp.request_timeout_secs
    );
    println!(
        "  max_consecutive_failures  = {}{suffix}",
        config.mcp.max_consecutive_failures
    );
    println!(
        "  idle_timeout_secs         = {}{suffix}",
        config.mcp.idle_timeout_secs
    );

    // Memory
    println!("\n[memory]");
    println!(
        "  decay_half_life_days      = {}{suffix}",
        config.memory.decay_half_life_days
    );
    println!(
        "  retrieval_boost           = {}{suffix}",
        config.memory.retrieval_boost
    );
    println!(
        "  prune_threshold           = {}{suffix}",
        config.memory.prune_threshold
    );

    // ACP
    println!("\n[acp]");
    match &config.acp {
        Some(acp) => {
            println!("  name                      = {}{suffix}", acp.name);
            println!("  command                   = {}{suffix}", acp.command);
            println!("  args                      = {:?}{suffix}", acp.args);
        }
        None => {
            println!("  (not configured)");
        }
    }

    Ok(())
}

/// Generate a complete default `config.toml` and print it to stdout.
pub fn generate() -> Result<()> {
    print!(
        r##"# Sõber configuration file
# ========================
#
# Place this file at one of these locations:
#   1. Path specified by SOBER_CONFIG environment variable
#   2. /etc/sober/config.toml  (production)
#   3. ./config.toml           (development)
#
# Environment variables (SOBER_*) override values from this file.
# Lines beginning with # are comments. Uncomment and edit as needed.

# Runtime environment: "development" or "production".
# Production mode enables JSON logging and requires session_secret.
environment = "development"

# --- PostgreSQL ---
[database]
# Connection URL (required).
url = ""
# Maximum pool connections.
max_connections = 10

# --- Qdrant vector database ---
[qdrant]
url = "http://localhost:6334"
# api_key = ""

# --- LLM provider ---
[llm]
# OpenAI-compatible API endpoint.
base_url = "https://openrouter.ai/api/v1"
# api_key = "sk-..."
model = "anthropic/claude-sonnet-4"
max_tokens = 4096
embedding_model = "text-embedding-3-small"
embedding_dim = 1536

# --- API server ---
[server]
host = "0.0.0.0"
port = 3000
# Rate limiting.
rate_limit_max_requests = 1200
rate_limit_window_secs = 60

# --- Authentication ---
[auth]
# Secret for session token signing (required in production).
# Generate with: openssl rand -hex 32
# session_secret = ""
# Session time-to-live in seconds (default: 30 days).
session_ttl_seconds = 2592000

# --- Envelope encryption ---
[crypto]
# Hex-encoded 256-bit master key for secrets management.
# Generate with: openssl rand -hex 32
# master_encryption_key = ""

# --- SearXNG search ---
[searxng]
url = "http://localhost:8080"

# --- Admin socket ---
[admin]
socket_path = "/run/sober/admin.sock"

# --- Agent process ---
[agent]
socket_path = "/run/sober/agent.sock"
metrics_port = 9100
workspace_root = "/var/lib/sober/workspaces"
sandbox_profile = "standard"

# --- Scheduler ---
[scheduler]
tick_interval_secs = 1
agent_socket_path = "/run/sober/agent.sock"
socket_path = "/run/sober/scheduler.sock"
max_concurrent_jobs = 10
metrics_port = 9101
workspace_root = "/var/lib/sober/workspaces"
sandbox_profile = "standard"

# --- Web reverse proxy ---
[web]
host = "0.0.0.0"
port = 8080
api_upstream_url = "http://localhost:3000"
# Serve static files from disk instead of embedded assets.
# static_dir = "/var/lib/sober/static"

# --- MCP server connections ---
[mcp]
request_timeout_secs = 30
max_consecutive_failures = 3
idle_timeout_secs = 300

# --- Memory system tuning ---
[memory]
decay_half_life_days = 30
retrieval_boost = 0.2
prune_threshold = 0.1

# --- ACP agent (optional) ---
# Uncomment to enable Agent Client Protocol transport.
# [acp]
# name = ""
# command = "claude"
# args = ["acp"]
"##
    );
    Ok(())
}

/// Redact a database URL, showing only the scheme and host.
fn redact_url(url: &str) -> String {
    if url.is_empty() {
        return "(not set)".to_owned();
    }
    // Parse as a simple URL: scheme://user:pass@host:port/db
    if let Some(at_pos) = url.find('@')
        && let Some(scheme_end) = url.find("://")
    {
        let scheme = &url[..scheme_end + 3];
        let rest = &url[at_pos..];
        return format!("{scheme}***:***{rest}");
    }
    // Fallback: redact the whole thing
    "***REDACTED***".to_owned()
}

/// Redact an optional secret value.
fn redact_opt(value: &Option<String>) -> &str {
    match value {
        Some(_) => "***REDACTED***",
        None => "(not set)",
    }
}
