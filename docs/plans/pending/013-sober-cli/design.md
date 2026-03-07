# 013 --- sober-cli

**Date:** 2026-03-06

## Overview

The `sober` binary is a CLI admin tool for offline operations --- things that do not require a running API server. It connects directly to PostgreSQL.

v1 commands only. `soberctl` (runtime ops via Unix admin socket) is deferred to post-v1.

## Command Structure (clap derive)

```
sober
  user
    create --email <EMAIL> --username <USERNAME> [--admin]   # Create user (prompts for password)
    approve <EMAIL>                                          # Approve pending user
    disable <EMAIL>                                          # Disable user
    enable <EMAIL>                                           # Re-enable disabled user
    list [--status pending|active|disabled]                  # List users with optional filter
    reset-password <EMAIL>                                   # Reset password (prompts for new)
  migrate
    run                                                      # Apply pending migrations
    status                                                   # Show migration status
    revert                                                   # Revert last migration
  config
    validate                                                 # Validate env config (checks all required vars)
    show                                                     # Show resolved config (redacts secrets)
```

## Database Connection

- Reads `DATABASE_URL` from env (or `.env` file via dotenvy).
- Creates a sqlx `PgPool` with `max_connections=1` (CLI does not need concurrency).
- Fails fast with a clear error if the connection fails.

## User Management

- **create**: Validates email/username format, prompts for password on stdin (with confirmation), hashes with Argon2id via sober-crypto.
- **approve**: Sets user status to Active, assigns the `user` role.
- **disable**: Sets user status to Disabled, expires all sessions.
- **enable**: Sets user status to Active (only if previously Disabled).
- **list**: Displays a table of users (id, email, username, status, created_at).
- **reset-password**: Prompts for new password, hashes, updates DB.

## Migration Management

- Uses sqlx's migration API directly (`sqlx::migrate!()` macro or the runtime migration runner).
- **run**: Applies all pending migrations from `backend/migrations/`.
- **status**: Shows which migrations have been applied and which are pending.
- **revert**: Reverts the most recently applied migration.

## Config Validation

- **validate**: Loads `AppConfig` from env, reports missing/invalid vars without starting anything.
- **show**: Loads and displays resolved config with secrets redacted (API keys shown as `sk-...***`).

## Output Format

- Human-readable table format for list commands (using a simple text table, no heavy deps).
- Success/error messages on stdout/stderr.
- Exit code 0 on success, 1 on error.

## Error Handling

- Uses `anyhow` for top-level error handling (binary, not library).
- Prints user-friendly error messages, not stack traces.
- Specific exit codes not needed for v1 (just 0/1).

## Dependencies

| Crate | Purpose |
|-------|---------|
| sober-core | Types, config, errors |
| sober-crypto | Password hashing (Argon2id) --- new dependency, update architecture docs |
| clap (derive feature) | Argument parsing |
| sqlx (postgres, runtime-tokio, migrate) | Database access and migrations |
| tokio (rt, macros) | Async runtime for sqlx |
| dotenvy | `.env` file loading |
| anyhow | Top-level error handling |
| tracing, tracing-subscriber | Structured logging |
| rpassword | Secure password input on terminal |
