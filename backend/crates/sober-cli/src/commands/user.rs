//! User management commands.

use anyhow::{Context, Result, bail};
use sober_core::types::{CreateUser, RoleKind, UserRepo, UserStatus};
use sober_crypto::password::hash_password;
use sober_db::PgUserRepo;

use crate::cli::UserStatusFilter;

/// Create a new user account.
///
/// The user is created with `Pending` status. If `--admin` is specified,
/// both the `user` and `admin` roles are assigned; otherwise only `user`.
pub async fn create(repo: &PgUserRepo, email: &str, username: &str, admin: bool) -> Result<()> {
    let password = rpassword::prompt_password("Password: ").context("failed to read password")?;

    if password.is_empty() {
        bail!("password cannot be empty");
    }

    let confirm = rpassword::prompt_password("Confirm password: ")
        .context("failed to read password confirmation")?;

    if password != confirm {
        bail!("passwords do not match");
    }

    let password_hash = hash_password(&password).context("failed to hash password")?;

    let input = CreateUser {
        email: email.to_owned(),
        username: username.to_owned(),
        password_hash,
    };

    let roles = if admin {
        vec![RoleKind::User, RoleKind::Admin]
    } else {
        vec![RoleKind::User]
    };

    let user = repo
        .create_with_roles(input, &roles)
        .await
        .context("failed to create user")?;

    let role_str = if admin { "user, admin" } else { "user" };
    println!(
        "Created user: {} ({}) [status: {}, roles: {}]",
        user.email, user.id, user.status, role_str,
    );

    Ok(())
}

/// Approve a pending user account by setting status to Active.
pub async fn approve(repo: &PgUserRepo, email: &str) -> Result<()> {
    let user = repo
        .get_by_email(email)
        .await
        .context("failed to find user")?;

    if user.status != UserStatus::Pending {
        bail!(
            "user {} has status '{}', expected 'pending'",
            email,
            user.status,
        );
    }

    repo.update_status(user.id, UserStatus::Active)
        .await
        .context("failed to approve user")?;

    println!("Approved user: {} ({})", email, user.id);
    Ok(())
}

/// Disable a user account.
pub async fn disable(repo: &PgUserRepo, email: &str) -> Result<()> {
    let user = repo
        .get_by_email(email)
        .await
        .context("failed to find user")?;

    if user.status == UserStatus::Disabled {
        bail!("user {} is already disabled", email);
    }

    repo.update_status(user.id, UserStatus::Disabled)
        .await
        .context("failed to disable user")?;

    println!("Disabled user: {} ({})", email, user.id);
    Ok(())
}

/// Re-enable a disabled user account.
pub async fn enable(repo: &PgUserRepo, email: &str) -> Result<()> {
    let user = repo
        .get_by_email(email)
        .await
        .context("failed to find user")?;

    if user.status != UserStatus::Disabled {
        bail!(
            "user {} has status '{}', expected 'disabled'",
            email,
            user.status,
        );
    }

    repo.update_status(user.id, UserStatus::Active)
        .await
        .context("failed to enable user")?;

    println!("Enabled user: {} ({})", email, user.id);
    Ok(())
}

/// List user accounts, optionally filtered by status.
pub async fn list(repo: &PgUserRepo, status_filter: Option<UserStatusFilter>) -> Result<()> {
    let status = status_filter.map(|f| match f {
        UserStatusFilter::Pending => UserStatus::Pending,
        UserStatusFilter::Active => UserStatus::Active,
        UserStatusFilter::Disabled => UserStatus::Disabled,
    });

    let users = repo.list(status).await.context("failed to list users")?;

    if users.is_empty() {
        println!("No users found.");
        return Ok(());
    }

    // Print table header
    println!(
        "{:<38} {:<30} {:<20} {:<10} CREATED",
        "ID", "EMAIL", "USERNAME", "STATUS",
    );
    println!("{}", "-".repeat(120));

    for user in &users {
        println!(
            "{:<38} {:<30} {:<20} {:<10} {}",
            user.id,
            user.email,
            user.username,
            user.status,
            user.created_at.format("%Y-%m-%d %H:%M"),
        );
    }

    println!("\n{} user(s) total.", users.len());
    Ok(())
}

/// Reset a user's password.
pub async fn reset_password(repo: &PgUserRepo, email: &str) -> Result<()> {
    let user = repo
        .get_by_email(email)
        .await
        .context("failed to find user")?;

    let password =
        rpassword::prompt_password("New password: ").context("failed to read password")?;

    if password.is_empty() {
        bail!("password cannot be empty");
    }

    let confirm = rpassword::prompt_password("Confirm new password: ")
        .context("failed to read password confirmation")?;

    if password != confirm {
        bail!("passwords do not match");
    }

    let password_hash = hash_password(&password).context("failed to hash password")?;

    repo.update_password_hash(user.id, &password_hash)
        .await
        .context("failed to update password")?;

    println!("Password reset for user: {} ({})", email, user.id);
    Ok(())
}
