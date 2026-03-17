# 029 — Workspace & Secrets Wiring Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire workspaces, secrets, and audit logging into the agent execution loop so the agent uses workspace context for file operations, resolves LLM keys from user-stored secrets, and persists a complete audit trail.

**Architecture:** Three subsystems (workspace, secrets, audit) get connected through the agent's tool registry and execution loop. A new `AgentRepos` trait bundle replaces the growing generic parameter list. LLM key resolution and MCP credential injection happen at the code level; secret management happens via agent tools. All tool calls get persisted to the messages table and audit log.

**Tech Stack:** Rust, tonic/prost (gRPC), sqlx (PostgreSQL), sober-crypto (AES-256-GCM envelope encryption), sober-workspace (filesystem ops), sober-sandbox (bwrap)

**Spec:** `docs/plans/pending/029-workspace-secrets-wiring/design.md`

---

## File Map

### New Files

| File | Responsibility |
|------|----------------|
| `backend/migrations/2026XXXXXXXXXX_rename_secrets_add_conversation.sql` | Rename `user_secrets` to `secrets`, add `conversation_id` column |
| `backend/crates/sober-core/src/types/agent_repos.rs` | `AgentRepos` trait bundle definition |
| `backend/crates/sober-workspace/src/layout.rs` | `ensure_conversation_dir()` function |
| `backend/crates/sober-agent/src/tools/secrets.rs` | `StoreSecretTool`, `ReadSecretTool`, `ListSecretsTool`, `DeleteSecretTool` |
| `backend/crates/sober-agent/src/tools/artifacts.rs` | `CreateArtifactTool`, `ListArtifactsTool`, `ReadArtifactTool`, `DeleteArtifactTool` |
| `backend/crates/sober-agent/src/tools/snapshots.rs` | `CreateSnapshotTool`, `ListSnapshotsTool`, `RestoreSnapshotTool` |
| `backend/crates/sober-agent/src/audit.rs` | Audit log helper functions (sandbox, secret, confirmation) |
| `backend/crates/sober-db/src/repos/agent_repos.rs` | `PgAgentRepos` concrete `AgentRepos` implementation |

### Modified Files

| File | Changes |
|------|---------|
| `backend/crates/sober-core/src/types/domain.rs` | Remove `SecretScope`, add `conversation_id` to `SecretRow`/`SecretMetadata` |
| `backend/crates/sober-core/src/types/input.rs` | Update `NewSecret`: replace `scope` with `user_id` + `conversation_id` |
| `backend/crates/sober-core/src/types/repo.rs` | Update `SecretRepo` trait signatures |
| `backend/crates/sober-core/src/types/mod.rs` | Re-export `AgentRepos` |
| `backend/crates/sober-db/src/repos/secrets.rs` | Update all queries for `secrets` table, add conversation scope |
| `backend/crates/sober-db/src/rows/` | Rename `UserSecretRow` to `SecretDbRow`, add `conversation_id` |
| `backend/crates/sober-db/src/repos/mod.rs` | Export `PgAgentRepos` |
| `backend/crates/sober-db/src/lib.rs` | Re-export `PgAgentRepos` |
| `backend/crates/sober-llm/src/resolver.rs` | Accept `conversation_id` in `resolve()`, update resolution logic |
| `backend/crates/sober-workspace/src/lib.rs` | Export `layout` module |
| `backend/crates/sober-agent/src/agent.rs` | Refactor to `Agent<R: AgentRepos>`, workspace resolution, tool call persistence |
| `backend/crates/sober-agent/src/grpc.rs` | Refactor to `AgentGrpcService<R: AgentRepos>` |
| `backend/crates/sober-agent/src/tools/mod.rs` | Register new tools |
| `backend/crates/sober-agent/src/tools/shell.rs` | Accept dynamic workspace path per-call |
| `backend/crates/sober-agent/src/main.rs` | Wire new repos, resolver, tools |
| `backend/crates/sober-agent/src/stream.rs` | Add `internal` flag to tool call events |
| `backend/crates/sober-core/src/types/tool.rs` | Add `internal: bool` to `ToolMetadata` |
| `backend/crates/sober-mcp/src/pool.rs` | Inject secrets into MCP server env before spawn |
| `backend/crates/sober-api/src/subscribe.rs` | Filter `internal` tool results in WS relay |
| `backend/proto/sober/agent/v1/agent.proto` | Add `bool internal = 3` to `ToolCallResult` |
| `backend/soul/SOUL.md` | Add "Artifact Discipline" section |

---

## Chunk 1: Foundation — Types, Schema, Proto

### Task 1: Database Migration

**Files:**
- Create: `backend/migrations/2026XXXXXXXXXX_rename_secrets_add_conversation.sql`

- [ ] **Step 1: Create migration file**

```bash
cd backend && sqlx migrate add rename_secrets_add_conversation
```

- [ ] **Step 2: Write migration SQL**

```sql
-- Rename table
ALTER TABLE user_secrets RENAME TO secrets;

-- Add conversation scope
ALTER TABLE secrets ADD COLUMN conversation_id UUID REFERENCES conversations(id);

-- Rebuild unique constraints for dual-scope naming
DROP INDEX IF EXISTS idx_user_secrets_name;
CREATE UNIQUE INDEX idx_secrets_name
  ON secrets (user_id, name) WHERE conversation_id IS NULL;
CREATE UNIQUE INDEX idx_secrets_conversation_name
  ON secrets (conversation_id, name) WHERE conversation_id IS NOT NULL;
CREATE INDEX idx_secrets_conversation ON secrets (conversation_id)
  WHERE conversation_id IS NOT NULL;

-- Rename leftover indexes for consistency
ALTER INDEX IF EXISTS idx_user_secrets_user RENAME TO idx_secrets_user;
ALTER INDEX IF EXISTS idx_user_secrets_type RENAME TO idx_secrets_type;
```

- [ ] **Step 3: Verify migration compiles**

Run: `cd backend && cargo build -q -p sober-db 2>&1 | head -20`
Expected: May show sqlx errors (queries still reference old table). That is OK — queries are fixed in Task 6.

- [ ] **Step 4: Commit**

```
feat(db): rename user_secrets to secrets, add conversation scope
```

---

### Task 2: Core Domain Type Updates

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs` (lines ~414-480)
- Modify: `backend/crates/sober-core/src/types/input.rs` (lines ~184-236)
- Modify: `backend/crates/sober-core/src/types/mod.rs`

- [ ] **Step 1: Remove `SecretScope` enum from `domain.rs`**

Delete the `SecretScope` enum (lines 414-421). Remove any `use` or re-export of it.

- [ ] **Step 2: Add `conversation_id` to secret domain types**

In `SecretMetadata` (lines ~423-440), add:
```rust
pub conversation_id: Option<ConversationId>,
```

In `SecretRow` (lines ~442-463), add:
```rust
pub conversation_id: Option<ConversationId>,
```

- [ ] **Step 3: Update `NewSecret` in `input.rs`**

Replace `scope: SecretScope` with:
```rust
pub user_id: UserId,
pub conversation_id: Option<ConversationId>,
```

- [ ] **Step 4: Remove `SecretScope` from `mod.rs` re-exports**

Grep for `SecretScope` in `types/mod.rs` and remove it.

- [ ] **Step 5: Fix compilation**

Run: `cd backend && cargo build -q -p sober-core 2>&1 | head -30`
Expected: Errors in downstream crates (`sober-db`, `sober-llm`) referencing removed `SecretScope`. Expected — fixed in later tasks.

- [ ] **Step 6: Commit**

```
refactor(core): remove SecretScope, add conversation_id to secret types
```

---

### Task 3: Update `SecretRepo` Trait

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs` (lines ~629-692)

- [ ] **Step 1: Rewrite `SecretRepo` trait**

Replace the existing trait (which uses `SecretScope` parameters) with the new signatures. All `scope: SecretScope` parameters become `user_id: UserId` plus optionally `conversation_id: Option<ConversationId>`.

Key method changes:
- `get_dek(scope: SecretScope)` becomes `get_dek(user_id: UserId)`
- `store_dek(scope: SecretScope, ...)` becomes `store_dek(user_id: UserId, ...)`
- `list_secrets(scope, secret_type)` becomes `list_secrets(user_id, conversation_id, secret_type)`
- `get_secret_by_name(scope, name)` becomes `get_secret_by_name(user_id, conversation_id, name)`
- `list_secret_ids(scope)` becomes `list_secret_ids(user_id, conversation_id)`

Use RPITIT pattern (existing codebase convention):
```rust
fn get_dek(&self, user_id: UserId)
    -> impl Future<Output = Result<Option<StoredDek>, AppError>> + Send;
```

- [ ] **Step 2: Verify sober-core compiles**

Run: `cd backend && cargo build -q -p sober-core`
Expected: PASS (trait is only a definition, no implementation here)

- [ ] **Step 3: Commit**

```
refactor(core): update SecretRepo trait for conversation-scoped secrets
```

---

### Task 4: `AgentRepos` Trait Bundle

**Files:**
- Create: `backend/crates/sober-core/src/types/agent_repos.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

- [ ] **Step 1: Create `agent_repos.rs`**

```rust
use super::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, McpServerRepo,
    MessageRepo, SecretRepo, UserRepo, WorkspaceRepo,
};

/// Bundles all repository traits needed by the agent.
///
/// Avoids an unwieldy generic parameter list on `Agent<Msg, Conv, Mcp, ...>`.
/// Production uses `PgAgentRepos`; tests can mock individual repos.
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

- [ ] **Step 2: Export from `types/mod.rs`**

Add `mod agent_repos;` and re-export `AgentRepos`.

- [ ] **Step 3: Verify compilation**

Run: `cd backend && cargo build -q -p sober-core`

- [ ] **Step 4: Commit**

```
feat(core): add AgentRepos trait bundle
```

---

### Task 5: Proto Update

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto` (line ~76)

- [ ] **Step 1: Add `internal` field to `ToolCallResult`**

Current (line 76):
```protobuf
message ToolCallResult { string name = 1; string output = 2; }
```

Change to:
```protobuf
message ToolCallResult {
  string name = 1;
  string output = 2;
  bool internal = 3;
}
```

- [ ] **Step 2: Add `internal` to `ToolMetadata` in core**

In `sober-core/src/types/tool.rs`, add `pub internal: bool` to `ToolMetadata`. Default `false` for all existing tools.

- [ ] **Step 3: Rebuild proto**

Run: `cd backend && cargo build -q -p sober-agent 2>&1 | head -20`

- [ ] **Step 4: Commit**

```
feat(proto): add internal flag to ToolCallResult for secret redaction
```

---

## Chunk 2: Database and LLM Layer

### Task 6: Update `PgSecretRepo`

**Files:**
- Modify: `backend/crates/sober-db/src/repos/secrets.rs` (all queries)
- Modify: `backend/crates/sober-db/src/rows/` (row types for secrets)

- [ ] **Step 1: Rename row type**

In the rows module (find file containing `UserSecretRow`), rename:
- `UserSecretRow` to `SecretDbRow`
- Add `conversation_id: Option<uuid::Uuid>` to `SecretDbRow`

- [ ] **Step 2: Update all SQL queries**

Replace `user_secrets` with `secrets` in every query string (~9 queries).

- [ ] **Step 3: Update `get_dek` and `store_dek`**

Change parameter from `scope: SecretScope` to `user_id: UserId`. Extract `user_id.as_uuid()` directly instead of destructuring scope.

- [ ] **Step 4: Update `list_secrets` for conversation scope**

New query: when `conversation_id` is `Some`, return conversation-scoped secrets first, then user-scoped:

```sql
SELECT id, user_id, name, secret_type, metadata, priority,
       conversation_id, created_at, updated_at
FROM secrets
WHERE user_id = $1
  AND ($2::uuid IS NULL OR conversation_id = $2 OR conversation_id IS NULL)
  AND ($3::text IS NULL OR secret_type = $3)
ORDER BY
  CASE WHEN conversation_id IS NOT NULL THEN 0 ELSE 1 END,
  priority ASC NULLS LAST
```

- [ ] **Step 5: Update `get_secret_by_name` for conversation scope**

Check conversation scope first, fall back to user scope:
```sql
SELECT * FROM secrets
WHERE user_id = $1 AND name = $2
  AND (conversation_id = $3 OR ($3::uuid IS NULL AND conversation_id IS NULL))
ORDER BY CASE WHEN conversation_id IS NOT NULL THEN 0 ELSE 1 END
LIMIT 1
```

- [ ] **Step 6: Update `store_secret`**

Add `conversation_id` to INSERT statement.

- [ ] **Step 7: Update `list_secret_ids`**

Change from `scope: SecretScope` to `user_id: UserId, conversation_id: Option<ConversationId>`.

- [ ] **Step 8: Update `From` conversions**

Update `From<SecretDbRow>` for `SecretRow` and `SecretMetadata` to map `conversation_id`.

- [ ] **Step 9: Verify compilation**

Run: `cd backend && cargo build -q -p sober-db`

- [ ] **Step 10: Commit**

```
refactor(db): update PgSecretRepo for conversation-scoped secrets
```

---

### Task 7: Implement `PgAgentRepos`

**Files:**
- Create: `backend/crates/sober-db/src/repos/agent_repos.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`
- Modify: `backend/crates/sober-db/src/lib.rs`

- [ ] **Step 1: Create `PgAgentRepos` struct**

```rust
use sober_core::types::AgentRepos;
use sqlx::PgPool;
use super::*;

pub struct PgAgentRepos {
    pub messages: PgMessageRepo,
    pub conversations: PgConversationRepo,
    pub mcp_servers: PgMcpServerRepo,
    pub users: PgUserRepo,
    pub secrets: PgSecretRepo,
    pub audit_log: PgAuditLogRepo,
    pub artifacts: PgArtifactRepo,
    pub workspaces: PgWorkspaceRepo,
}

impl PgAgentRepos {
    pub fn new(pool: PgPool) -> Self {
        Self {
            messages: PgMessageRepo::new(pool.clone()),
            conversations: PgConversationRepo::new(pool.clone()),
            mcp_servers: PgMcpServerRepo::new(pool.clone()),
            users: PgUserRepo::new(pool.clone()),
            secrets: PgSecretRepo::new(pool.clone()),
            audit_log: PgAuditLogRepo::new(pool.clone()),
            artifacts: PgArtifactRepo::new(pool.clone()),
            workspaces: PgWorkspaceRepo::new(pool),
        }
    }
}

impl AgentRepos for PgAgentRepos {
    type Msg = PgMessageRepo;
    type Conv = PgConversationRepo;
    type Mcp = PgMcpServerRepo;
    type User = PgUserRepo;
    type Secret = PgSecretRepo;
    type Audit = PgAuditLogRepo;
    type Artifact = PgArtifactRepo;
    type Workspace = PgWorkspaceRepo;

    fn messages(&self) -> &PgMessageRepo { &self.messages }
    fn conversations(&self) -> &PgConversationRepo { &self.conversations }
    fn mcp_servers(&self) -> &PgMcpServerRepo { &self.mcp_servers }
    fn users(&self) -> &PgUserRepo { &self.users }
    fn secrets(&self) -> &PgSecretRepo { &self.secrets }
    fn audit_log(&self) -> &PgAuditLogRepo { &self.audit_log }
    fn artifacts(&self) -> &PgArtifactRepo { &self.artifacts }
    fn workspaces(&self) -> &PgWorkspaceRepo { &self.workspaces }
}
```

- [ ] **Step 2: Export from `repos/mod.rs` and `lib.rs`**

- [ ] **Step 3: Verify compilation**

Run: `cd backend && cargo build -q -p sober-db`

- [ ] **Step 4: Commit**

```
feat(db): add PgAgentRepos bundle for agent dependency injection
```

---

### Task 8: Update `LlmKeyResolver`

**Files:**
- Modify: `backend/crates/sober-llm/src/resolver.rs` (~141 lines)

- [ ] **Step 1: Update `resolve()` signature**

```rust
pub async fn resolve(
    &self,
    user_id: UserId,
    conversation_id: Option<ConversationId>,
) -> Result<ResolvedLlmKey, AppError>
```

- [ ] **Step 2: Replace `try_scope` with conversation-aware resolution**

The existing `try_scope(SecretScope::User(user_id))` becomes:
1. `self.secret_repo.list_secrets(user_id, conversation_id, Some("llm_provider"))` — returns conversation-scoped first, then user-scoped (handled by DB query ordering)
2. For each result: get DEK via `self.secret_repo.get_dek(user_id)`, unwrap with MEK, decrypt
3. Fall back to system config

- [ ] **Step 3: Update tests**

The existing 3 tests use mock `SecretRepo`. Update mock to match new trait signatures (no `SecretScope`).

- [ ] **Step 4: Run tests**

Run: `cd backend && cargo test -p sober-llm -q`
Expected: 3 tests pass

- [ ] **Step 5: Commit**

```
feat(llm): update LlmKeyResolver for conversation-scoped secrets
```

---

## Chunk 3: Workspace Helpers

### Task 9: Workspace Layout Module

**Files:**
- Create: `backend/crates/sober-workspace/src/layout.rs`
- Modify: `backend/crates/sober-workspace/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::ConversationId;
    use tempfile::TempDir;

    #[tokio::test]
    async fn creates_conversation_dir() {
        let tmp = TempDir::new().unwrap();
        let conv_id = ConversationId::new();
        let path = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        assert!(path.exists());
        assert_eq!(path, tmp.path().join(conv_id.to_string()));
    }

    #[tokio::test]
    async fn idempotent_on_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let conv_id = ConversationId::new();
        let p1 = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        let p2 = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        assert_eq!(p1, p2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p sober-workspace -q -- layout`
Expected: FAIL (function does not exist)

- [ ] **Step 3: Implement `ensure_conversation_dir`**

```rust
use std::path::{Path, PathBuf};
use sober_core::types::ConversationId;
use crate::error::WorkspaceError;

/// Returns the conversation-specific directory under a workspace root.
/// Creates the directory if it does not exist.
pub async fn ensure_conversation_dir(
    workspace_root: &Path,
    conversation_id: ConversationId,
) -> Result<PathBuf, WorkspaceError> {
    let dir = workspace_root.join(conversation_id.to_string());
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        WorkspaceError::Io(format!(
            "failed to create conversation dir {}: {e}",
            dir.display()
        ))
    })?;
    Ok(dir)
}
```

- [ ] **Step 4: Export from `lib.rs`**

Add `pub mod layout;` to `sober-workspace/src/lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test -p sober-workspace -q -- layout`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```
feat(workspace): add ensure_conversation_dir layout helper
```

---

## Chunk 4: Agent Refactor

### Task 10: Refactor Agent to `AgentRepos`

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs` (lines ~104-171)
- Modify: `backend/crates/sober-agent/src/grpc.rs` (lines ~32-65)
- Modify: `backend/crates/sober-agent/src/main.rs`

This is the largest single task.

- [ ] **Step 1: Update `Agent` struct generics**

In `agent.rs`, change:
```rust
pub struct Agent<Msg, Conv, Mcp, User>
where Msg: MessageRepo, Conv: ConversationRepo, ...
```
to:
```rust
pub struct Agent<R: AgentRepos>
```

Replace individual repo fields with a single `repos: Arc<R>`.

- [ ] **Step 2: Add new fields to `Agent`**

```rust
pub struct Agent<R: AgentRepos> {
    llm: Arc<dyn LlmEngine>,
    mind: Arc<Mind>,
    memory: Arc<MemoryStore>,
    context_loader: Arc<ContextLoader<R::Msg>>,
    tool_registry: Arc<ToolRegistry>,
    repos: Arc<R>,
    config: AgentConfig,
    memory_config: MemoryConfig,
    registrar: Option<ConfirmationRegistrar>,
    broadcast_tx: ConversationUpdateSender,
    resolver: Option<Arc<LlmKeyResolver<R::Secret>>>,
    mek: Option<Arc<Mek>>,
}
```

- [ ] **Step 3: Update constructor to accept `repos: Arc<R>`**

Remove individual repo parameters, add `repos`, `resolver`, `mek`.

- [ ] **Step 4: Update all field accesses in `agent.rs`**

Mechanical find-and-replace across the file:
- `self.message_repo` becomes `self.repos.messages()`
- `self.conversation_repo` becomes `self.repos.conversations()`
- `self.mcp_server_repo` becomes `self.repos.mcp_servers()`
- `self.user_repo` becomes `self.repos.users()`

- [ ] **Step 5: Update `AgentGrpcService` in `grpc.rs`**

```rust
pub struct AgentGrpcService<R: AgentRepos> {
    agent: Arc<Agent<R>>,
    confirmation_sender: ConfirmationSender,
    permission_mode: SharedPermissionMode,
    broadcast_tx: ConversationUpdateSender,
}
```

Update all method signatures similarly.

- [ ] **Step 6: Update `main.rs`**

Create `PgAgentRepos::new(pool.clone())` and pass `Arc::new(repos)` to the agent constructor. Remove individual repo creation lines.

Wire the `LlmKeyResolver`:
```rust
let (resolver, mek_arc) = match config.crypto.master_encryption_key.as_ref() {
    Some(hex_key) => {
        let mek = Mek::from_hex(hex_key).expect("invalid MASTER_ENCRYPTION_KEY");
        let resolver = LlmKeyResolver::new(
            repos.secrets.clone(),
            mek.clone(),
            config.llm.clone(),
        );
        (Some(Arc::new(resolver)), Some(Arc::new(mek)))
    }
    None => (None, None),
};
```

- [ ] **Step 7: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent`
Expected: PASS

- [ ] **Step 8: Commit**

```
refactor(agent): replace individual repo generics with AgentRepos bundle
```

---

### Task 11: Workspace Context in Agent Loop

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs` (agentic loop, ~line 350+)
- Modify: `backend/crates/sober-agent/src/tools/shell.rs`

- [ ] **Step 1: Resolve workspace on message receipt**

In the agentic loop (around line 350-400 in `agent.rs`), after loading the conversation, add workspace resolution:

```rust
let workspace_dir = if let Some(ws_id) = conversation.workspace_id {
    match self.repos.workspaces().get_by_id(ws_id).await? {
        Some(ws) => {
            let dir = sober_workspace::layout::ensure_conversation_dir(
                Path::new(&ws.root_path),
                conversation_id,
            ).await?;
            Some(dir)
        }
        None => None,
    }
} else {
    None
};
```

- [ ] **Step 2: Make ShellTool accept dynamic workspace path**

Currently `ShellTool` has a static `workspace_home: PathBuf` set at construction. Add a mechanism for the agent loop to override the working directory per-call.

In the agent's tool dispatch (line ~803), inject the resolved workspace_dir into the tool input before calling `tool.execute()`:

```rust
if tool_name == "shell" {
    if let Some(ref dir) = workspace_dir {
        tool_input["workdir_override"] = serde_json::Value::String(
            dir.to_string_lossy().to_string()
        );
    }
}
```

In `ShellTool::execute()`, check for `workdir_override` first:
```rust
let workdir = if let Some(ov) = input.get("workdir_override").and_then(|v| v.as_str()) {
    PathBuf::from(ov)
} else if let Some(ref wd) = parsed_input.workdir {
    self.workspace_home.join(wd)
} else {
    self.workspace_home.clone()
};
```

- [ ] **Step 3: Verify compilation and existing tests**

Run: `cd backend && cargo build -q -p sober-agent && cargo test -p sober-agent -q`

- [ ] **Step 4: Commit**

```
feat(agent): resolve workspace context and set shell cwd per-conversation
```

---

## Chunk 5: Agent Tools — Secrets

### Task 12: Secret Tools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/secrets.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`

All four secret tools share context: `SecretRepo`, `AuditLogRepo`, `Mek`, user/conversation IDs.

- [ ] **Step 1: Define shared context and tool structs**

```rust
use std::sync::Arc;
use sober_core::types::{
    SecretRepo, AuditLogRepo, UserId, ConversationId,
    Tool, ToolMetadata, BoxToolFuture,
};
use sober_crypto::envelope::{Mek, Dek};

/// Shared context for all secret tools.
pub struct SecretToolContext<S: SecretRepo, A: AuditLogRepo> {
    pub secret_repo: Arc<S>,
    pub audit_repo: Arc<A>,
    pub mek: Arc<Mek>,
    pub user_id: UserId,
    pub conversation_id: Option<ConversationId>,
}

pub struct StoreSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}
pub struct ReadSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}
pub struct ListSecretsTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}
pub struct DeleteSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}
```

- [ ] **Step 2: Implement `Tool` for `StoreSecretTool`**

Input schema:
```json
{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "secret_type": { "type": "string" },
    "data": { "type": "object", "description": "Key-value pairs to encrypt" },
    "scope": { "type": "string", "enum": ["conversation", "user"], "default": "conversation" }
  },
  "required": ["name", "secret_type", "data"]
}
```

Execute logic:
1. Get or create DEK for `user_id`
2. Serialize `data` to JSON bytes, encrypt with DEK
3. Build `NewSecret` with user_id, conversation_id (based on scope), metadata (non-sensitive fields like `provider`)
4. Call `secret_repo.store_secret()`
5. Write audit log entry (`action = "secret_store"`)
6. Return `"Secret '{name}' stored successfully."` (no values echoed)

- [ ] **Step 3: Implement `Tool` for `ReadSecretTool`**

Execute logic:
1. `get_secret_by_name(user_id, conversation_id, name)`
2. Get DEK, unwrap with MEK, decrypt encrypted_data
3. Write audit log (`action = "secret_read"`)
4. Return decrypted JSON to agent code

Set `ToolMetadata { internal: true, .. }` so results are not forwarded to WebSocket.

- [ ] **Step 4: Implement `ListSecretsTool` and `DeleteSecretTool`**

`ListSecretsTool`: returns names + types + non-sensitive metadata.
`DeleteSecretTool`: deletes by name, writes audit log.

- [ ] **Step 5: Register in `tools/mod.rs`**

Add `pub mod secrets;` and export tool types.

- [ ] **Step 6: Write unit test for store + read round-trip**

Use mock `SecretRepo` and `AuditLogRepo`:
1. Store a secret with `StoreSecretTool`
2. Read it with `ReadSecretTool`
3. Verify decrypted content matches original
4. Verify audit entries were created

- [ ] **Step 7: Run tests**

Run: `cd backend && cargo test -p sober-agent -q -- secrets`

- [ ] **Step 8: Commit**

```
feat(agent): add secret management tools (store, read, list, delete)
```

---

## Chunk 6: Agent Tools — Artifacts and Snapshots

### Task 13: Artifact Tools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/artifacts.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`

- [ ] **Step 1: Define shared context and tool structs**

```rust
pub struct ArtifactToolContext<R: AgentRepos> {
    pub repos: Arc<R>,
    pub blob_store: Arc<BlobStore>,
    pub user_id: UserId,
    pub conversation_id: ConversationId,
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
}
```

Four tools: `CreateArtifactTool`, `ListArtifactsTool`, `ReadArtifactTool`, `DeleteArtifactTool`.

- [ ] **Step 2: Implement `CreateArtifactTool`**

Input: title, description, kind, storage_type, content.

Execute:
1. Validate workspace context exists
2. Based on `storage_type`:
   - `inline`: set `inline_content`
   - `blob`: `blob_store.store(content.as_bytes())`, set `blob_key`
   - `git`: set `git_repo` and `git_ref`
3. Call `repos.artifacts().create(CreateArtifact { ... })`
4. Return artifact ID

- [ ] **Step 3: Implement `ListArtifactsTool`**

Calls `repos.artifacts().list_by_workspace(workspace_id, filter)`.

- [ ] **Step 4: Implement `ReadArtifactTool`**

Calls `repos.artifacts().get_by_id(id)`, resolves content by `storage_type`.

- [ ] **Step 5: Implement `DeleteArtifactTool`**

Calls `repos.artifacts().update_state(id, ArtifactState::Archived)`.

- [ ] **Step 6: Register in `tools/mod.rs`**

- [ ] **Step 7: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 8: Commit**

```
feat(agent): add artifact management tools (create, list, read, delete)
```

---

### Task 14: Snapshot Tools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/snapshots.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`

Uses existing `SnapshotManager` from `sober-workspace`.

- [ ] **Step 1: Define snapshot tool structs**

```rust
pub struct SnapshotToolContext<R: AgentRepos> {
    pub repos: Arc<R>,
    pub snapshot_manager: Arc<SnapshotManager>,
    pub conversation_id: ConversationId,
    pub workspace_id: WorkspaceId,
    pub conversation_dir: PathBuf,
}
```

Three tools: `CreateSnapshotTool`, `ListSnapshotsTool`, `RestoreSnapshotTool`.

- [ ] **Step 2: Implement `CreateSnapshotTool`**

1. Call `snapshot_manager.create(conversation_dir, label)`
2. Create artifact record (kind = `snapshot`, storage_type = `blob`)
3. Return artifact ID + path

- [ ] **Step 3: Implement `ListSnapshotsTool`**

Query artifacts with `kind = snapshot` for this workspace. Return formatted list.

- [ ] **Step 4: Implement `RestoreSnapshotTool`**

1. Fetch artifact by ID, verify it is a snapshot
2. Create pre-restore snapshot (safety net)
3. Call `snapshot_manager.restore(snapshot_path, conversation_dir)`
4. Write audit log
5. Return confirmation message

- [ ] **Step 5: Register in `tools/mod.rs`**

- [ ] **Step 6: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 7: Commit**

```
feat(agent): add snapshot tools (create, list, restore)
```

---

## Chunk 7: Integration — Audit, Events, MCP

### Task 15: Tool Call Persistence

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs` (lines ~596-608, ~803-843)

- [ ] **Step 1: Store assistant messages with `tool_calls` populated**

In the agent loop, when the LLM response contains tool calls (line ~744), store the assistant message with the `tool_calls` JSONB:

```rust
let tool_calls_json = serde_json::to_value(&choice.message.tool_calls)?;
repos.messages().create(CreateMessage {
    conversation_id,
    role: MessageRole::Assistant,
    content: String::new(),
    tool_calls: Some(tool_calls_json),
    tool_result: None,
    token_count: usage_stats.map(|u| u.total_tokens as i32),
    metadata: None,
    user_id: None,
}).await?;
```

- [ ] **Step 2: Store tool results as separate messages**

After each tool execution (line ~843), store the result:

```rust
repos.messages().create(CreateMessage {
    conversation_id,
    role: MessageRole::Tool,
    content: output.clone(),
    tool_calls: None,
    tool_result: Some(serde_json::json!({
        "tool_call_id": tool_call_id,
        "name": tool_name,
    })),
    token_count: None,
    metadata: None,
    user_id: None,
}).await?;
```

Check if `MessageRole::Tool` exists. If not, add it to the enum and migration.

- [ ] **Step 3: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 4: Commit**

```
feat(agent): persist tool calls and results to messages table
```

---

### Task 16: Audit Trail Wiring

**Files:**
- Create: `backend/crates/sober-agent/src/audit.rs`
- Modify: `backend/crates/sober-agent/src/tools/shell.rs`
- Modify: `backend/crates/sober-agent/src/agent.rs`

- [ ] **Step 1: Create audit helper module**

```rust
use sober_core::types::{
    AuditLogRepo, CreateAuditLog, UserId, WorkspaceId,
};
use sober_core::error::AppError;
use sober_sandbox::audit::SandboxAuditEntry;

pub async fn log_shell_exec<A: AuditLogRepo>(
    audit: &A,
    actor_id: UserId,
    workspace_id: Option<WorkspaceId>,
    entry: &SandboxAuditEntry,
) -> Result<(), AppError> {
    audit.create(CreateAuditLog {
        actor_id: Some(actor_id),
        action: "shell_exec".to_string(),
        target_type: Some("workspace".to_string()),
        target_id: workspace_id.map(|w| *w.as_uuid()),
        details: Some(serde_json::to_value(entry).unwrap_or_default()),
        ip_address: None,
    }).await?;
    Ok(())
}

pub async fn log_confirmation<A: AuditLogRepo>(
    audit: &A,
    actor_id: UserId,
    approved: bool,
    details: serde_json::Value,
) -> Result<(), AppError> {
    let action = if approved { "confirm_approve" } else { "confirm_deny" };
    audit.create(CreateAuditLog {
        actor_id: Some(actor_id),
        action: action.to_string(),
        target_type: None,
        target_id: None,
        details: Some(details),
        ip_address: None,
    }).await?;
    Ok(())
}
```

- [ ] **Step 2: Wire sandbox audit into shell tool**

After `BwrapSandbox::execute()` returns in `ShellTool`, call `log_shell_exec()`. Pass audit repo access through tool context.

- [ ] **Step 3: Wire confirmation audit**

In `agent.rs` where confirmation responses are processed, call `log_confirmation()`.

- [ ] **Step 4: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 5: Commit**

```
feat(agent): wire audit trail for shell exec and confirmations
```

---

### Task 17: Event Filtering for Internal Tools

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs` (tool call result broadcasting, line ~845-859)
- Modify: `backend/crates/sober-api/src/subscribe.rs` (line ~158-163)

- [ ] **Step 1: Set `internal` flag when broadcasting tool results**

In the agent loop where `ToolCallResult` events are broadcast, check tool metadata:

```rust
let internal = tool.map_or(false, |t| t.metadata().internal);
let _ = broadcast_tx.send(proto::ConversationUpdate {
    conversation_id: conv_id_str.clone(),
    event: Some(proto::conversation_update::Event::ToolCallResult(
        proto::ToolCallResult {
            name: tool_name.to_string(),
            output: output.clone(),
            internal,
        },
    )),
});
```

- [ ] **Step 2: Filter in API WebSocket relay**

In `sober-api/src/subscribe.rs`, in `conversation_update_to_ws()`, skip internal results:

```rust
Event::ToolCallResult(ref result) => {
    if result.internal {
        return None;
    }
    Some(ServerWsMessage::ChatToolResult { /* ... */ })
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd backend && cargo build -q -p sober-agent -p sober-api`

- [ ] **Step 4: Commit**

```
feat(agent,api): filter internal tool results from WebSocket stream
```

---

### Task 18: MCP Credential Injection

**Files:**
- Modify: `backend/crates/sober-mcp/src/pool.rs` (lines ~69-158)

- [ ] **Step 1: Add secret resolution helper**

```rust
/// Resolve MCP server credentials from user secrets.
/// Merges decrypted key-value pairs into the base env map.
async fn resolve_mcp_env<S: SecretRepo>(
    secret_repo: &S,
    mek: &Mek,
    user_id: UserId,
    conversation_id: Option<ConversationId>,
    server_name: &str,
    base_env: &HashMap<String, String>,
) -> Result<HashMap<String, String>, AppError> {
    let mut env = base_env.clone();
    let secrets = secret_repo.list_secrets(
        user_id, conversation_id, Some("mcp_server")
    ).await?;

    for meta in secrets {
        if meta.metadata.get("server").and_then(|v| v.as_str()) == Some(server_name) {
            if let Some(row) = secret_repo.get_secret(meta.id).await? {
                if let Some(stored_dek) = secret_repo.get_dek(user_id).await? {
                    let dek = mek.unwrap(&stored_dek.encrypted_dek)?;
                    let decrypted = dek.decrypt(&row.encrypted_data)?;
                    let kv: HashMap<String, String> = serde_json::from_slice(&decrypted)?;
                    env.extend(kv);
                    break;
                }
            }
        }
    }
    Ok(env)
}
```

- [ ] **Step 2: Wire into MCP server launch path**

Before `McpClient::connect()`, call `resolve_mcp_env()` and pass the enriched env.

- [ ] **Step 3: Verify compilation**

Run: `cd backend && cargo build -q -p sober-mcp`

- [ ] **Step 4: Commit**

```
feat(mcp): resolve credentials from secrets before server spawn
```

---

### Task 19: SOUL.md Update

**Files:**
- Modify: `backend/soul/SOUL.md` (after line ~103)

- [ ] **Step 1: Add "Artifact Discipline" section**

After the "Workspace Discipline" section, insert the full artifact guidance text from the design spec (see design.md lines 125-144).

- [ ] **Step 2: Commit**

```
feat(mind): add artifact discipline to SOUL.md
```

---

### Task 20: Final Integration and Verification

- [ ] **Step 1: Wire all new tools in `main.rs`**

Context-dependent tools (secrets, artifacts, snapshots) need per-conversation context (user_id, conversation_id, workspace_id). These are resolved at message-handling time, not at startup. Build a per-turn tool set in the agent loop:

```rust
// In agent loop, after workspace resolution:
let mut turn_tools: Vec<Arc<dyn Tool>> = vec![];

if let Some(mek) = &self.mek {
    let secret_ctx = Arc::new(SecretToolContext {
        secret_repo: Arc::new(self.repos.secrets().clone()),
        audit_repo: Arc::new(self.repos.audit_log().clone()),
        mek: Arc::clone(mek),
        user_id,
        conversation_id: Some(conversation_id),
    });
    turn_tools.push(Arc::new(StoreSecretTool::new(Arc::clone(&secret_ctx))));
    turn_tools.push(Arc::new(ReadSecretTool::new(Arc::clone(&secret_ctx))));
    turn_tools.push(Arc::new(ListSecretsTool::new(Arc::clone(&secret_ctx))));
    turn_tools.push(Arc::new(DeleteSecretTool::new(Arc::clone(&secret_ctx))));
}

if let Some(ref ws_dir) = workspace_dir {
    // Create artifact and snapshot tools with workspace context
    // ...
}
```

Merge `turn_tools` with the static `tool_registry` for that turn's tool definitions.

- [ ] **Step 2: Regenerate sqlx prepared statements**

Run: `cd backend && cargo sqlx prepare --workspace -q`
Commit `.sqlx/` changes.

- [ ] **Step 3: Full build**

Run: `cd backend && cargo build -q`
Expected: PASS

- [ ] **Step 4: Full clippy**

Run: `cd backend && cargo clippy -q -- -D warnings`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cd backend && cargo test --workspace -q`
Expected: PASS

- [ ] **Step 6: Final commit**

```
feat(agent): wire all workspace, secret, and audit tools into agent loop
```

---

## Dependency Order

```
Task 1 (migration)
  |
  v
Task 2 (core types) + Task 5 (proto) --- can run in parallel
  |
  v
Task 3 (SecretRepo trait)
  |
  v
Task 4 (AgentRepos)
  |
  v
Task 6 (PgSecretRepo) + Task 7 (PgAgentRepos) --- can run in parallel
  |
  v
Task 8 (LlmKeyResolver)

Task 9 (workspace layout) --- independent, can run any time

Task 10 (agent refactor) --- depends on Tasks 4, 7
  |
  v
Task 11 (workspace context) --- depends on Task 9
  |
  v
Task 12 (secret tools) + Task 13 (artifact tools) + Task 14 (snapshot tools)
  |                        --- can run in parallel ---
  v
Task 15 (tool call persistence)
  |
  v
Task 16 (audit trail) + Task 17 (event filtering) + Task 18 (MCP)
  |                    --- can run in parallel ---
  v
Task 19 (SOUL.md) + Task 20 (final integration)
```

**Parallelizable groups:**
- Tasks 2 + 5
- Tasks 6 + 7
- Task 9 (independent of all others)
- Tasks 12 + 13 + 14
- Tasks 16 + 17 + 18
