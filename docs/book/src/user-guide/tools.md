# Built-in Agent Tools

Sõber ships with 17 built-in tools grouped into two categories:

- **Static tools** — registered once at startup, available in every conversation.
- **Per-conversation tools** — instantiated fresh for each conversation, carrying workspace and user context.

Tools are exposed to the LLM in OpenAI function-calling format. The LLM decides when to call them; you interact with the results through the chat interface.

---

## Static Tools

### Web & Search

#### `web_search`

Search the web via a configured SearXNG instance and return a ranked list of results.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | The search query. |
| `max_results` | integer | no | Maximum results to return (default: 5). |

Returns a numbered list: title, URL, and a content snippet for each result.

**Example**

```json
{ "query": "Rust async trait patterns 2025", "max_results": 3 }
```

---

#### `fetch_url`

Fetch and extract the text content of a URL. HTML is stripped of scripts and styles; block-level elements are converted to newlines. Output is truncated to 8 000 characters.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | yes | Must start with `http://` or `https://`. |

Supported content types: all `text/*` types, `application/json`, `application/xml`, `application/xhtml+xml`, `application/javascript`, `application/yaml`, `application/toml`, `application/csv`, `application/ld+json`, `application/rss+xml`, `application/atom+xml`. Binary content (images, PDFs, video) is rejected. Maximum response body: 10 MB. Request timeout: 10 seconds.

**Example**

```json
{ "url": "https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html" }
```

---

### Memory

#### `recall`

Search long-term memory using a semantic query. The agent calls this proactively: at conversation start, when the user references the past, or before saying "I don't know". Passive context loading includes only `preference` chunks; all other memory types require an explicit `recall` call.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | Semantic search query. |
| `chunk_type` | string | no | Filter by type: `fact`, `conversation`, `preference`, `skill`, `code`, `soul`. |
| `scope` | string | no | `user` (default) or `system` for global knowledge. |
| `limit` | integer | no | Max results (default: 10, max: 20). |

Results include chunk type, importance score, similarity score, creation date, and content. Retrieval automatically boosts the importance score of returned memories.

**Example**

```json
{ "query": "user's preferred programming language", "chunk_type": "preference", "limit": 5 }
```

---

#### `remember`

Store a piece of information in long-term memory with a chunk type and importance score. Use when the user shares personal facts or preferences, after extracting key outcomes from a conversation, or when explicitly asked to remember something.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `content` | string | yes | The information to store. |
| `chunk_type` | string | yes | `fact`, `preference`, `skill`, or `code`. |
| `importance` | number | no | Score 0.0–1.0. Defaults: `preference`=0.8, `fact`/`skill`=0.7, `code`=0.6, `conversation`=0.5. |

**Example**

```json
{ "content": "User prefers dark mode and compact UI density", "chunk_type": "preference", "importance": 0.85 }
```

---

### Scheduling

#### `scheduler`

Manage scheduled jobs via the scheduler gRPC service. A single tool dispatched by an `action` field.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `action` | string | yes | One of: `list`, `get`, `create`, `cancel`, `pause`, `resume`, `runs`. |

**`list`** — List active, paused, and running jobs. Optional filters: `owner_type`, `status` / `statuses[]`, `name_filter`, `workspace_id`.

**`get`** — Show details for a job. Requires `job_id`.

**`create`** — Create a new scheduled job. Requires `name`, `schedule`, and `payload_type` plus payload-specific fields.

Schedule formats:

| Format | Example |
|--------|---------|
| Interval | `every: 30m` |
| Cron | `0 9 * * MON-FRI` |

Payload types:

| `payload_type` | Required fields | Description |
|---------------|----------------|-------------|
| `prompt` | `text` | Send a prompt through the agent LLM pipeline. |
| `internal` | `operation` | Run a built-in operation: `MemoryPruning`, `SessionCleanup`, `VectorIndexOptimize`, `PluginAudit`. |
| `artifact` | `blob_ref`, `workspace_id` | Execute a blob artifact in the sandbox. |

**`cancel`** — Cancel a job. Requires `job_id`.

**`pause`** — Pause a job. Requires `job_id`.

**`resume`** — Resume a paused job. Requires `job_id`.

**`runs`** — List recent runs for a job. Requires `job_id`. Optional: `limit`.

---

## Per-Conversation Tools

These tools are instantiated for each conversation and carry workspace, user, and conversation context.

### Shell

#### `shell`

Execute a shell command in the user's workspace directory, sandboxed via `bwrap`. Supports pipes, redirects, and standard shell constructs (`sh -c`). Output is truncated to 16 000 characters.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | yes | Shell command to execute. |
| `workdir` | string | no | Working directory relative to workspace root. |
| `timeout` | integer | no | Timeout in seconds (default: 300). |

**Risk classification and permission modes**

Every command is classified as `Safe`, `Moderate`, or `Dangerous` before execution.

| Mode | Behavior |
|------|---------|
| `Autonomous` | No confirmation required. |
| `PolicyBased` | Confirmation required for `Dangerous` commands. |
| `Interactive` | Confirmation required for all commands. |

Commands on the admin deny list are blocked regardless of mode. When `auto_snapshot` is enabled, a workspace snapshot is created automatically before any `Dangerous` command.

**Example**

```json
{ "command": "cargo test -q", "workdir": "my-project", "timeout": 120 }
```

---

### Secrets

Secrets are encrypted with AES-256-GCM. Each user has a Data Encryption Key (DEK) wrapped by the system Master Encryption Key (MEK). All operations are audit-logged.

Secrets have a **scope**:
- `conversation` (default) — accessible only within the current conversation.
- `user` — accessible across all conversations.

#### `store_secret`

Encrypt and store a secret.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | yes | Identifier (e.g. `openai-api-key`). |
| `secret_type` | string | yes | `llm_provider`, `mcp_server`, `api_key`, or `oauth_app`. |
| `data` | object | yes | Key-value pairs to encrypt. |
| `scope` | string | no | `conversation` (default) or `user`. |

Non-sensitive fields (`provider`, `server`, `base_url`, `model`, `description`) are stored as plaintext metadata for listing purposes.

---

#### `read_secret`

Decrypt and retrieve a secret by name. This tool is **internal**: the decrypted value is never forwarded over the WebSocket to the browser. It is only available within the agent's reasoning context.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | yes | Name of the secret to retrieve. |

---

#### `list_secrets`

List secret metadata (names and types only, no decrypted values).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `secret_type` | string | no | Filter by type. Omit to list all. |

---

#### `delete_secret`

Permanently remove a secret from the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | yes | Name of the secret to delete. |

---

### Artifacts

Artifacts are versioned, typed records associated with a workspace. Content is stored either inline (in PostgreSQL) or as a content-addressed blob via `BlobStore`.

**Kinds:** `code_change`, `document`, `proposal`, `snapshot`, `trace`

**States:** `draft`, `proposed`, `approved`, `rejected`, `archived`

#### `create_artifact`

Create a new workspace artifact.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `title` | string | yes | Display title. |
| `kind` | string | yes | `code_change`, `document`, `proposal`, `snapshot`, or `trace`. |
| `content` | string | yes | Artifact content. |
| `description` | string | no | Optional description. |
| `storage_type` | string | no | `inline` (default) or `blob`. |

---

#### `list_artifacts`

List artifacts in the current workspace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `kind` | string | no | Filter by kind. |
| `state` | string | no | Filter by state. |

---

#### `read_artifact`

Read the full content of an artifact by ID.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `artifact_id` | string | yes | UUID of the artifact. |

Returns title, kind, state, storage type, description, creation timestamp, and content.

---

#### `delete_artifact`

Archive (soft-delete) an artifact. The artifact moves to `archived` state but remains in the database.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `artifact_id` | string | yes | UUID of the artifact to archive. |

---

### Snapshots

Snapshots are tar archives of the conversation workspace directory, recorded as `Snapshot`-kind artifacts.

#### `create_snapshot`

Create a tar snapshot of the current workspace conversation directory.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `description` | string | no | Human-readable label (used in the artifact title and archive filename, truncated to 64 chars). |

Returns the artifact ID and filesystem path of the created archive.

---

#### `list_snapshots`

List snapshot artifacts for this workspace, including artifact IDs, labels, and creation times.

No parameters.

---

#### `restore_snapshot`

Restore the workspace from a previously created snapshot. Before restoring, a safety snapshot of the current state is automatically created so the workspace can be recovered if the restore is undesired.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `artifact_id` | string | yes | Artifact ID of the snapshot to restore (from `list_snapshots`). |

The operation is audit-logged with workspace and conversation IDs.

---

### Plugins

#### `generate_plugin`

Generate a new plugin via LLM from a natural-language description. Supports two output types:

- **WASM plugin** — the generated binary is stored content-addressed in `BlobStore`, registered in the plugin database, and optionally tracked as a workspace artifact. Goes through the audit pipeline before installation.
- **Markdown skill** — a text-based skill description stored in the workspace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `description` | string | yes | Natural-language description of what the plugin should do. |
| `plugin_kind` | string | no | `wasm` (default) or `skill`. |
| `name` | string | no | Plugin name (auto-derived from description if omitted). |
