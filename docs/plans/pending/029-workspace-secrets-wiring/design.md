# 029 — Workspace & Secrets Wiring + Audit Trail

## Problem

Workspaces, secrets, and audit logging have solid foundations (DB schema, repo
traits, Postgres implementations, crypto primitives) but are not wired into the
agent execution loop. The agent cannot use workspace context for file operations,
cannot resolve LLM keys from user-stored secrets, and tool calls leave no
persistent audit trail.

### Current State

**Workspaces:** Conversations have an optional `workspace_id` FK, but the agent
ignores it. Shell exec uses no working directory derived from workspace context.
Artifacts have full DB schema but zero API endpoints or agent tools.

**Secrets:** `LlmKeyResolver` exists but the agent uses static `config.llm`
env vars. No agent tools for storing/reading secrets. MCP servers launch
without user-stored credentials.

**Audit:** `SandboxAuditEntry` is constructed in memory but never persisted.
Tool calls stream via WebSocket but aren't stored in messages. The `audit_log`
table has zero write calls.

## Design

### 1. Workspace Wiring

#### Conversation Working Directory

Every conversation with a `workspace_id` gets a unique directory under the
workspace root, named by its conversation UUID:

```
{workspace.root_path}/{conversation_id}/
```

UUID directories are intentional: they guarantee uniqueness, avoid name
collisions, and match the DB identifier for easy correlation. Human
navigation is handled by the UI and CLI, not by browsing the filesystem.

When the agent receives a message for a workspace-bound conversation:

1. Resolve `Workspace` from DB via `workspace_id`
2. Compute working directory: `{workspace.root_path}/{conversation_id}/`
3. Create directory if it doesn't exist (`tokio::fs::create_dir_all`)
4. Pass as `cwd` to shell exec tool
5. File operations outside workspace root trigger confirmation flow (existing)

Conversations without a workspace operate as today (no working directory).

**Directory lifecycle:** Conversation directories are not automatically
cleaned up when conversations are deleted. Cleanup is deferred to the
workspace garbage collection job (future plan).

#### Scheduler Integration

The scheduler also uses workspace context:

- **Prompt jobs** → dispatched to agent via gRPC with `conversation_id`. Agent
  resolves workspace from conversation as above.
- **Artifact jobs** → scheduler resolves workspace directly, executes within
  `{workspace.root_path}/`.
- **Internal jobs** (cleanup, pruning) → operate on workspace paths via
  `sober-workspace` lib.

The scheduler does not use conversation-scoped subdirectories for
non-conversation work.

#### Artifact Agent Tools

Four tools registered in the agent alongside shell_exec, recall, remember.
Artifact tools are only available in workspace-bound conversations. If no
workspace is set, the tools return an error indicating workspace context is
required.

**`create_artifact`**
- Input: title, description, kind (code_change | document | proposal |
  snapshot | trace), storage_type (git | blob | inline), content
- Auto-sets `workspace_id`, `conversation_id`, `created_by` from context
- For `blob` storage: writes content to blob store, records `blob_key`
- For `inline` storage: stores content directly in DB
- For `git` storage: records `git_repo` and `git_ref`
- Returns artifact ID

**`list_artifacts`**
- Input: optional kind filter, optional state filter
- Scoped to current workspace
- Returns artifact metadata (id, title, kind, state, created_at)

**`read_artifact`**
- Input: artifact ID
- Returns artifact content (resolves from blob store, git, or inline)
- For blob: reads from `{data_root}/blobs/{prefix}/{sha256}`

**`delete_artifact`**
- Input: artifact ID
- Soft-deletes: sets state to `archived`
- Logged to audit trail

#### SOUL.md Artifact Guidance

Add to the "Workspace Discipline" section in `backend/soul/SOUL.md`:

```markdown
## Artifact Discipline

- Use `create_artifact` for all meaningful outputs: code changes, documents,
  proposals, analysis results. Do not leave important work as unnamed files.
- Choose the right artifact kind:
  - `code_change` — diffs, patches, new code files
  - `document` — reports, summaries, documentation
  - `proposal` — suggested changes for user review
  - `snapshot` — workspace state captures before destructive operations
  - `trace` — execution logs and debugging output (internal, not shown to users)
- Set artifact state correctly:
  - `draft` — work in progress, not ready for review
  - `proposed` — ready for user review
  - `approved` / `rejected` — after user decision
- When building on previous work, set `parent_id` to link artifacts.
- Use `list_artifacts` to check existing work before creating duplicates.
```

#### sober-workspace Helper

Add a standalone function to `sober-workspace`:

```rust
/// Returns the conversation-specific directory under a workspace root.
/// Creates the directory if it does not exist.
pub async fn ensure_conversation_dir(
    workspace_root: &Path,
    conversation_id: ConversationId,
) -> Result<PathBuf, WorkspaceError>;
```

This lives in a new `sober-workspace::layout` module alongside the existing
`init_workspace_dir()` function.

### 2. Secrets Wiring

#### Table Rename and Schema Update

Rename `user_secrets` → `secrets`. Add `conversation_id` (nullable FK) for
conversation-scoped secrets.

Migration:
```sql
ALTER TABLE user_secrets RENAME TO secrets;
ALTER TABLE secrets ADD COLUMN conversation_id UUID REFERENCES conversations(id);
DROP INDEX idx_user_secrets_name;
CREATE UNIQUE INDEX idx_secrets_name
  ON secrets (user_id, name) WHERE conversation_id IS NULL;
CREATE UNIQUE INDEX idx_secrets_conversation_name
  ON secrets (conversation_id, name) WHERE conversation_id IS NOT NULL;
CREATE INDEX idx_secrets_conversation ON secrets (conversation_id)
  WHERE conversation_id IS NOT NULL;
ALTER INDEX idx_user_secrets_user RENAME TO idx_secrets_user;
ALTER INDEX idx_user_secrets_type RENAME TO idx_secrets_type;
```

**Ripple effect:** All SQL queries in `PgSecretRepo` reference `user_secrets`
and use `UserSecretRow` row types. This migration requires:
- Update all queries from `user_secrets` to `secrets`
- Rename `UserSecretRow` → `SecretDbRow` (internal row type in `sober-db`)
- Regenerate `.sqlx/` prepared statements (`cargo sqlx prepare`)

#### Scoping Model

The `SecretScope` enum is used for storage lookup, **not for encryption key
management**. All secrets — whether conversation-scoped or user-scoped — are
encrypted with the **owning user's DEK**. The `encryption_keys` table remains
keyed by `user_id` with no changes.

Rather than adding a new enum variant, `conversation_id` is an optional filter
parameter on repo methods:

```rust
pub trait SecretRepo: Send + Sync {
    // --- DEK management (always per-user, no conversation scoping) ---
    async fn get_dek(&self, user_id: UserId) -> Result<Option<StoredDek>, AppError>;
    async fn store_dek(
        &self, user_id: UserId, encrypted_dek: Vec<u8>, mek_version: i32,
    ) -> Result<(), AppError>;

    // --- Secret CRUD ---
    /// List secrets. If conversation_id is Some, returns conversation-scoped
    /// secrets first, then user-scoped. If None, returns only user-scoped.
    async fn list_secrets(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
        secret_type: Option<&str>,
    ) -> Result<Vec<SecretMetadata>, AppError>;

    /// Get a secret by name. Checks conversation scope first, then user scope.
    async fn get_secret_by_name(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
        name: &str,
    ) -> Result<Option<SecretRow>, AppError>;

    async fn get_secret(&self, id: SecretId) -> Result<Option<SecretRow>, AppError>;
    async fn store_secret(&self, secret: NewSecret) -> Result<SecretId, AppError>;
    async fn update_secret(&self, id: SecretId, update: UpdateSecret) -> Result<(), AppError>;
    async fn delete_secret(&self, id: SecretId) -> Result<(), AppError>;

    /// List all secret IDs for a user (used for bulk operations like rotation).
    async fn list_secret_ids(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
    ) -> Result<Vec<SecretId>, AppError>;
}
```

**Domain type updates:**
- Replace `scope: SecretScope` in `NewSecret` with `user_id: UserId` +
  `conversation_id: Option<ConversationId>`.
- Add `conversation_id: Option<ConversationId>` to `SecretRow` and
  `SecretMetadata`.
- Remove `SecretScope` enum entirely — replaced by the explicit `user_id` +
  `conversation_id` parameter pattern across all repo methods.

**Name override semantics:** A conversation-scoped secret can have the same
name as a user-scoped secret. This is intentional — conversation secrets
override user secrets. `get_secret_by_name` returns the conversation-scoped
match first.

#### LLM Key Resolution (Code-Level)

The `LlmKeyResolver` resolves credentials automatically before every LLM call.
No agent prompt involvement.

Resolution chain:
1. Conversation secrets (`secret_type = 'llm_provider'`, conversation-scoped)
2. User secrets (`secret_type = 'llm_provider'`, user-scoped)
3. System config (`LLM_API_KEY`, `LLM_BASE_URL` env vars)

Updated resolver:
```rust
impl<S: SecretRepo> LlmKeyResolver<S> {
    pub async fn resolve(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
    ) -> Result<ResolvedLlmKey, AppError> {
        // 1. Query secrets with conversation_id (gets conversation-scoped first)
        let secrets = self.secret_repo.list_secrets(
            user_id, conversation_id, Some("llm_provider")
        ).await?;

        // 2. Try each secret in order (conversation-scoped first, then user)
        for meta in &secrets {
            let row = self.secret_repo.get_secret(meta.id).await?;
            if let Some(row) = row {
                let dek = self.unwrap_dek(user_id).await?;
                let key = dek.decrypt(&row.encrypted_data)?;
                let parsed: LlmProviderSecret = serde_json::from_slice(&key)?;
                return Ok(ResolvedLlmKey { /* ... */ });
            }
        }

        // 3. Fall back to system config
        self.resolve_from_system_config()
    }
}
```

Injected into agent gRPC service at startup alongside existing `Mek`.

#### MCP Credential Injection (Code-Level)

When launching an MCP server:

1. Query secrets with `secret_type = "mcp_server"` and matching
   `metadata.server` name
2. Resolution: conversation secrets → user secrets
3. Decrypt envelope → extract env var key-value pairs
4. Inject into MCP server process environment on spawn
5. If no matching secret: launch without credentials (may fail)
6. On failure: agent can detect and ask user to store credentials

#### Agent Secret Tools

Four tools for managing secrets in conversation context:

**`store_secret`**
- Input: name, secret_type (`llm_provider` | `mcp_server` | `api_key` | custom),
  key-value pairs (the sensitive data), optional scope (`conversation` default |
  `user`)
- Encrypts key-value pairs as JSON envelope with user's DEK
- Creates DEK if none exists for the user
- Stores with conversation or user scope
- Agent confirms storage without echoing values

**`read_secret`**
- Input: name or secret_type
- Decrypts internally, returns to agent code
- Used for MCP credential injection, shell command auth, etc.
- **Security invariant:** plaintext values never appear in conversation events
  (TextDelta, ToolCallResult sent to client)
- Tool result is marked as `internal_only` in the event stream

**`list_secrets`**
- Input: optional secret_type filter
- Returns names + types + non-sensitive metadata (provider, server name)
- No encrypted values exposed
- Agent can report: "You have an OpenAI key and a GitHub token configured"

**`delete_secret`**
- Input: name
- Deletes the secret from DB
- Logged to audit trail

#### Secret Security in Events

When the agent calls `read_secret`, the `ToolCallResult` event must not expose
plaintext secret values to WebSocket clients.

Add `internal: bool` field to the shared `ToolCallResult` proto message
(used by both `AgentEvent` and `ConversationUpdate`).

**Filtering happens in two places:**

1. **Agent side (broadcast):** The agent broadcasts all events (including
   internal) to its internal `tokio::sync::broadcast` channel. This ensures
   the full event stream is available for DB persistence.
2. **API side (WebSocket relay):** The relay task in `sober-api/src/subscribe.rs`
   (`conversation_update_to_ws`) checks `internal` on `ToolCallResult` events
   and skips forwarding them to WebSocket clients.

This means:
- The `messages` table stores the full (unredacted) tool result — DB is
  trusted server-side storage.
- The agent's event broadcast is unfiltered — consumers decide what to forward.
- Only the API-to-WebSocket relay filters `internal` events.

### 3. Audit Trail Wiring

#### Tool Call Persistence

Store all tool calls and results in the `messages` table. The columns
`tool_calls` (JSONB) and `tool_result` (JSONB) already exist.

For each LLM response containing tool calls:
1. Store the assistant message with `tool_calls` populated
2. After tool execution, store a tool-role message with `tool_result`
3. Both messages get `conversation_id` and proper ordering

This gives a complete, queryable history of what the agent did.

#### Sandbox Audit Persistence

After every `BwrapSandbox::execute()` call:
1. Convert `SandboxAuditEntry` to `CreateAuditLog`
2. Write to `audit_log` table with:
   - `action = "shell_exec"`
   - `actor_id = user_id`
   - `target_type = "workspace"` / `target_id = workspace_id`
   - `details` = full `SandboxAuditEntry` as JSON (command, exit code,
     duration, outcome, denied network requests)

#### Secret Access Audit

Log every secret operation to `audit_log`:
- `action = "secret_read"` / `"secret_store"` / `"secret_delete"`
- `actor_id = user_id`
- `target_type = "secret"` / `target_id = secret_id`
- `details` = `{ name, secret_type, scope, conversation_id }` (no plaintext)

#### Confirmation Decision Audit

When a user approves or denies a confirmation request:
- `action = "confirm_approve"` / `"confirm_deny"`
- `details` = `{ command, risk_level, conversation_id }`

## Agent Struct Design

The agent gRPC service is currently generic over `<Msg, Conv, Mcp, User>`. This
plan adds dependencies on `SecretRepo`, `AuditLogRepo`, `ArtifactRepo`, and
`WorkspaceRepo`.

Rather than expanding the generic parameter list to 8 types, introduce an
`AgentRepos` trait bundle:

```rust
pub trait AgentRepos: Send + Sync + 'static {
    type Msg: MessageRepo;
    type Conv: ConversationRepo;
    type Mcp: McpServerRepo;
    type User: UserRepo;
    type Secret: SecretRepo;
    type Audit: AuditLogRepo;
    type Artifact: ArtifactRepo;
    type Workspace: WorkspaceRepo;

    fn messages(&self) -> &Self::Msg;
    fn conversations(&self) -> &Self::Conv;
    fn mcp_servers(&self) -> &Self::Mcp;
    fn users(&self) -> &Self::User;
    fn secrets(&self) -> &Self::Secret;
    fn audit_log(&self) -> &Self::Audit;
    fn artifacts(&self) -> &Self::Artifact;
    fn workspaces(&self) -> &Self::Workspace;
}
```

Production implementation bundles all `Pg*Repo` types. Test implementation
uses mocks. The agent struct becomes `AgentGrpcService<R: AgentRepos>` —
one generic parameter regardless of how many repos it needs.

## Changes by Crate

| Crate | Changes |
|-------|---------|
| **sober-core** | Remove `SecretScope` enum. Add `conversation_id: Option<ConversationId>` to `SecretRow`, `SecretMetadata`, `NewSecret`. Update `SecretRepo` trait signatures to use `user_id` + optional `conversation_id`. Define `AgentRepos` trait bundle. Add `ArtifactFilter` to input types. |
| **sober-db** | Rename table migration (`user_secrets` → `secrets`). Update all SQL queries and rename `UserSecretRow` → `SecretDbRow`. Update `PgSecretRepo` for conversation-scoped queries. Add audit log write calls. Implement `AgentRepos` for Pg types. Regenerate `.sqlx/` prepared statements. |
| **sober-llm** | Update `LlmKeyResolver::resolve()` to accept `conversation_id`. Resolution: conversation → user → system config. |
| **sober-agent** | Refactor to `AgentGrpcService<R: AgentRepos>`. Inject `LlmKeyResolver` + `Mek`. Resolve workspace + conversation working directory on message receipt. Register 8 new tools (4 artifact + 4 secret). Persist tool calls/results to messages table. Write sandbox audit entries after shell exec. Add `internal` flag to `ToolCallResult` events, filter in broadcast relay. |
| **sober-mcp** | Resolve `mcp_server` credentials from secrets before MCP server spawn. |
| **sober-workspace** | Add `layout` module with `ensure_conversation_dir()` function. |
| **sober-mind** (SOUL.md) | Add "Artifact Discipline" section to workspace guidance. |
| **sober-sandbox** | No code changes (audit entry already generated, caller now persists it). |

## Non-Goals

- Workspace or secret CRUD API endpoints (future plan)
- Frontend UI for secret management (future plan)
- Key rotation (DEK/MEK) — infrastructure exists, automation deferred
- Group-scoped secrets — conversation scope covers the group use case
- Blob scoping/cleanup automation — deferred to observability plan (#018)
- Conversation directory cleanup on deletion — deferred to workspace GC job
