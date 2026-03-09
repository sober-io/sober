//! Database migration commands.
//!
//! Uses sqlx's migration API with migrations embedded in the binary
//! via `sqlx::migrate!()`.

use anyhow::{Context, Result};
use sqlx::PgPool;

/// Run all pending migrations.
pub async fn run(pool: &PgPool) -> Result<()> {
    println!("Running migrations...");

    let migrator = sqlx::migrate!("../../migrations");
    migrator
        .run(pool)
        .await
        .context("failed to run migrations")?;

    println!("Migrations applied successfully.");
    Ok(())
}

/// Show the status of all migrations (applied vs pending).
pub async fn status(pool: &PgPool) -> Result<()> {
    let migrator = sqlx::migrate!("../../migrations");

    // Fetch which migrations have already been applied.
    let applied: Vec<_> = sqlx::query_as::<_, (i64, String)>(
        "SELECT version, description FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let applied_versions: std::collections::HashSet<i64> =
        applied.iter().map(|(v, _)| *v).collect();

    println!("{:<20} {:<50} STATUS", "VERSION", "DESCRIPTION");
    println!("{}", "-".repeat(80));

    for migration in migrator.iter() {
        let status = if applied_versions.contains(&migration.version) {
            "applied"
        } else {
            "pending"
        };

        println!(
            "{:<20} {:<50} {}",
            migration.version, migration.description, status,
        );
    }

    let pending_count = migrator
        .iter()
        .filter(|m| !applied_versions.contains(&m.version))
        .count();

    println!(
        "\n{} total, {} applied, {} pending.",
        migrator.iter().count(),
        applied.len(),
        pending_count,
    );

    Ok(())
}

/// Revert the last applied migration.
pub async fn revert(pool: &PgPool) -> Result<()> {
    let migrator = sqlx::migrate!("../../migrations");

    println!("Reverting last migration...");
    migrator
        .undo(pool, 1)
        .await
        .context("failed to revert migration")?;

    println!("Migration reverted successfully.");
    Ok(())
}
