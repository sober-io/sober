# Database Setup

Before running `install.sh` or `docker compose up`, you need a PostgreSQL database and a
Qdrant instance. This page covers setting both up from scratch.

> **Docker Compose users** can skip this page — the Compose file provisions both services
> automatically.

---

## PostgreSQL

### Install

```bash
# Debian / Ubuntu
sudo apt install -y postgresql postgresql-client

# Fedora / RHEL
sudo dnf install -y postgresql-server postgresql
sudo postgresql-setup --initdb
sudo systemctl enable --now postgresql

# Arch
sudo pacman -S postgresql
sudo -u postgres initdb -D /var/lib/postgres/data
sudo systemctl enable --now postgresql
```

### Create Role and Database

Connect as the `postgres` superuser and run:

```bash
sudo -u postgres psql
```

```sql
-- Create a dedicated role with login and password
CREATE ROLE sober WITH LOGIN PASSWORD 'your-secure-password-here';

-- Create the database owned by that role
CREATE DATABASE sober OWNER sober;
```

Generate a strong password:

```bash
openssl rand -base64 32
```

### Verify Connectivity

```bash
psql "postgres://sober:your-secure-password-here@localhost:5432/sober" -c "SELECT 1"
```

You should see a single row with `1`. If this fails, check the next section.

### Troubleshooting: Authentication

PostgreSQL's default auth method varies by distro. If you get `peer authentication failed` or
`password authentication failed`, edit `pg_hba.conf`:

```bash
# Find the file
sudo -u postgres psql -c "SHOW hba_file"
```

Add or modify the line for local TCP connections:

```
# TYPE  DATABASE  USER   ADDRESS        METHOD
host    sober     sober  127.0.0.1/32   scram-sha-256
host    sober     sober  ::1/128        scram-sha-256
```

Then reload:

```bash
sudo systemctl reload postgresql
```

### Passwords with Special Characters

If your database password contains special characters, they must be
URL-encoded in the connection string. Common characters:

| Character | Encoded |
|-----------|---------|
| `@` | `%40` |
| `:` | `%3A` |
| `/` | `%2F` |
| `#` | `%23` |
| `{` | `%7B` |
| `}` | `%7D` |
| `\|` | `%7C` |

For example, a password of `p@ss:word` becomes:

```
postgres://sober:p%40ss%3Aword@localhost:5432/sober
```

If you get **"invalid port number"** during migration, this is almost always an
unescaped special character in the password breaking the URL parser.

### Remote PostgreSQL

If your PostgreSQL server is on a different host, the steps are the same — just run the SQL
on the remote server and use its address in the connection string:

```
postgres://sober:password@db.example.com:5432/sober
```

Ensure the server's `pg_hba.conf` allows connections from your Sober host's IP and that
port 5432 is reachable (firewall/security group).

---

## Qdrant

### Install

**Option A: Package / binary**

```bash
# Download the latest release
curl -fsSL https://github.com/qdrant/qdrant/releases/latest/download/qdrant-x86_64-unknown-linux-gnu.tar.gz \
  | sudo tar -xz -C /usr/local/bin

# Or use the official install script
curl -fsSL https://install.qdrant.tech | bash
```

**Option B: Docker (even on bare-metal Sober installs)**

```bash
docker run -d --name qdrant \
  -p 6333:6333 \
  -p 6334:6334 \
  -v qdrant_data:/qdrant/storage \
  --restart unless-stopped \
  qdrant/qdrant
```

**Option C: systemd service (bare-metal)**

Create `/etc/systemd/system/qdrant.service`:

```ini
[Unit]
Description=Qdrant Vector Database
After=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/qdrant --config-path /etc/qdrant/config.yaml
User=qdrant
Group=qdrant
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

```bash
sudo useradd --system --no-create-home qdrant
sudo mkdir -p /etc/qdrant /var/lib/qdrant
sudo chown qdrant:qdrant /var/lib/qdrant
sudo systemctl enable --now qdrant
```

### Ports

Qdrant exposes two ports:

| Port | Protocol | Used by |
|------|----------|---------|
| 6333 | HTTP/REST | Health checks, REST API clients |
| 6334 | gRPC | **Sõber** (`qdrant-client` Rust crate) |

Sõber connects via gRPC, so `config.toml` should point to port **6334**.

### Verify Connectivity

```bash
# REST health check (port 6333)
curl -s http://localhost:6333/healthz
# Expected: "healthz check passed"
```

> **Note:** The gRPC port (6334) does not respond to HTTP — you cannot health-check it
> with `curl`. Use the REST port for health checks.

### API Key (Optional)

If your Qdrant instance requires an API key, pass it to Sober via `config.toml`:

```toml
[qdrant]
url = "http://localhost:6334"
api_key = "your-qdrant-api-key"
```

Or via environment variable:

```bash
export SOBER_QDRANT_API_KEY="your-qdrant-api-key"
```

---

## Wire Credentials into Sober

Once both services are running, you have two options:

**Option A: Pass to install.sh**

```bash
sudo bash scripts/install.sh \
  --database-url "postgres://sober:your-password@localhost:5432/sober" \
  --yes
```

**Option B: Edit config.toml directly**

```toml
[database]
url = "postgres://sober:your-password@localhost:5432/sober"

[qdrant]
url = "http://localhost:6334"
```

File permissions should restrict access since the password is stored in plaintext:

```bash
sudo chmod 0600 /etc/sober/config.toml
sudo chown sober:sober /etc/sober/config.toml
```

---

## Next Step

With PostgreSQL and Qdrant ready, continue to [Installation](installation.md).
