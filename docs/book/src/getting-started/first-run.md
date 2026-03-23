# First Run

This page walks you through starting Sõber for the first time, creating your account, and
sending your first message to the agent.

---

## Step 1: Open the Web UI

Navigate to the web UI in your browser.

| Deployment Method | URL |
|-------------------|-----|
| Docker Compose | `http://localhost:8088` |
| Binary install | `http://<your-host>:8080` |
| From source (dev) | `http://localhost:8080` |

You should see the Sõber welcome screen. If the page does not load, check that all services are
running:

```bash
# Docker Compose
docker compose ps

# systemd (binary install)
systemctl status sober-web sober-api sober-agent sober-scheduler

# From source — check that sober-web is running and listening on port 8080
```

---

## Step 2: Create Your Account

Click **Get Started** or **Register** on the welcome screen.

Fill in:

- **Username** — your display name inside Sõber
- **Email address** — used for login and notifications
- **Password** — must meet the minimum length requirement

Click **Create account**. The first account created on a fresh installation is automatically
granted admin privileges.

> If registration is disabled (set by the admin), contact your administrator for an invite.

---

## Step 3: Start a Conversation

After logging in you will land on the main chat interface.

1. Click **New conversation** in the left sidebar (or press `N`).
2. A new conversation is created and the input field is focused.
3. Type a message and press **Enter** (or click the send button) to send it.

Sõber will begin streaming a response. You will see text appear token by token as the LLM
generates it.

---

## Step 4: Basic Agent Interaction

Here are a few things to try on your first run:

**Ask about Sõber itself:**

> "What can you do?"

The agent will describe its current capabilities, available tools, and any configured plugins.

**Check your memory:**

> "What do you know about me so far?"

On a fresh install, the agent has no long-term memory of you yet. It will say so. After a few
conversations, it will begin building a memory of your preferences and context.

**Ask for a task:**

> "Create a file called hello.txt in my workspace with 'Hello, world!' as the content."

If a workspace is configured, the agent can create and manage files on your behalf using its
workspace tools.

---

## Step 5: Admin Settings

As the first user (admin), you have access to the admin panel. Navigate to **Settings →
Administration** or visit `/settings/admin` directly.

From the admin panel you can:

- **View system status** — uptime, connected services, scheduler state
- **Manage users** — invite new users, change roles, deactivate accounts
- **Install plugins** — browse and install WASM plugins from the registry
- **Review audit logs** — see all security events and agent actions
- **Configure soul.md** — edit the base agent personality for all users

---

## Step 6: Using the CLI

The `sober` and `soberctl` CLI tools give you administrative access without the web UI.

```bash
# Check system status (requires sober-api to be running)
soberctl status

# List all users
soberctl users list

# View recent agent activity
soberctl agent logs --tail 50

# Run database migrations (offline, does not need the API)
sober migrate run

# Validate your configuration
sober config validate
```

Run `sober --help` or `soberctl --help` for the full command reference.

---

## Troubleshooting

**The page loads but I get a 502 error when I try to log in.**

The web server is running but `sober-api` is not reachable. Check that `sober-api` is running
and listening on the configured port (default `3000`). Also verify `web.api_upstream_url` in
your config points to the correct address.

**Registration fails with "database connection error".**

`sober-api` cannot reach PostgreSQL. Check your `SOBER_DATABASE_URL` and ensure the database
is running and accepting connections.

**The agent responds with "LLM provider error".**

Your `SOBER_LLM_API_KEY` is missing, invalid, or the model name in `SOBER_LLM_MODEL` is not
available on your chosen provider. Verify the key and model in your config, then restart the
API service.

**Streaming responses do not appear (the page hangs after sending).**

This is usually a WebSocket connectivity issue. Check that your reverse proxy (if any) is
configured to pass WebSocket upgrade headers. For nginx, add:

```nginx
proxy_http_version 1.1;
proxy_set_header Upgrade $http_upgrade;
proxy_set_header Connection "upgrade";
```

---

## Where to Go Next

| I want to… | Go to… |
|------------|--------|
| Understand how the system is designed | [Architecture Overview](../architecture/overview.md) |
| Install and manage plugins | [Plugins](../plugins/overview.md) |
| Configure memory and workspaces | [User Guide](../user-guide/overview.md) |
| Write my own plugin | [Writing Plugins](../plugins/writing-plugins.md) |
| Contribute to Sõber | [Contributing](../contributing.md) |
