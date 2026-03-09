//! Configuration validation and display commands.

use anyhow::Result;
use sober_core::config::AppConfig;

/// Validate that all required configuration is present.
///
/// Attempts to load the configuration from environment variables and
/// reports any missing or invalid values.
pub fn validate() -> Result<()> {
    match AppConfig::load_from_env() {
        Ok(_) => {
            println!("Configuration is valid.");
            Ok(())
        }
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(1);
        }
    }
}

/// Display the resolved configuration with secrets redacted.
pub fn show() -> Result<()> {
    let config = match AppConfig::load_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== Sõber Configuration ===\n");

    // Database
    println!("[database]");
    println!(
        "  url                = {}",
        redact_url(&config.database.url)
    );
    println!("  max_connections    = {}", config.database.max_connections);

    // Qdrant
    println!("\n[qdrant]");
    println!("  url                = {}", config.qdrant.url);
    println!(
        "  api_key            = {}",
        redact_opt(&config.qdrant.api_key)
    );

    // LLM
    println!("\n[llm]");
    println!("  base_url           = {}", config.llm.base_url);
    println!("  api_key            = {}", redact_opt(&config.llm.api_key));
    println!("  model              = {}", config.llm.model);
    println!("  max_tokens         = {}", config.llm.max_tokens);
    println!("  embedding_model    = {}", config.llm.embedding_model);

    // Server
    println!("\n[server]");
    println!("  host               = {}", config.server.host);
    println!("  port               = {}", config.server.port);

    // Auth
    println!("\n[auth]");
    println!(
        "  session_secret     = {}",
        redact_opt(&config.auth.session_secret)
    );
    println!("  session_ttl_secs   = {}", config.auth.session_ttl_seconds);

    // SearXNG
    println!("\n[searxng]");
    println!("  url                = {}", config.searxng.url);

    // Admin
    println!("\n[admin]");
    println!(
        "  socket_path        = {}",
        config.admin.socket_path.display()
    );

    // Scheduler
    println!("\n[scheduler]");
    println!(
        "  tick_interval_secs = {}",
        config.scheduler.tick_interval_secs
    );
    println!(
        "  agent_socket_path  = {}",
        config.scheduler.agent_socket_path.display()
    );
    println!(
        "  admin_socket_path  = {}",
        config.scheduler.admin_socket_path.display()
    );
    println!(
        "  max_concurrent_jobs = {}",
        config.scheduler.max_concurrent_jobs
    );

    // MCP
    println!("\n[mcp]");
    println!(
        "  request_timeout_secs      = {}",
        config.mcp.request_timeout_secs
    );
    println!(
        "  max_consecutive_failures  = {}",
        config.mcp.max_consecutive_failures
    );
    println!(
        "  idle_timeout_secs         = {}",
        config.mcp.idle_timeout_secs
    );

    // Memory
    println!("\n[memory]");
    println!(
        "  decay_half_life_days = {}",
        config.memory.decay_half_life_days
    );
    println!("  retrieval_boost      = {}", config.memory.retrieval_boost);
    println!("  prune_threshold      = {}", config.memory.prune_threshold);

    // ACP
    println!("\n[acp]");
    match &config.acp {
        Some(acp) => {
            println!("  name               = {}", acp.name);
            println!("  command            = {}", acp.command);
            println!("  args               = {:?}", acp.args);
        }
        None => {
            println!("  (not configured)");
        }
    }

    // Environment
    println!("\n[environment]");
    println!("  mode               = {:?}", config.environment);

    Ok(())
}

/// Redact a database URL, showing only the scheme and host.
fn redact_url(url: &str) -> String {
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
