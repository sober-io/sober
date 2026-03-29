//! `sober` — unified CLI for administration, configuration, and runtime control.

mod cli;
mod commands;

use anyhow::{Context, Result};
use clap::Parser;
use sober_core::config::AppConfig;
use sober_db::{DatabaseConfig, PgEvolutionRepo, PgPluginRepo, PgUserRepo, create_pool};
use tracing_subscriber::EnvFilter;

use cli::{
    Cli, Command, ConfigCommand, EvolutionCommand, MigrateCommand, PluginCommand, SkillCommand,
    UserCommand,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal tracing for CLI output.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Config(cmd) => run_config(cmd),
        Command::User(cmd) => run_user(cmd).await,
        Command::Migrate(cmd) => run_migrate(cmd).await,
        Command::Scheduler(cmd) => commands::scheduler::handle(cmd).await,
        Command::Evolution(cmd) => run_evolution(cmd).await,
        Command::Plugin(cmd) => run_plugin(cmd).await,
        Command::Skill(cmd) => run_skill(cmd).await,
    }
}

/// Execute a config subcommand (no database required).
fn run_config(cmd: ConfigCommand) -> Result<()> {
    match cmd {
        ConfigCommand::Validate => commands::config::validate(),
        ConfigCommand::Show { source } => commands::config::show(source),
        ConfigCommand::Generate => commands::config::generate(),
    }
}

/// Execute a user subcommand (requires database connection).
async fn run_user(cmd: UserCommand) -> Result<()> {
    let pool = connect_db().await?;
    let repo = PgUserRepo::new(pool);

    match cmd {
        UserCommand::Create {
            email,
            username,
            admin,
        } => commands::user::create(&repo, &email, &username, admin).await,
        UserCommand::Approve { email } => commands::user::approve(&repo, &email).await,
        UserCommand::Disable { email } => commands::user::disable(&repo, &email).await,
        UserCommand::Enable { email } => commands::user::enable(&repo, &email).await,
        UserCommand::List { status } => commands::user::list(&repo, status).await,
        UserCommand::ResetPassword { email } => commands::user::reset_password(&repo, &email).await,
    }
}

/// Execute a migration subcommand (requires database connection).
async fn run_migrate(cmd: MigrateCommand) -> Result<()> {
    let pool = connect_db().await?;

    match cmd {
        MigrateCommand::Run => commands::migrate::run(&pool).await,
        MigrateCommand::Status => commands::migrate::status(&pool).await,
        MigrateCommand::Revert => commands::migrate::revert(&pool).await,
    }
}

/// Execute an evolution subcommand (requires database, some need agent gRPC).
async fn run_evolution(cmd: EvolutionCommand) -> Result<()> {
    let pool = connect_db().await?;
    let repo = PgEvolutionRepo::new(pool);

    match cmd {
        EvolutionCommand::List { r#type, status } => {
            commands::evolution::list(&repo, r#type, status).await
        }
        EvolutionCommand::Approve { id, socket } => {
            commands::evolution::approve(&repo, &id, &socket).await
        }
        EvolutionCommand::Reject { id } => commands::evolution::reject(&repo, &id).await,
        EvolutionCommand::Revert { id, socket } => {
            commands::evolution::revert(&repo, &id, &socket).await
        }
        EvolutionCommand::Config => commands::evolution::config(&repo).await,
    }
}

/// Execute a plugin subcommand (requires database, some need agent gRPC).
async fn run_plugin(cmd: PluginCommand) -> Result<()> {
    let pool = connect_db().await?;
    let repo = PgPluginRepo::new(pool);

    match cmd {
        PluginCommand::List { kind, status } => commands::plugin::list(&repo, kind, status).await,
        PluginCommand::Enable { id, socket } => commands::plugin::enable(&repo, &id, &socket).await,
        PluginCommand::Disable { id, socket } => {
            commands::plugin::disable(&repo, &id, &socket).await
        }
        PluginCommand::Remove { id } => commands::plugin::remove(&repo, &id).await,
    }
}

/// Execute a skill subcommand (requires running sober-agent).
async fn run_skill(cmd: SkillCommand) -> Result<()> {
    match cmd {
        SkillCommand::List { socket } => commands::skill::list(&socket).await,
        SkillCommand::Reload { socket } => commands::skill::reload(&socket).await,
    }
}

/// Connect to the database using the resolved application configuration.
///
/// Uses a single connection (max_connections=1) since CLI operations are
/// sequential.
async fn connect_db() -> Result<sqlx::PgPool> {
    let app_config = AppConfig::load().context("failed to load configuration")?;

    let config = DatabaseConfig {
        url: app_config.database.url,
        max_connections: 1,
    };

    create_pool(&config)
        .await
        .context("failed to connect to database")
}
