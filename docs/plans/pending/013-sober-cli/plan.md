# 013 --- sober-cli: Implementation Plan

## Steps

1. Add dependencies to sober-cli `Cargo.toml`. Update the `[[bin]]` section --- v1 only produces the `sober` binary (`soberctl` is deferred).
2. Create module structure:
   - `src/bin/sober.rs` (or `src/main.rs`) --- entrypoint
   - `src/cli.rs` --- clap structs
   - `src/commands/mod.rs` --- command dispatch
   - `src/commands/user.rs` --- user management commands
   - `src/commands/migrate.rs` --- migration commands
   - `src/commands/config.rs` --- config validation/display
   - `src/db.rs` --- database connection helper
3. Implement `cli.rs`: top-level `Cli` struct with subcommands using clap derive.
4. Implement `db.rs`: `create_pool(database_url)` helper --- `PgPool` with `max_connections=1`.
5. Implement `commands/user.rs`: create, approve, disable, enable, list, reset_password.
6. Implement `commands/migrate.rs`: run, status, revert using sqlx migration API.
7. Implement `commands/config.rs`: validate and show.
8. Implement main: parse CLI, init tracing (minimal), connect to DB, dispatch command.
9. Write integration tests (require running Postgres): create user, approve, list, disable flow.
10. Test migration commands against a test database.

## Acceptance Criteria

- `sober --help` shows all commands and subcommands.
- `sober user create` creates a user with Pending status and hashed password.
- `sober user approve` changes status to Active and assigns the `user` role.
- `sober user list` displays a formatted table.
- `sober migrate run` applies migrations.
- `sober migrate status` shows applied/pending migrations.
- `sober config validate` reports missing env vars clearly.
- `cargo clippy` passes clean.
- Exit codes: 0 on success, 1 on error.


