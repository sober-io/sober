# MCP Servers

MCP (Model Context Protocol) is an open standard that allows AI assistants to connect to external tools and data sources. Sõber implements both an MCP client (to consume external MCP servers) and an MCP server (to expose its own capabilities).

By adding MCP servers to Sõber, you give the agent access to additional tools — file systems, databases, APIs, and more — without modifying the core codebase.

---

## How MCP Works

An MCP server is a process that speaks the MCP JSON-RPC protocol. It advertises:

- **Tools** — callable functions with typed input schemas (analogous to Sõber's built-in tools).
- **Resources** — named data sources the agent can read (files, database records, etc.).
- **Prompts** — pre-built prompt templates the server provides.

When the agent starts a conversation, it queries all connected MCP servers for their tool lists and adds those tools to its tool registry alongside the built-in tools. The LLM sees all tools in a unified list and can call any of them.

---

## Transport Types

Sõber supports two MCP transport types:

### stdio (subprocess)

The MCP server is launched as a child process. Communication happens over stdin/stdout. This is the most common transport for local MCP servers.

```json
{
  "name": "filesystem",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp/data"],
  "env": {}
}
```

### SSE (Server-Sent Events)

The MCP server runs independently and exposes an HTTP SSE endpoint. Sõber connects as an HTTP client.

```json
{
  "name": "my-api-server",
  "url": "http://localhost:3100/sse"
}
```

---

## MCP Server Lifecycle

```
DISCOVER → INITIALIZE → CONNECTED → (IDLE_TIMEOUT / FAILURE) → RESTART
```

1. **DISCOVER** — Sõber looks up registered MCP servers for the current user from the database.
2. **INITIALIZE** — Sõber launches the server process (stdio) or connects (SSE) and performs the MCP initialize handshake. The server responds with its capabilities and info.
3. **CONNECTED** — The server is live. Tools are registered in the agent's tool registry.
4. **FAILURE** — If a request fails, the consecutive failure counter increments. After `max_consecutive_failures` failures, the server is marked unavailable.
5. **RESTART** — After a `restart_cooldown_secs` pause, Sõber attempts to reconnect.
6. **IDLE_TIMEOUT** — If no requests are made to the server for `idle_timeout_secs`, the connection is closed and the server is marked inactive. It reconnects on next use.

---

## Configuration Options

Global MCP behaviour is configured in `config.toml`:

```toml
[mcp]
request_timeout_secs = 30       # Timeout for individual tool calls
max_consecutive_failures = 3    # Failures before marking a server unavailable
idle_timeout_secs = 300         # Seconds of inactivity before disconnecting
restart_cooldown_secs = 5       # Wait before reconnection attempts (not in config.toml, internal default)
```

Per-server configuration (name, command, args, env) is stored in the database as a `Plugin` record with `plugin_type = "mcp_server"`. Credentials (e.g. API keys passed as environment variables) are encrypted via the secret vault and injected at launch time.

---

## Adding an MCP Server

### Via the agent (conversational)

Ask the agent to add an MCP server:

> "Add the filesystem MCP server and give it read access to ~/projects."

The agent will:
1. Use `store_secret` to encrypt any credentials.
2. Register the server in the plugin database.
3. Confirm once the server is connected and its tools are available.

### Via the API

```http
POST /api/v1/plugins
Content-Type: application/json

{
  "name": "filesystem",
  "plugin_type": "mcp_server",
  "scope": "user",
  "config": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
  }
}
```

---

## Credentials and Environment Variables

MCP servers often need credentials (API keys, tokens) passed as environment variables. These are handled through the secret vault:

1. Store the credential: ask the agent to `store_secret` with `secret_type: "mcp_server"` and `scope: "user"`.
2. When the MCP server is launched, Sõber's `sober-mcp` crate decrypts the relevant secrets and injects them into the server's environment.

This ensures that credentials are never stored in plaintext in the database or config files.

---

## MCP Servers as Sandboxed Processes

stdio MCP servers run as child processes. Sõber uses `sober-sandbox` (bwrap-based) to optionally restrict server process capabilities — limiting filesystem access, network access, and resource usage. The sandbox profile applied depends on the server's declared capabilities and scope.

---

## Example MCP Server Integrations

### Filesystem access

```json
{
  "name": "filesystem",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/docs"]
}
```

Gives the agent tools to list, read, write, and search files under the specified path.

### GitHub

```json
{
  "name": "github",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-github"],
  "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "<from-secret-vault>" }
}
```

Provides tools for searching code, creating issues, reading pull requests, and more.

### PostgreSQL

```json
{
  "name": "postgres",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/mydb"]
}
```

Gives the agent read access to query your database schema and data.

### Custom MCP server

Any process that speaks MCP JSON-RPC over stdio can be registered. See the [MCP specification](https://spec.modelcontextprotocol.io) for the protocol details.

---

## Listing and Removing MCP Servers

Ask the agent to list configured MCP servers:

> "What MCP servers do I have configured?"

To remove one:

> "Remove the filesystem MCP server."

The agent calls the plugin management API to deregister the server. Active connections are gracefully terminated.
