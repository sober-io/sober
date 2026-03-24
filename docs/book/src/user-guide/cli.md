# CLI Reference

Sõber ships a single `sober` command-line tool for both offline administration and runtime control. It is compiled from the `sober-cli` crate via `cargo build -p sober-cli`.

```
sober config ...       # No dependencies required
sober user ...         # Requires PostgreSQL
sober migrate ...      # Requires PostgreSQL
sober scheduler ...    # Requires running sober-scheduler
```

---

## `sober migrate`

Manage database schema migrations. Migrations are embedded in the binary via `sqlx::migrate!()` and applied through `sqlx-cli`.

```
sober migrate run      # Apply all pending migrations
sober migrate status   # Show which migrations are applied and which are pending
sober migrate revert   # Revert the most recently applied migration
```

Requires a valid `DATABASE_URL` (or `database.url` in `config.toml`).

---

## `sober user`

Manage user accounts.

```
sober user create --email alice@example.com --username alice [--admin]
sober user approve alice@example.com
sober user disable alice@example.com
sober user enable alice@example.com
sober user list [--status pending|active|disabled]
sober user reset-password alice@example.com
```

`create` registers a new account. Accounts require approval before they can log in (unless `--admin` is passed, which approves automatically). `reset-password` generates and prints a temporary password.

---

## `sober config`

Inspect and generate configuration without connecting to a database.

### `sober config show [--source]`

Print the fully resolved configuration to stdout with all secrets redacted. Reads from `config.toml` overlaid with `SOBER_*` environment variables.

```
sober config show
sober config show --source
```

With `--source`, each line is annotated with `(default)` to indicate the value origin. Full per-value source tracking (default / toml / env) is a future enhancement.

Example output:

```
=== Sõber Configuration ===

[environment]
  mode = Development (default)

[database]
  url = postgres://***:***@localhost:5432/sober (default)
  max_connections = 10 (default)

[llm]
  base_url = https://openrouter.ai/api/v1 (default)
  api_key = ***REDACTED*** (default)
  model = anthropic/claude-sonnet-4 (default)
...
```

### `sober config validate`

Load and validate the full configuration. Prints `Configuration is valid.` on success, or a descriptive error on failure. Use this to catch missing required fields before starting services.

```
sober config validate
```

### `sober config generate`

Generate a fully annotated `config.toml` template and print it to stdout. Redirect to a file to create a starting configuration:

```
sober config generate > config.toml
```

The generated template includes all sections and fields with comments explaining each setting.

---

## `sober scheduler`

Manage the scheduler at runtime. Connects to `sober-scheduler` via Unix domain socket (gRPC). The scheduler must be running.

All scheduler commands accept a `--socket` flag (default: `/run/sober/scheduler.sock`) to point at a non-default socket path.

### Health

```
sober scheduler health
sober scheduler health --socket /run/sober/scheduler.sock
```

Prints `healthy: true` and the scheduler's version string.

### List jobs

```
sober scheduler list
sober scheduler list --owner-type user
sober scheduler list --status paused
```

Optional filters: `--owner-type` (`system`, `user`, `agent`), `--status` (`active`, `paused`, `cancelled`, `running`).

Output per job:

```
  id:           550e8400-e29b-41d4-a716-446655440000
  name:         daily-memory-prune
  status:       active
  schedule:     0 3 * * *
  owner_type:   system
  next_run_at:  2026-03-24T03:00:00Z
  created_at:   2026-01-15T12:00:00Z
```

### Get a specific job

```
sober scheduler get <job-id>
```

### Cancel a job

```
sober scheduler cancel <job-id>
```

### Force-run a job immediately

```
sober scheduler run <job-id>
```

Dispatches the job immediately regardless of its next scheduled time. Prints `force run accepted` or `force run rejected`.

### Pause / resume the tick engine

```
sober scheduler pause
sober scheduler resume
```

Pausing the tick engine stops all job scheduling globally until resumed. Useful during maintenance.

### List runs for a job

```
sober scheduler runs <job-id>
sober scheduler runs <job-id> --limit 10
```

Prints run IDs, statuses, start and finish times, and any error messages.

---

## Configuration

The CLI loads configuration via `AppConfig::load()`, which reads from (in priority order):

1. `SOBER_CONFIG` environment variable — path to a `config.toml` file.
2. `/etc/sober/config.toml` — production default.
3. `./config.toml` — development default.
4. `SOBER_*` environment variables — override individual values from the file.

See `sober config generate` for the full list of available settings and their defaults.
