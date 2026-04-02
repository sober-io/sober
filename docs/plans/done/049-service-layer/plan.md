# Service Layer Extraction for sober-api

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract business logic from sober-api route handlers into a `services/` module so handlers are thin (parse HTTP, call service, format response).

**Architecture:** Every route group (except health/system) gets a service struct holding `PgPool` + needed clients. Services construct `Pg*Repo` instances per method call. Multi-step operations use `self.db.begin()` + `sqlx::query` against `&mut *tx` for atomicity. Response DTOs replace manual `serde_json::json!()`. WS message types move to their own file. Single PR.

**Tech Stack:** Rust, axum, sqlx, tonic, serde

### Transaction Strategy

Plan #047 introduced the `_tx` method pattern on repos: each repo method that may participate in a cross-repo transaction has a `_tx` variant accepting `&mut PgConnection`. Services call `self.db.begin()`, pass `&mut *tx` to `_tx` methods, then `tx.commit()`.

**Available `_tx` methods (from #047):**
- `PgConversationRepo::create_tx`, `create_inbox_tx`, `update_title_tx`, `convert_to_group_tx`, `update_agent_mode_tx`
- `PgWorkspaceRepo::provision_tx`
- `PgWorkspaceSettingsRepo::upsert_tx`
- `PgConversationUserRepo::reset_all_unread_tx`
- `PgMessageRepo::create_tx`, `delete_tx`, `clear_conversation_tx`
- `PgConversationAttachmentRepo::find_unreferenced_by_message_tx`, `delete_tx`

**Operations requiring service-level transactions:**

| Service | Method | `_tx` methods composed |
|---------|--------|----------------------|
| `ConversationService` | `create` | `provision_tx` + `create_tx` |
| `ConversationService` | `update_settings` | `update_agent_mode_tx` + `upsert_tx` |
| `ConversationService` | `clear_messages` | `clear_conversation_tx` + `reset_all_unread_tx` |
| `CollaboratorService` | `add` | `create_tx` (cu) + `create_tx` (event msg) |
| `CollaboratorService` | `update_role` | `update_role_tx` + `create_tx` (event msg) |
| `CollaboratorService` | `remove` / `leave` | `remove_collaborator_tx` + `create_tx` (event msg) + `convert_to_direct_tx` |
| `MessageService` | `delete` | `delete_tx` + `find_unreferenced_by_message_tx` + `delete_tx` (attachments) |

**New `_tx` methods needed:** `PgConversationUserRepo` needs `create_tx`, `remove_collaborator_tx`, `update_role_tx`. `PgConversationRepo` needs `convert_to_direct_tx`.

**Pattern:**
```rust
let mut tx = self.db.begin().await.map_err(|e| AppError::Internal(e.into()))?;

PgWorkspaceRepo::provision_tx(&mut *tx, user_id, &name, &root).await?;
PgConversationRepo::create_tx(&mut *tx, user_id, title, Some(ws_id)).await?;

tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;
```

---

### Task 1: Move WS types to dedicated module

**Files:**
- Create: `backend/crates/sober-api/src/ws_types.rs`
- Modify: `backend/crates/sober-api/src/lib.rs`
- Modify: `backend/crates/sober-api/src/routes/ws.rs`
- Modify: `backend/crates/sober-api/src/routes/collaborators.rs:17`
- Modify: `backend/crates/sober-api/src/connections.rs` (imports `ServerWsMessage`)
- Modify: `backend/crates/sober-api/src/subscribe.rs` (imports `ServerWsMessage`)

`ServerWsMessage`, `CollaboratorInfo` are WS protocol types used across 4 files. Moving them out of `routes/ws.rs` breaks the awkward `crate::routes::ws::ServerWsMessage` import path.

- [ ] **Step 1:** Create `ws_types.rs` with `ServerWsMessage` and `CollaboratorInfo` moved from `routes/ws.rs:29-230`. Keep `ClientWsMessage` in `ws.rs` (it's private to the handler).

- [ ] **Step 2:** Add `pub mod ws_types;` to `lib.rs`.

- [ ] **Step 3:** Update imports in all consumers:
  - `routes/ws.rs`: `use crate::ws_types::ServerWsMessage;`
  - `routes/collaborators.rs:17`: `use crate::ws_types::{CollaboratorInfo, ServerWsMessage};`
  - `connections.rs`: `use crate::ws_types::ServerWsMessage;`
  - `subscribe.rs`: `use crate::ws_types::ServerWsMessage;`

- [ ] **Step 4:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q`

- [ ] **Step 5:** Commit: `refactor(api): move WS types to dedicated ws_types module`

---

### Task 2: Create services module scaffold + add to AppState

**Files:**
- Create: `backend/crates/sober-api/src/services/mod.rs`
- Create: `backend/crates/sober-api/src/services/tag.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/user.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/conversation.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/collaborator.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/message.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/ws_dispatch.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/plugin.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/evolution.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/attachment.rs` (empty struct)
- Create: `backend/crates/sober-api/src/services/auth.rs` (empty struct)
- Modify: `backend/crates/sober-api/src/lib.rs`
- Modify: `backend/crates/sober-api/src/state.rs`

- [ ] **Step 1:** Create `services/mod.rs` with submodule declarations and a shared `verify_membership` helper (moved from `routes/mod.rs:31-38`):

```rust
//! Service layer — business logic extracted from route handlers.

pub mod attachment;
pub mod auth;
pub mod collaborator;
pub mod conversation;
pub mod evolution;
pub mod message;
pub mod plugin;
pub mod tag;
pub mod user;
pub mod ws_dispatch;

use sober_core::error::AppError;
use sober_core::types::{ConversationId, ConversationUser, ConversationUserRepo, UserId};
use sober_db::PgConversationUserRepo;
use sqlx::PgPool;

/// Verify the authenticated user is a member of the conversation.
pub(crate) async fn verify_membership(
    db: &PgPool,
    conversation_id: ConversationId,
    user_id: UserId,
) -> Result<ConversationUser, AppError> {
    let cu_repo = PgConversationUserRepo::new(db.clone());
    cu_repo.get(conversation_id, user_id).await
}
```

- [ ] **Step 2:** Create each service file with struct + constructor. Each holds `PgPool` plus any additional dependencies. Example for tag:

```rust
use sqlx::PgPool;

pub struct TagService {
    pub(crate) db: PgPool,
}

impl TagService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
```

Service dependencies:

| Service | Fields |
|---------|--------|
| `TagService` | `db: PgPool` |
| `UserService` | `db: PgPool` |
| `ConversationService` | `db: PgPool, config: AppConfig` |
| `CollaboratorService` | `db: PgPool, user_connections: UserConnectionRegistry` |
| `MessageService` | `db: PgPool` |
| `WsDispatchService` | `db: PgPool, agent_client: AgentClient, connections: ConnectionRegistry` |
| `PluginService` | `db: PgPool, agent_client: AgentClient` |
| `EvolutionService` | `db: PgPool, agent_client: AgentClient, config: AppConfig` |
| `AttachmentService` | `db: PgPool, blob_store: Arc<BlobStore>` |
| `AuthService` | `db: PgPool` (for inbox creation — delegates auth to `AuthService`) |

- [ ] **Step 3:** Add `pub mod services;` to `lib.rs`.

- [ ] **Step 4:** Add service fields to `AppState` and construct them in `new()` and `from_parts()`. Services are wrapped in `Arc`:

```rust
// In AppState struct:
pub tag: Arc<TagService>,
pub user: Arc<UserService>,
pub conversation: Arc<ConversationService>,
pub collaborator: Arc<CollaboratorService>,
pub message: Arc<MessageService>,
pub ws_dispatch: Arc<WsDispatchService>,
pub plugin: Arc<PluginService>,
pub evolution: Arc<EvolutionService>,
pub attachment: Arc<AttachmentService>,
pub auth_api: Arc<AuthService>,
```

Construct in both `new()` and `from_parts()`:
```rust
let tag = Arc::new(TagService::new(db.clone()));
let user = Arc::new(UserService::new(db.clone()));
let conversation = Arc::new(ConversationService::new(db.clone(), config.clone()));
let collaborator = Arc::new(CollaboratorService::new(db.clone(), user_connections.clone()));
let message = Arc::new(MessageService::new(db.clone()));
let ws_dispatch = Arc::new(WsDispatchService::new(db.clone(), agent_client.clone(), connections.clone()));
let plugin = Arc::new(PluginService::new(db.clone(), agent_client.clone()));
let evolution = Arc::new(EvolutionService::new(db.clone(), agent_client.clone(), config.clone()));
let attachment = Arc::new(AttachmentService::new(db.clone(), blob_store.clone()));
let auth_api = Arc::new(AuthService::new(db.clone()));
```

- [ ] **Step 5:** Update `routes/mod.rs`: change `verify_membership` to re-export from services: `pub use crate::services::verify_membership;` (keep `insert_event_message` for now — moves in Task 5).

- [ ] **Step 6:** Update any integration tests that call `AppState::from_parts()` — the new service fields must be constructed there too.

- [ ] **Step 7:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q`

- [ ] **Step 8:** Commit: `refactor(api): scaffold services module and wire to AppState`

---

### Task 3: TagService + UserService

**Files:**
- Modify: `backend/crates/sober-api/src/services/tag.rs`
- Modify: `backend/crates/sober-api/src/services/user.rs`
- Modify: `backend/crates/sober-api/src/routes/tags.rs`
- Modify: `backend/crates/sober-api/src/routes/users.rs`
- Modify: `backend/crates/sober-api/src/routes/messages.rs` (message tagging)

- [ ] **Step 1:** Implement `TagService` methods using pool-based repo calls (tag operations are individually idempotent — `create_or_get` is an upsert, `tag_conversation` is INSERT ON CONFLICT — so transactions add no safety benefit):

```rust
use sober_core::error::AppError;
use sober_core::types::{ConversationId, CreateTag, MessageId, Tag, TagId, UserId, MessageRepo};
use sober_db::{PgMessageRepo, PgTagRepo};
use sqlx::PgPool;

pub struct TagService {
    db: PgPool,
}

impl TagService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Tag>, AppError> {
        let repo = PgTagRepo::new(self.db.clone());
        repo.list_by_user(user_id).await
    }

    pub async fn add_to_conversation(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        name: String,
    ) -> Result<Tag, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;
        let repo = PgTagRepo::new(self.db.clone());
        let tag = repo.create_or_get(CreateTag { user_id, name }).await?;
        repo.tag_conversation(conversation_id, tag.id).await?;
        Ok(tag)
    }

    pub async fn remove_from_conversation(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;
        PgTagRepo::new(self.db.clone()).untag_conversation(conversation_id, tag_id).await
    }

    pub async fn add_to_message(
        &self,
        message_id: MessageId,
        user_id: UserId,
        name: String,
    ) -> Result<Tag, AppError> {
        let msg = PgMessageRepo::new(self.db.clone()).get_by_id(message_id).await?;
        super::verify_membership(&self.db, msg.conversation_id, user_id).await?;
        let repo = PgTagRepo::new(self.db.clone());
        let tag = repo.create_or_get(CreateTag { user_id, name }).await?;
        repo.tag_message(message_id, tag.id).await?;
        Ok(tag)
    }

    pub async fn remove_from_message(
        &self,
        message_id: MessageId,
        user_id: UserId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        let msg = PgMessageRepo::new(self.db.clone()).get_by_id(message_id).await?;
        super::verify_membership(&self.db, msg.conversation_id, user_id).await?;
        PgTagRepo::new(self.db.clone()).untag_message(message_id, tag_id).await
    }
}
```

- [ ] **Step 3:** Implement `UserService` with a typed response DTO:

```rust
use serde::Serialize;
use sober_core::error::AppError;
use sober_core::types::UserRepo;
use sober_db::PgUserRepo;
use sqlx::PgPool;

#[derive(Serialize)]
pub struct UserSearchResult {
    pub id: String,
    pub username: String,
}

pub struct UserService {
    db: PgPool,
}

impl UserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<UserSearchResult>, AppError> {
        let repo = PgUserRepo::new(self.db.clone());
        let users = repo.search_by_username(query, limit).await?;
        Ok(users.into_iter().map(|u| UserSearchResult {
            id: u.id.to_string(),
            username: u.username,
        }).collect())
    }
}
```

- [ ] **Step 2:** Slim down `routes/tags.rs` handlers to delegate to `state.tag`:

```rust
async fn list_tags(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<Vec<Tag>>, AppError> {
    let tags = state.tag.list_by_user(auth_user.user_id).await?;
    Ok(ApiResponse::new(tags))
}

async fn add_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<AddTagRequest>,
) -> Result<ApiResponse<Tag>, AppError> {
    let tag = state.tag.add_to_conversation(
        ConversationId::from_uuid(id), auth_user.user_id, body.name,
    ).await?;
    Ok(ApiResponse::new(tag))
}

async fn remove_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, tag_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state.tag.remove_from_conversation(
        ConversationId::from_uuid(id), auth_user.user_id, TagId::from_uuid(tag_id),
    ).await?;
    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}
```

- [ ] **Step 4:** Slim down `routes/users.rs`:

```rust
async fn search_users(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Query(params): Query<SearchUsersQuery>,
) -> Result<ApiResponse<Vec<UserSearchResult>>, AppError> {
    let results = state.user.search(&params.q, 10).await?;
    Ok(ApiResponse::new(results))
}
```

- [ ] **Step 5:** Slim down message tagging in `routes/messages.rs` (`add_message_tag`, `remove_message_tag`) to delegate to `state.tag.add_to_message(...)` and `state.tag.remove_from_message(...)`.

- [ ] **Step 6:** Remove unused imports from all modified handler files.

- [ ] **Step 7:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 8:** Commit: `refactor(api): extract TagService and UserService`

---

### Task 4: ConversationService

**Files:**
- Modify: `backend/crates/sober-api/src/services/conversation.rs`
- Modify: `backend/crates/sober-api/src/routes/conversations.rs`

- [ ] **Step 1:** Define response DTOs in the service (replace `serde_json::json!()` calls):

```rust
#[derive(Serialize)]
pub struct CreateConversationResponse {
    pub id: String,
    pub title: Option<String>,
    pub workspace_id: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
    pub unread_count: i32,
    pub last_read_message_id: Option<String>,
    pub tags: Vec<Tag>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct UpdateConversationResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
}

#[derive(Serialize)]
pub struct InboxResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub is_archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ConvertToGroupResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
}
```

Also move `SettingsResponse` from `routes/conversations.rs:251-269` into the service.

- [ ] **Step 2:** Implement `ConversationService` methods. Each method does what the handler currently does minus HTTP extraction:

```rust
pub struct ConversationService {
    db: PgPool,
    config: AppConfig,
}
```

Methods:
- `list(user_id, filter) -> Result<Vec<ConversationWithDetails>, AppError>` — delegates to repo
- `create(user_id, title: Option<&str>) -> Result<CreateConversationResponse, AppError>` — **tx**: `provision_tx` + `create_tx`. `MAX_WORKSPACE_NAME_LEN` constant moves here.
- `get(conversation_id, user_id) -> Result<ConversationWithDetails, AppError>` — membership check, tags, users, workspace join
- `update(conversation_id, user_id, title, archived) -> Result<UpdateConversationResponse, AppError>`
- `delete(conversation_id, user_id) -> Result<(), AppError>` — owner check
- `get_settings(conversation_id, user_id) -> Result<SettingsResponse, AppError>`
- `update_settings(conversation_id, user_id, body: UpdateSettingsInput) -> Result<SettingsResponse, AppError>` — **tx**: `update_agent_mode_tx` + `upsert_tx`. Define `UpdateSettingsInput` as a plain struct (not Deserialize — handler deserializes, service takes typed input).
- `mark_read(conversation_id, user_id, message_id: Option<MessageId>) -> Result<(), AppError>` — fallback to latest message
- `get_inbox(user_id) -> Result<InboxResponse, AppError>`
- `convert_to_group(conversation_id, user_id, title: &str) -> Result<ConvertToGroupResponse, AppError>` — owner + kind check
- `clear_messages(conversation_id, user_id) -> Result<(), AppError>` — **tx**: `clear_conversation_tx` + `reset_all_unread_tx`. Owner check.
- `list_jobs(conversation_id, user_id) -> Result<Vec<Job>, AppError>`

Example `create` (moved from handler, already uses `_tx` after #047):
```rust
pub async fn create(&self, user_id: UserId, title: Option<&str>) -> Result<CreateConversationResponse, AppError> {
    let ws_name = title.unwrap_or("untitled").chars().take(MAX_WORKSPACE_NAME_LEN).collect::<String>();
    let ws_root = format!("{}/{}", self.config.workspace_root.display(), uuid::Uuid::now_v7());

    let mut tx = self.db.begin().await.map_err(|e| AppError::Internal(e.into()))?;

    let (workspace, _settings) = PgWorkspaceRepo::provision_tx(&mut tx, user_id, &ws_name, &ws_root).await?;
    let conversation = PgConversationRepo::create_tx(&mut tx, user_id, title, Some(workspace.id)).await?;

    tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;

    Ok(CreateConversationResponse { /* fields from conversation */ })
}
```

Example `clear_messages`:
```rust
pub async fn clear_messages(&self, conversation_id: ConversationId, user_id: UserId) -> Result<(), AppError> {
    let membership = super::verify_membership(&self.db, conversation_id, user_id).await?;
    if membership.role != ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    let mut tx = self.db.begin().await.map_err(|e| AppError::Internal(e.into()))?;
    PgMessageRepo::clear_conversation_tx(&mut tx, conversation_id).await?;
    PgConversationUserRepo::reset_all_unread_tx(&mut tx, conversation_id).await?;
    tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;

    Ok(())
}
```

- [ ] **Step 3:** Slim down `routes/conversations.rs` handlers to extract → service → response. Remove business logic, repo construction, and `serde_json::json!()` from handlers.

- [ ] **Step 4:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 5:** Commit: `refactor(api): extract ConversationService`

---

### Task 5: CollaboratorService

**Files:**
- Modify: `backend/crates/sober-api/src/services/collaborator.rs`
- Modify: `backend/crates/sober-api/src/routes/collaborators.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs` (remove `insert_event_message`)

- [ ] **Step 1:** Implement `CollaboratorService`. Move `insert_event_message` from `routes/mod.rs:41-58` as a private helper method. Move all authorization logic, event creation, and WS broadcasting:

```rust
pub struct CollaboratorService {
    db: PgPool,
    user_connections: UserConnectionRegistry,
}
```

Methods:
- `list(conversation_id, user_id) -> Result<Vec<ConversationUserWithUsername>, AppError>`
- `add(conversation_id, caller_user_id, target_username: &str) -> Result<ConversationUserWithUsername, AppError>` — auth (owner/admin), idempotency, **tx: `create_tx` (cu) + `create_tx` (event msg)**, WS broadcast
- `update_role(conversation_id, caller_user_id, target_user_id, role) -> Result<(), AppError>` — role validation, owner-only, **tx: `update_role_tx` + `create_tx` (event msg)**, WS broadcast
- `remove(conversation_id, caller_user_id, target_user_id) -> Result<(), AppError>` — auth matrix, **tx: `remove_collaborator_tx` + `create_tx` (event msg) + `convert_to_direct_tx`**, broadcast to remaining + kicked
- `leave(conversation_id, user_id) -> Result<(), AppError>` — owner cannot leave, **tx: `remove_collaborator_tx` + `create_tx` (event msg) + `convert_to_direct_tx`**, broadcast

Note: `PgConversationUserRepo` needs new `_tx` methods: `create_tx`, `remove_collaborator_tx`, `update_role_tx`. Also `PgConversationRepo` needs `convert_to_direct_tx`.

Private helper (uses `_tx` to participate in the caller's transaction):
```rust
async fn insert_event_message_tx(
    conn: &mut PgConnection,
    conversation_id: ConversationId,
    content: &str,
    metadata: serde_json::Value,
) -> Result<Message, AppError> {
    PgMessageRepo::create_tx(conn, CreateMessage {
        conversation_id,
        role: MessageRole::Event,
        content: vec![ContentBlock::text(content)],
        reasoning: None,
        token_count: None,
        metadata: Some(metadata),
        user_id: None,
    }).await
}
```

Example usage in `add()`:
```rust
pub async fn add(&self, conversation_id: ConversationId, caller_id: UserId, target_username: &str)
    -> Result<ConversationUserWithUsername, AppError>
{
    // ... auth checks, lookup target user ...

    let mut tx = self.db.begin().await.map_err(|e| AppError::Internal(e.into()))?;

    PgConversationUserRepo::create_tx(&mut tx, conversation_id, target_user.id, ConversationUserRole::Member).await?;

    let content = format!("{} added {}", actor.username, target_user.username);
    Self::insert_event_message_tx(&mut tx, conversation_id, &content, metadata).await?;

    tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;

    // WS broadcast (outside tx — fire and forget)
    self.user_connections.send(...).await;

    Ok(new_collaborator)
}
```

- [ ] **Step 2:** Slim down `routes/collaborators.rs` handlers.

- [ ] **Step 3:** Remove `insert_event_message` from `routes/mod.rs`. Remove the `verify_membership` function body and leave only the re-export (or remove entirely if all callers now use `services::verify_membership` directly).

- [ ] **Step 4:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 5:** Commit: `refactor(api): extract CollaboratorService`

---

### Task 6: MessageService

**Files:**
- Modify: `backend/crates/sober-api/src/services/message.rs`
- Modify: `backend/crates/sober-api/src/routes/messages.rs`

- [ ] **Step 1:** Define `MessageWithDetails` response DTO to replace the manual `serde_json::Value` assembly in `list_messages`:

```rust
#[derive(Serialize)]
pub struct MessageWithDetails {
    #[serde(flatten)]
    pub message: Message,
    pub tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_executions: Vec<ToolExecution>,
    pub attachments: HashMap<String, ConversationAttachment>,
}
```

**Important:** Verify that `#[serde(flatten)]` on `Message` produces the same JSON shape as `serde_json::to_value(&msg)`. If `Message` already derives `Serialize`, this should work. The key difference is that `tags`, `tool_executions`, and `attachments` are now typed instead of `serde_json::Value`.

- [ ] **Step 2:** Implement `MessageService`:

```rust
pub struct MessageService {
    db: PgPool,
}
```

Methods:
- `list(conversation_id, user_id, before, limit) -> Result<Vec<MessageWithDetails>, AppError>` — the entire batch-fetch logic from the current handler (tags, tool executions, attachments)
- `delete(message_id, user_id) -> Result<(), AppError>` — auth check (owner or sender), **tx: `delete_tx` + `find_unreferenced_by_message_tx` + `delete_tx` (attachments)** (already atomic in handler after #047, move as-is)

- [ ] **Step 3:** Slim down `routes/messages.rs`. The `list_messages` handler goes from 144 lines to ~5. The `delete_message` handler goes from 56 lines to ~3. Remove `add_message_tag` and `remove_message_tag` business logic (already moved to TagService in Task 3).

- [ ] **Step 4:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 5:** Commit: `refactor(api): extract MessageService`

---

### Task 7: WsDispatchService

**Files:**
- Modify: `backend/crates/sober-api/src/services/ws_dispatch.rs`
- Modify: `backend/crates/sober-api/src/routes/ws.rs`

- [ ] **Step 1:** Implement `WsDispatchService`. Extract the business logic from each `match` arm in `handle_socket`:

```rust
pub struct WsDispatchService {
    db: PgPool,
    agent_client: AgentClient,
    connections: ConnectionRegistry,
}
```

Methods:
- `subscribe(conversation_id: &str, user_id: UserId) -> Result<(), AppError>` — verify membership, mark read (best-effort)
- `send_message(conversation_id: &str, user_id: UserId, username: &str, content: Vec<ContentBlock>) -> Result<(), AppError>` — verify membership, broadcast user message via `connections`, broadcast typing indicator, proto conversion, fire-and-forget gRPC `HandleMessage` (spawn task with tracing)
- `confirm_response(confirm_id: String, approved: bool) -> Result<(), AppError>` — gRPC `SubmitConfirmation`
- `set_permission_mode(mode: String) -> Result<(), AppError>` — gRPC `SetPermissionMode`

Note: `send_message` needs access to `connections` for broadcasting. It also needs access to `out_tx` for error reporting — pass an `error_tx: mpsc::Sender<ServerWsMessage>` parameter for the spawned task's error path.

**Proto conversion** (the `ContentBlock -> proto::ContentBlock` mapping, currently lines 439-478 of ws.rs) moves to a private helper in the service or as a `From` impl.

- [ ] **Step 2:** Slim down `routes/ws.rs`. The `handle_socket` function keeps:
  - Socket split, send task, ping/pong loop
  - Registration/unregistration with connection registries
  - Deserializing `ClientWsMessage`
  - Calling service methods and handling channel errors

Each match arm becomes a service call.

- [ ] **Step 3:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 4:** Commit: `refactor(api): extract WsDispatchService`

---

### Task 8: PluginService

**Files:**
- Modify: `backend/crates/sober-api/src/services/plugin.rs`
- Modify: `backend/crates/sober-api/src/routes/plugins.rs`

- [ ] **Step 1:** Define response DTOs:

```rust
#[derive(Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: String,
    pub description: String,
    pub status: String,
    pub scope: String,
    pub config: serde_json::Value,
    pub installed_at: String,
}

impl From<proto::PluginInfo> for PluginInfo { ... } // replaces plugin_info_to_json

#[derive(Serialize)]
pub struct ImportResult {
    pub imported_count: i32,
    pub plugins: Vec<PluginInfo>,
}

#[derive(Serialize)]
pub struct ReloadResult {
    pub active_count: i32,
}

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

#[derive(Serialize)]
pub struct AuditLogEntry {
    pub id: String,
    pub plugin_id: Option<String>,
    pub plugin_name: Option<String>,
    pub kind: String,
    pub origin: String,
    pub stages: serde_json::Value,
    pub verdict: String,
    pub rejection_reason: Option<String>,
    pub audited_at: String,
    pub audited_by: Option<String>,
}
```

- [ ] **Step 2:** Implement `PluginService` methods — each wraps the gRPC proxy logic:

```rust
pub struct PluginService {
    db: PgPool,
    agent_client: AgentClient,
}
```

Methods: `list`, `install`, `import`, `reload`, `get`, `update`, `uninstall`, `list_audit_logs`, `list_skills`, `reload_skills`, `list_tools`.

- [ ] **Step 3:** Slim down `routes/plugins.rs`. Remove `plugin_info_to_json` helper.

- [ ] **Step 4:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 5:** Commit: `refactor(api): extract PluginService`

---

### Task 9: EvolutionService

**Files:**
- Modify: `backend/crates/sober-api/src/services/evolution.rs`
- Modify: `backend/crates/sober-api/src/routes/evolution.rs`

- [ ] **Step 1:** Implement `EvolutionService`. Move `EvolutionConfigResponse` from routes to service. Move the state machine validation logic:

```rust
pub struct EvolutionService {
    db: PgPool,
    agent_client: AgentClient,
    config: AppConfig,
}
```

Methods:
- `list_events(evolution_type, status) -> Result<Vec<EvolutionEvent>, AppError>`
- `get_event(id) -> Result<EvolutionEvent, AppError>`
- `update_event(id, target_status, admin_user_id) -> Result<EvolutionEvent, AppError>` — state machine validation + gRPC dispatch
- `get_config() -> Result<EvolutionConfigResponse, AppError>`
- `update_config(updates: UpdateConfigInput) -> Result<EvolutionConfigResponse, AppError>`
- `get_timeline(limit, evolution_type, status) -> Result<Vec<EvolutionEvent>, AppError>`

- [ ] **Step 2:** Slim down `routes/evolution.rs` handlers. Keep `RequireAdmin` extraction in handlers (authorization is an HTTP concern).

- [ ] **Step 3:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 4:** Commit: `refactor(api): extract EvolutionService`

---

### Task 10: AttachmentService

**Files:**
- Modify: `backend/crates/sober-api/src/services/attachment.rs`
- Modify: `backend/crates/sober-api/src/routes/attachments.rs`

- [ ] **Step 1:** Implement `AttachmentService`. Multipart parsing stays in the handler. Processing, storage, and metrics move to service:

```rust
pub struct AttachmentService {
    db: PgPool,
    blob_store: Arc<BlobStore>,
}
```

Methods:
- `upload(conversation_id, user_id, filename: String, data: Vec<u8>) -> Result<ConversationAttachment, AppError>` — validate content type, process image/document, store blob, create record, record metrics

`serve_attachment` stays in the handler — it's pure HTTP response building (content-type, cache-control, content-disposition headers). `MAX_UPLOAD_SIZE` stays in the handler too — the size check is a multipart parsing concern.

- [ ] **Step 2:** Slim down `routes/attachments.rs`. `upload_attachment` handler: multipart extraction + size check → service call. `serve_attachment` stays as-is (thin handler calling repo + blob_store directly).

- [ ] **Step 3:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 4:** Commit: `refactor(api): extract AttachmentService`

---

### Task 11: AuthService

**Files:**
- Modify: `backend/crates/sober-api/src/services/auth.rs`
- Modify: `backend/crates/sober-api/src/routes/auth.rs`

- [ ] **Step 1:** Implement `AuthService`. The actual auth logic stays in `sober-auth::AuthService`. This service handles the API-level concerns that don't belong in the auth crate (inbox creation on register, role formatting on /me):

```rust
pub struct AuthService {
    db: PgPool,
}
```

Methods:
- `create_inbox_for_user(user_id: UserId) -> Result<(), AppError>` — called after register
- `get_user_with_roles(user_id: UserId) -> Result<UserProfile, AppError>` — user + role lookup for /me

```rust
#[derive(Serialize)]
pub struct UserProfile {
    pub id: String,
    pub email: String,
    pub username: String,
    pub status: String,
    pub roles: Vec<String>,
}
```

- [ ] **Step 2:** Slim down `routes/auth.rs` handlers. `register`: auth.register() → auth_api.create_inbox(). `me`: auth_api.get_user_with_roles(). `login` and `logout` stay mostly as-is since they're HTTP-specific (cookies, headers).

- [ ] **Step 3:** Verify: `cargo build -p sober-api -q && cargo test -p sober-api -q && cargo clippy -p sober-api -q -- -D warnings`

- [ ] **Step 4:** Commit: `refactor(api): extract AuthService`

---

### Task 12: Final cleanup

**Files:**
- Modify: `backend/crates/sober-api/src/routes/mod.rs`

- [ ] **Step 1:** Clean up `routes/mod.rs`:
  - Remove `verify_membership` function body (should now just re-export or be gone)
  - Remove `insert_event_message` (moved to CollaboratorService)
  - Remove unused imports (`CreateMessage`, `MessageRole`, `PgConversationUserRepo`, `PgMessageRepo`, etc.)

- [ ] **Step 2:** Run full workspace verification:

```bash
cargo build -p sober-api -q
cargo test -p sober-api -q
cargo clippy -p sober-api -q -- -D warnings
cargo fmt -p sober-api --check -q
```

- [ ] **Step 3:** Run full workspace tests to catch any cross-crate breakage:

```bash
cargo test --workspace -q
cargo clippy --workspace -q -- -D warnings
```

- [ ] **Step 4:** Commit: `refactor(api): clean up routes/mod.rs after service extraction`

---

### Task 13: Update documentation

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/rust-patterns.md`

- [ ] **Step 1:** Update `ARCHITECTURE.md` — in the crate map table, update the `sober-api` entry to mention the service layer:

```
| `sober-api` | HTTP/WebSocket API gateway, rate limiting, channel adapters, Unix admin socket. Thin route handlers delegate to `services/` module for business logic. |
```

Add a new subsection under the API Gateway section describing the handler → service → repo flow.

- [ ] **Step 2:** Update `docs/rust-patterns.md` — add two new sections:

**Service Layer Pattern:**
- Service structs hold `PgPool` + needed clients, constructed in `AppState`
- Methods return `Result<T, AppError>` with typed response DTOs
- Handlers: extract HTTP input → call service → wrap in `ApiResponse`
- Multi-table operations use `_tx` repo methods within a service-level transaction

**Transaction Composition (`_tx` pattern):**
- Repos provide `_tx` method variants accepting `&mut PgConnection`
- Services call `self.db.begin()`, compose `_tx` calls, then `tx.commit()`
- Read-only operations and single-table writes use pool-based repo methods
- WS broadcasts and other side effects happen outside the transaction

- [ ] **Step 3:** Verify docs build (if mdBook is configured): `cd docs && mdbook build`

- [ ] **Step 4:** Commit: `docs(arch): document service layer and _tx transaction pattern`

---

## Verification

After all tasks:

1. **Build:** `cargo build --workspace -q`
2. **Clippy:** `cargo clippy --workspace -q -- -D warnings`
3. **Tests:** `cargo test --workspace -q`
4. **Format:** `cargo fmt --check -q`
5. **Manual smoke test:** Start the stack with `docker compose up -d --build --quiet-pull`, verify WebSocket, conversation CRUD, collaborator management, and plugin operations work via the frontend.
