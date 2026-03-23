//! CLI argument definitions using clap derive.

use clap::{Parser, Subcommand, ValueEnum};

/// Sõber — offline admin CLI for database migrations, user management,
/// and configuration validation.
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
