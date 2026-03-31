//! CLI argument definitions using clap derive.

use clap::{Parser, Subcommand, ValueEnum};

/// Sõber — CLI for administration, configuration, and runtime control.
#[derive(Debug, Parser)]
#[command(name = "sober", version, about)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage user accounts.
    #[command(subcommand)]
    User(UserCommand),

    /// Manage database migrations.
    #[command(subcommand)]
    Migrate(MigrateCommand),

    /// Validate and display configuration.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Manage the scheduler (requires running sober-scheduler).
    #[command(subcommand)]
    Scheduler(SchedulerCommand),

    /// Manage self-evolution events.
    #[command(subcommand)]
    Evolution(EvolutionCommand),

    /// Manage installed plugins.
    #[command(subcommand)]
    Plugin(PluginCommand),

    /// Manage skills.
    #[command(subcommand)]
    Skill(SkillCommand),

    /// Garbage collection commands.
    #[command(subcommand)]
    Gc(GcCommand),
}

/// User management subcommands.
#[derive(Debug, Subcommand)]
pub enum UserCommand {
    /// Create a new user account.
    Create {
        /// User's email address.
        #[arg(long)]
        email: String,

        /// User's display username.
        #[arg(long)]
        username: String,

        /// Grant admin role on creation.
        #[arg(long, default_value_t = false)]
        admin: bool,
    },

    /// Approve a pending user account (sets status to active).
    Approve {
        /// Email of the user to approve.
        email: String,
    },

    /// Disable an active user account.
    Disable {
        /// Email of the user to disable.
        email: String,
    },

    /// Re-enable a disabled user account.
    Enable {
        /// Email of the user to enable.
        email: String,
    },

    /// List user accounts.
    List {
        /// Filter by account status.
        #[arg(long)]
        status: Option<UserStatusFilter>,
    },

    /// Reset a user's password.
    ResetPassword {
        /// Email of the user whose password to reset.
        email: String,
    },
}

/// User status filter for the list command.
#[derive(Debug, Clone, ValueEnum)]
pub enum UserStatusFilter {
    /// Pending approval.
    Pending,
    /// Active accounts.
    Active,
    /// Disabled accounts.
    Disabled,
}

/// Migration management subcommands.
#[derive(Debug, Subcommand)]
pub enum MigrateCommand {
    /// Run all pending migrations.
    Run,

    /// Show migration status (applied and pending).
    Status,

    /// Revert the last applied migration.
    Revert,
}

/// Default path for the scheduler Unix domain socket.
pub const DEFAULT_SCHEDULER_SOCKET: &str = "/run/sober/scheduler.sock";

/// Scheduler management subcommands.
#[derive(Debug, Subcommand)]
pub enum SchedulerCommand {
    /// Check scheduler health.
    Health {
        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// List scheduled jobs.
    List {
        /// Filter by owner type (system, user, agent).
        #[arg(long)]
        owner_type: Option<String>,

        /// Filter by status (active, paused, cancelled, running).
        #[arg(long)]
        status: Option<String>,

        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// Pause the scheduler tick engine.
    Pause {
        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// Resume the scheduler tick engine.
    Resume {
        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// Force-run a specific job immediately.
    Run {
        /// Job ID (UUID) to run.
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// Cancel a scheduled job.
    Cancel {
        /// Job ID (UUID) to cancel.
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// Show details for a specific job.
    Get {
        /// Job ID (UUID).
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },

    /// List runs for a specific job.
    Runs {
        /// Job ID (UUID).
        job_id: String,

        /// Maximum number of runs to return.
        #[arg(long, default_value_t = 20)]
        limit: u32,

        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },
}

/// Configuration subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate that all required configuration is present.
    Validate,

    /// Display the resolved configuration (secrets redacted).
    Show {
        /// Show where each value came from (default/toml/env).
        #[arg(long)]
        source: bool,
    },

    /// Generate a default configuration file to stdout.
    Generate,
}

/// Default path for the agent Unix domain socket.
pub const DEFAULT_AGENT_SOCKET: &str = "/run/sober/agent.sock";

/// Evolution management subcommands.
#[derive(Debug, Subcommand)]
pub enum EvolutionCommand {
    /// List evolution events.
    List {
        /// Filter by evolution type (plugin, skill, instruction, automation).
        #[arg(long, value_name = "TYPE")]
        r#type: Option<String>,

        /// Filter by status (proposed, approved, executing, active, failed, rejected, reverted).
        #[arg(long)]
        status: Option<String>,
    },

    /// Approve a proposed evolution event and trigger execution.
    Approve {
        /// Evolution event ID (UUID).
        id: String,

        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },

    /// Reject a proposed evolution event.
    Reject {
        /// Evolution event ID (UUID).
        id: String,
    },

    /// Revert an active evolution event.
    Revert {
        /// Evolution event ID (UUID).
        id: String,

        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },

    /// Show current evolution autonomy configuration.
    Config,
}

/// Plugin management subcommands.
#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins.
    List {
        /// Filter by plugin kind (mcp, skill, wasm).
        #[arg(long)]
        kind: Option<String>,

        /// Filter by status (enabled, disabled, failed).
        #[arg(long)]
        status: Option<String>,
    },

    /// Enable a disabled plugin.
    Enable {
        /// Plugin ID (UUID).
        id: String,

        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },

    /// Disable an active plugin.
    Disable {
        /// Plugin ID (UUID).
        id: String,

        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },

    /// Remove a plugin entirely.
    Remove {
        /// Plugin ID (UUID).
        id: String,
    },
}

/// Garbage collection subcommands.
#[derive(Debug, Subcommand)]
pub enum GcCommand {
    /// Run blob garbage collection (requires running sober-scheduler).
    Blobs {
        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },
}

/// Skill management subcommands.
#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// List available skills.
    List {
        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },

    /// Reload the skill catalog.
    Reload {
        /// Path to agent socket.
        #[arg(long, default_value = DEFAULT_AGENT_SOCKET)]
        socket: String,
    },
}
