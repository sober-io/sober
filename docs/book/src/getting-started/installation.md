# Installation

Sõber can be installed in three ways. Choose the method that matches your deployment target.

| Method | Best For |
|--------|----------|
| [install.sh](#method-1-installsh-recommended) | Production servers, single-host deployments |
| [Docker Compose](#method-2-docker-compose) | Local development, self-hosted with containers |
| [From Source](#method-3-from-source) | Contributors, custom builds |

---

## Method 1: install.sh (Recommended)

The install script downloads the latest Sõber release, creates a `sober` system user, installs
binaries to `/opt/sober/bin`, configures systemd services, generates `/etc/sober/config.toml`,
and runs database migrations automatically.

### Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash
```

The script will prompt for required values (database URL, LLM API key, etc.) unless you pass
them as flags.

### Non-Interactive Install

Pass configuration values directly to skip prompts:

```bash
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash -s -- \
  --database-url "postgres://sober:secret@localhost:5432/sober" \
  --llm-api-key "sk-..." \
  --llm-model "anthropic/claude-sonnet-4" \
  --yes
```

### Available Flags

| Flag | Description |
|------|-------------|
| `--user <name>` | System user to create (default: `sober`) |
| `--group <name>` | System group to create (default: same as `--user`) |
| `--version <tag>` | Release version to install (default: latest) |
| `--yes` | Skip confirmation prompts |
| `--database-url <url>` | PostgreSQL connection string |
| `--llm-base-url <url>` | LLM API base URL (default: OpenRouter) |
| `--llm-api-key <key>` | LLM provider API key |
| `--llm-model <id>` | Model identifier |
| `--uninstall` | Remove Sõber and all installed files |
| `--help` | Show usage information |

### What the Script Does

1. Downloads the release archive for your architecture (x86_64 or aarch64)
2. Creates a `sober` system user (or the user specified with `--user`)
3. Installs binaries to `/opt/sober/bin` and adds them to `PATH`
4. Writes `/etc/sober/config.toml` from your provided values
5. Installs systemd unit files for `sober-agent`, `sober-api`, `sober-scheduler`, and `sober-web`
6. Runs `sober migrate run` to initialise the database schema
7. Enables and starts all services

### Upgrading

Re-run the same install command to upgrade to the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash
```

The script detects an existing installation, stops the services, replaces the binaries, runs
any new migrations, and restarts the services.

### Uninstalling

```bash
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash -s -- --uninstall
```

This removes binaries and systemd units. Configuration and data are preserved.
To remove everything and start from scratch:

```bash
# Uninstall binaries and services
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash -s -- --uninstall

# Remove configuration and data
sudo rm -rf /etc/sober /opt/sober/data

# Remove the system user (optional)
sudo userdel sober
sudo groupdel sober

# Reinstall
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash
```

### Force Fresh Install

The script detects an existing installation by checking for `/opt/sober/bin/sober-api`
and switches to upgrade mode. To force a fresh install:

```bash
sudo rm -rf /opt/sober/bin
curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash
```

### Post-Install

After the install completes, all four services will be running:

```bash
systemctl status sober-web sober-api sober-agent sober-scheduler
```

The web UI is available at `http://<your-host>:8080` by default.

---

## Method 2: Docker Compose

The Docker Compose setup provides a fully self-contained stack including PostgreSQL, Qdrant,
SearXNG, and all Sõber services.

### Setup

```bash
# Clone the repository
git clone https://github.com/sober-io/sober.git
cd sober

# Copy the example environment file
cp .env.example .env

# Edit with your values (LLM API key is required)
$EDITOR .env
```

Alternatively, use the TOML-based configuration:

```bash
cp infra/config/config.toml.example config.toml
$EDITOR config.toml
```

### Start the Stack

```bash
docker compose up -d
```

On first start, the `migrate` service runs automatically before the other services start.

### Services and Ports

| Service | Purpose | Default Port |
|---------|---------|-------------|
| `postgres` | PostgreSQL 17 | internal |
| `qdrant` | Vector database | internal |
| `searxng` | Web search aggregation | internal |
| `sober-agent` | Agent gRPC server | Unix socket (internal) |
| `sober-api` | HTTP/WebSocket API | `3000` |
| `sober-scheduler` | Autonomous scheduler | Unix socket (internal) |
| `sober-web` | Frontend and reverse proxy | `8088` |
| `migrate` | Runs migrations on startup | — |

The web UI is available at `http://localhost:8088`.

### Stopping and Removing

```bash
# Stop services (preserves volumes)
docker compose down

# Stop and remove all data
docker compose down -v
```

---

## Method 3: From Source

Building from source requires the full set of [prerequisites](prerequisites.md#from-source)
including Rust, Node.js 24, pnpm, and `protoc`.

### Build

```bash
# Clone
git clone https://github.com/sober-io/sober.git
cd sober

# Build the backend (release mode)
cd backend
cargo build --release -q

# Build the frontend
cd ../frontend
pnpm install --silent
pnpm build --silent
```

The compiled binaries are in `backend/target/release/`:

| Binary | Description |
|--------|-------------|
| `sober-web` | Web server with embedded frontend |
| `sober-api` | API gateway |
| `sober-agent` | Agent gRPC server |
| `sober-scheduler` | Scheduler service |
| `sober` | Unified CLI (admin, config, runtime control) |

### Configure and Run

Copy the example config and edit it:

```bash
cp infra/config/config.toml.example config.toml
$EDITOR config.toml
```

Run migrations, then start each service:

```bash
./backend/target/release/sober migrate run

./backend/target/release/sober-agent &
./backend/target/release/sober-api &
./backend/target/release/sober-scheduler &
./backend/target/release/sober-web
```

For development, use the `justfile` shortcuts:

```bash
just dev    # Start all services in dev mode
just build  # Build everything
just test   # Run all tests
```

---

## Next Step

With Sõber installed, continue to [Configuration](configuration.md) to review and customise
your settings before the [First Run](first-run.md).
