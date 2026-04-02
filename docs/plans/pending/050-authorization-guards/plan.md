# #050: Authorization Guards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Centralize all authorization checks into reusable guard functions (backend) and guard utilities + components (frontend), replacing scattered inline role checks. Add missing plugin authorization enforcement.

**Architecture:** Backend gets a `guards` module in `sober-api` with composable check functions called from services. Frontend gets a `$lib/guards/` module with reactive helpers and `<RequireRole>`/`<RequireConversationRole>` wrapper components. Proto `PluginInfo` gains an `owner_id` field so plugin authorization can work end-to-end.

**Tech Stack:** Rust (axum, sober-auth, sober-core), Svelte 5 (runes, snippets), protobuf

---

### Task 1: Backend Guards Module

**Files:**
- Create: `backend/crates/sober-api/src/guards.rs`
- Modify: `backend/crates/sober-api/src/lib.rs` (add `pub mod guards;`)

- [ ] **Step 1: Write unit tests for guard functions**

Create `backend/crates/sober-api/src/guards.rs` with tests at the bottom:

```rust
//! Reusable authorization guard functions.
//!
//! Layer 2 authorization: services call these after fetching context
//! (membership, plugin, etc.) from the database. Layer 1 (extractors
//! like `AuthUser` and `RequireAdmin`) handles coarse route-level gating.

use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ConversationUser, ConversationUserRole, Plugin, PluginScope, RoleKind, UserId,
};

/// Requires the user to hold the admin role.
pub fn require_admin(user: &AuthUser) -> Result<(), AppError> {
    if user.has_role(&RoleKind::Admin) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Requires the user to hold at least the given conversation role.
///
/// Role hierarchy: Owner > Admin > Member.
pub fn require_conversation_role(
    membership: &ConversationUser,
    minimum: ConversationUserRole,
) -> Result<(), AppError> {
    if role_rank(membership.role) >= role_rank(minimum) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Requires the user to be the conversation owner.
pub fn require_owner(membership: &ConversationUser) -> Result<(), AppError> {
    require_conversation_role(membership, ConversationUserRole::Owner)
}

/// Requires the user to be the conversation owner or the sender of a message.
pub fn require_owner_or_sender(
    membership: &ConversationUser,
    sender_id: Option<UserId>,
    acting_user_id: UserId,
) -> Result<(), AppError> {
    let is_owner = membership.role == ConversationUserRole::Owner;
    let is_sender = sender_id == Some(acting_user_id);
    if is_owner || is_sender {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Checks whether a caller can remove a user with `target_role`.
///
/// Rules: nobody can remove the owner; owner can remove anyone else;
/// admin can only remove members; members cannot remove anyone.
pub fn check_can_remove(
    caller_role: ConversationUserRole,
    target_role: ConversationUserRole,
) -> Result<(), AppError> {
    if target_role == ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }
    match caller_role {
        ConversationUserRole::Owner => Ok(()),
        ConversationUserRole::Admin => {
            if target_role != ConversationUserRole::Member {
                return Err(AppError::Forbidden);
            }
            Ok(())
        }
        ConversationUserRole::Member => Err(AppError::Forbidden),
    }
}

/// Checks whether the user can modify (update/delete) a plugin.
///
/// - System plugins: admin only.
/// - User plugins: owner only.
/// - Workspace plugins: owner or system admin.
pub fn can_modify_plugin(user: &AuthUser, plugin: &Plugin) -> Result<(), AppError> {
    match plugin.scope {
        PluginScope::System => require_admin(user),
        PluginScope::User => {
            if plugin.owner_id == Some(user.user_id) {
                Ok(())
            } else {
                Err(AppError::Forbidden)
            }
        }
        PluginScope::Workspace => {
            if plugin.owner_id == Some(user.user_id) {
                Ok(())
            } else {
                require_admin(user)
            }
        }
    }
}

fn role_rank(role: ConversationUserRole) -> u8 {
    match role {
        ConversationUserRole::Member => 1,
        ConversationUserRole::Admin => 2,
        ConversationUserRole::Owner => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::{ConversationId, PluginId, PluginKind, PluginOrigin, PluginStatus};

    fn auth_user(roles: Vec<RoleKind>) -> AuthUser {
        AuthUser {
            user_id: UserId::new(),
            roles,
        }
    }

    fn admin_user() -> AuthUser {
        auth_user(vec![RoleKind::User, RoleKind::Admin])
    }

    fn regular_user() -> AuthUser {
        auth_user(vec![RoleKind::User])
    }

    fn membership(role: ConversationUserRole) -> ConversationUser {
        ConversationUser {
            conversation_id: ConversationId::new(),
            user_id: UserId::new(),
            role,
            joined_at: chrono::Utc::now(),
            unread_count: 0,
            last_read_message_id: None,
        }
    }

    fn test_plugin(scope: PluginScope, owner_id: Option<UserId>) -> Plugin {
        Plugin {
            id: PluginId::new(),
            name: "test".into(),
            kind: PluginKind::Mcp,
            version: None,
            description: None,
            origin: PluginOrigin::User,
            scope,
            owner_id,
            workspace_id: None,
            status: PluginStatus::Enabled,
            config: serde_json::json!({}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // --- require_admin ---

    #[test]
    fn require_admin_passes_for_admin() {
        assert!(require_admin(&admin_user()).is_ok());
    }

    #[test]
    fn require_admin_fails_for_regular_user() {
        assert!(require_admin(&regular_user()).is_err());
    }

    // --- require_conversation_role ---

    #[test]
    fn owner_satisfies_any_minimum() {
        let m = membership(ConversationUserRole::Owner);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Owner).is_ok());
    }

    #[test]
    fn admin_satisfies_admin_and_member() {
        let m = membership(ConversationUserRole::Admin);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Owner).is_err());
    }

    #[test]
    fn member_only_satisfies_member() {
        let m = membership(ConversationUserRole::Member);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_err());
    }

    // --- require_owner ---

    #[test]
    fn require_owner_passes_for_owner() {
        assert!(require_owner(&membership(ConversationUserRole::Owner)).is_ok());
    }

    #[test]
    fn require_owner_fails_for_non_owner() {
        assert!(require_owner(&membership(ConversationUserRole::Admin)).is_err());
        assert!(require_owner(&membership(ConversationUserRole::Member)).is_err());
    }

    // --- require_owner_or_sender ---

    #[test]
    fn owner_can_always_act() {
        let m = membership(ConversationUserRole::Owner);
        let other = UserId::new();
        assert!(require_owner_or_sender(&m, Some(other), m.user_id).is_ok());
    }

    #[test]
    fn sender_can_act_on_own_message() {
        let user_id = UserId::new();
        let mut m = membership(ConversationUserRole::Member);
        m.user_id = user_id;
        assert!(require_owner_or_sender(&m, Some(user_id), user_id).is_ok());
    }

    #[test]
    fn non_owner_non_sender_is_rejected() {
        let m = membership(ConversationUserRole::Member);
        let other = UserId::new();
        assert!(require_owner_or_sender(&m, Some(other), m.user_id).is_err());
    }

    // --- check_can_remove ---

    #[test]
    fn owner_can_remove_admin_and_member() {
        assert!(check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Admin).is_ok());
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Member).is_ok()
        );
    }

    #[test]
    fn nobody_can_remove_owner() {
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Owner).is_err()
        );
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Owner).is_err()
        );
    }

    #[test]
    fn admin_can_remove_member_only() {
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Member).is_ok()
        );
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Admin).is_err()
        );
    }

    #[test]
    fn member_cannot_remove_anyone() {
        assert!(
            check_can_remove(ConversationUserRole::Member, ConversationUserRole::Member).is_err()
        );
    }

    // --- can_modify_plugin ---

    #[test]
    fn admin_can_modify_system_plugin() {
        let user = admin_user();
        let plugin = test_plugin(PluginScope::System, None);
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn regular_user_cannot_modify_system_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::System, None);
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }

    #[test]
    fn owner_can_modify_own_user_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::User, Some(user.user_id));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn non_owner_cannot_modify_user_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::User, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }

    #[test]
    fn owner_can_modify_workspace_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(user.user_id));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn admin_can_modify_any_workspace_plugin() {
        let user = admin_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn non_owner_non_admin_cannot_modify_workspace_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-api --lib guards -q`

Note: Tests should pass immediately since implementations are included. This is a data-in/data-out module with no IO — tests don't need mocks or DB.

- [ ] **Step 3: Register the module**

Add `pub mod guards;` to `backend/crates/sober-api/src/lib.rs` alongside the existing module declarations.

- [ ] **Step 4: Verify compilation**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo check -p sober-api -q`

Check that the `ConversationUser` struct has the fields used in tests (`unread_count`, `last_read_message_id`). If the struct shape differs, adjust the `membership()` helper.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-api/src/guards.rs backend/crates/sober-api/src/lib.rs
git commit -m "feat(api): add centralized authorization guards module"
```

---

### Task 2: Migrate ConversationService to Guards

**Files:**
- Modify: `backend/crates/sober-api/src/services/conversation.rs`

- [ ] **Step 1: Add guards import**

At the top of `backend/crates/sober-api/src/services/conversation.rs`, add:

```rust
use crate::guards;
```

- [ ] **Step 2: Replace inline check in `delete()` (line 243-244)**

Replace:
```rust
if membership.role != ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

With:
```rust
guards::require_owner(&membership)?;
```

- [ ] **Step 3: Add guard to `update_settings()` (after line 279)**

Currently `update_settings` only calls `verify_membership` without a role check. Add a guard after the membership check:

```rust
let membership = super::verify_membership(&self.db, conversation_id, user_id).await?;
guards::require_conversation_role(&membership, ConversationUserRole::Admin)?;
```

This requires admin or owner to modify settings, matching the frontend `canEditAgentMode` check.

- [ ] **Step 4: Replace inline check in `convert_to_group()` (line 403-404)**

Replace:
```rust
if membership.role != ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

With:
```rust
guards::require_owner(&membership)?;
```

- [ ] **Step 5: Replace inline check in `clear_messages()` (line 443-444)**

Replace:
```rust
if membership.role != ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

With:
```rust
guards::require_owner(&membership)?;
```

- [ ] **Step 6: Remove unused import**

After replacing all inline checks, `ConversationUserRole` may no longer be directly used in this file for comparisons. Check if it's still needed (it is — `ConversationKind::Direct` check in `convert_to_group` uses other enums from the same import). Keep the import but remove `AppError` from direct use if it's only used through guards now. Verify with `cargo check`.

- [ ] **Step 7: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test --workspace -q`

- [ ] **Step 8: Commit**

```bash
git add backend/crates/sober-api/src/services/conversation.rs
git commit -m "refactor(api): use guards in ConversationService"
```

---

### Task 3: Migrate MessageService to Guards

**Files:**
- Modify: `backend/crates/sober-api/src/services/message.rs`

- [ ] **Step 1: Add guards import**

```rust
use crate::guards;
```

- [ ] **Step 2: Replace inline check in `delete()` (lines 137-140)**

Replace:
```rust
let is_owner = membership.role == ConversationUserRole::Owner;
let is_sender = msg.user_id == Some(user_id);
if !is_owner && !is_sender {
    return Err(AppError::NotFound("message not found".into()));
}
```

With:
```rust
guards::require_owner_or_sender(&membership, msg.user_id, user_id)?;
```

Note: The original code returns `NotFound` instead of `Forbidden` (to avoid leaking message existence). The guard returns `Forbidden`. This is a deliberate change — the caller is already a conversation member (verified above), so revealing the message exists is not a security concern. If the original behavior is preferred, keep the inline check instead.

- [ ] **Step 3: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-api -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/message.rs
git commit -m "refactor(api): use guards in MessageService"
```

---

### Task 4: Migrate CollaboratorService to Guards

**Files:**
- Modify: `backend/crates/sober-api/src/services/collaborator.rs`

- [ ] **Step 1: Add guards import and replace inline checks**

Add at top:
```rust
use crate::guards;
```

In `add()` (lines 49-53), replace:
```rust
if caller_cu.role != ConversationUserRole::Owner
    && caller_cu.role != ConversationUserRole::Admin
{
    return Err(AppError::Forbidden);
}
```

With:
```rust
guards::require_conversation_role(&caller_cu, ConversationUserRole::Admin)?;
```

In `update_role()` (lines 143-146), replace:
```rust
let caller_cu = cu_repo.get(conversation_id, caller_user_id).await?;
if caller_cu.role != ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

With:
```rust
let caller_cu = cu_repo.get(conversation_id, caller_user_id).await?;
guards::require_owner(&caller_cu)?;
```

In `remove()` (line 215), replace:
```rust
check_can_remove(caller_cu.role, target_cu.role)?;
```

With:
```rust
guards::check_can_remove(caller_cu.role, target_cu.role)?;
```

In `leave()` (lines 279-281), replace:
```rust
let caller_cu = cu_repo.get(conversation_id, user_id).await?;
if caller_cu.role == ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

With:
```rust
let caller_cu = cu_repo.get(conversation_id, user_id).await?;
if caller_cu.role == ConversationUserRole::Owner {
    return Err(AppError::Forbidden);
}
```

Note: `leave()` is the inverse — owners **cannot** leave. This doesn't map to a standard guard, so keep the inline check as-is.

- [ ] **Step 2: Remove the local `check_can_remove` function (lines 356-374)**

Delete the `check_can_remove` function and its tests (lines 356-428) from `collaborator.rs`. The logic now lives in `guards::check_can_remove`. The tests in `guards.rs` (Task 1) already cover all the same cases.

- [ ] **Step 3: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test --workspace -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/collaborator.rs
git commit -m "refactor(api): use guards in CollaboratorService"
```

---

### Task 5: Add Plugin Authorization — Proto + DTO Changes

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto` (line 217)
- Modify: `backend/crates/sober-api/src/services/plugin.rs` (lines 12-22, 24-40)

- [ ] **Step 1: Add `owner_id` to proto `PluginInfo` message**

In `backend/proto/sober/agent/v1/agent.proto`, after line 217 (`string scope = 9;`), add:

```protobuf
  optional string owner_id = 10;
```

- [ ] **Step 2: Add `owner_id` and `scope` fields to API `PluginInfo` DTO**

In `backend/crates/sober-api/src/services/plugin.rs`, update the `PluginInfo` struct (lines 12-22):

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
    pub owner_id: Option<String>,
    pub config: serde_json::Value,
    pub installed_at: String,
}
```

Update the `From<proto::PluginInfo>` impl to map `owner_id`:

```rust
impl From<proto::PluginInfo> for PluginInfo {
    fn from(info: proto::PluginInfo) -> Self {
        let config: serde_json::Value =
            serde_json::from_str(&info.config).unwrap_or(serde_json::json!({}));
        Self {
            id: info.id,
            name: info.name,
            kind: info.kind,
            version: info.version,
            description: info.description,
            status: info.status,
            scope: info.scope,
            owner_id: info.owner_id,
            config,
            installed_at: info.installed_at,
        }
    }
}
```

- [ ] **Step 3: Update agent-side gRPC to populate `owner_id`**

In `backend/crates/sober-agent/src/grpc/plugins.rs`, the `plugin_to_proto` function (line 41) builds the proto message. Add `owner_id` to the struct literal:

```rust
pub(crate) fn plugin_to_proto(plugin: &sober_core::types::Plugin) -> proto::PluginInfo {
    proto::PluginInfo {
        id: plugin.id.to_string(),
        name: plugin.name.clone(),
        kind: format!("{:?}", plugin.kind).to_lowercase(),
        version: plugin.version.clone().unwrap_or_default(),
        description: plugin.description.clone().unwrap_or_default(),
        status: format!("{:?}", plugin.status).to_lowercase(),
        config: plugin.config.to_string(),
        installed_at: plugin.installed_at.to_rfc3339(),
        scope: format!("{:?}", plugin.scope).to_lowercase(),
        owner_id: plugin.owner_id.map(|id| id.to_string()),
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo check --workspace -q`

- [ ] **Step 5: Commit**

```bash
git add backend/proto/ backend/crates/sober-api/src/services/plugin.rs backend/crates/sober-agent/
git commit -m "feat(api): add owner_id to PluginInfo proto and DTO"
```

---

### Task 6: Add Plugin Authorization — Service Guards

**Files:**
- Modify: `backend/crates/sober-api/src/services/plugin.rs`
- Modify: `backend/crates/sober-api/src/routes/plugins.rs`

- [ ] **Step 1: Add `AuthUser` parameter to plugin service methods**

The plugin service methods currently don't receive user context. Update `PluginService` methods that need authorization to accept `user: &AuthUser`:

In `backend/crates/sober-api/src/services/plugin.rs`, update method signatures:

```rust
pub async fn update(
    &self,
    id: uuid::Uuid,
    user: &AuthUser,
    enabled: Option<bool>,
    config: Option<serde_json::Value>,
    scope: Option<String>,
) -> Result<PluginInfo, AppError> {
```

```rust
pub async fn uninstall(&self, id: uuid::Uuid, user: &AuthUser) -> Result<(), AppError> {
```

```rust
pub async fn reload(&self, user: &AuthUser) -> Result<ReloadResult, AppError> {
```

Add the import at the top:
```rust
use sober_auth::AuthUser;
use crate::guards;
```

- [ ] **Step 2: Add guard checks in service methods**

In `update()`, after the existing `let plugin_id = PluginId::from_uuid(id);` line, add a DB lookup + guard:

```rust
let plugin_id = PluginId::from_uuid(id);
let repo = PgPluginRepo::new(self.db.clone());
let plugin = repo.get_by_id(plugin_id).await?;
guards::can_modify_plugin(user, &plugin)?;
```

In `uninstall()`, before the gRPC call, add:

```rust
let plugin_id = PluginId::from_uuid(id);
let repo = PgPluginRepo::new(self.db.clone());
let plugin = repo.get_by_id(plugin_id).await?;
guards::can_modify_plugin(user, &plugin)?;
```

In `reload()`, add at the start:

```rust
guards::require_admin(user)?;
```

- [ ] **Step 3: Update route handlers to pass `AuthUser`**

In `backend/crates/sober-api/src/routes/plugins.rs`, update handlers that call the modified service methods:

`update_plugin` (line 123): change `_auth_user: AuthUser` to `auth_user: AuthUser` and pass `&auth_user` to the service call.

`uninstall_plugin` (line 136): same — use `auth_user` and pass to service.

`reload_plugins` (line 96): same — use `auth_user` and pass to service.

Example for `update_plugin`:
```rust
async fn update_plugin(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdatePluginBody>,
) -> Result<ApiResponse<PluginInfo>, AppError> {
    let plugin = state
        .plugin
        .update(id, &auth_user, body.enabled, body.config, body.scope)
        .await?;
    Ok(ApiResponse::new(plugin))
}
```

- [ ] **Step 4: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test --workspace -q`

- [ ] **Step 5: Run clippy**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo clippy -q -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-api/src/services/plugin.rs backend/crates/sober-api/src/routes/plugins.rs
git commit -m "feat(api): enforce plugin authorization guards"
```

---

### Task 7: Frontend Guards Module

**Files:**
- Create: `frontend/src/lib/guards/index.ts`

- [ ] **Step 1: Create the guards module**

```typescript
import { auth } from '$lib/stores/auth.svelte';
import type { ConversationUserRole } from '$lib/types';
import type { Plugin } from '$lib/types/plugin';

const ROLE_HIERARCHY: Record<ConversationUserRole, number> = {
	owner: 3,
	admin: 2,
	member: 1
};

// --- System-level guards ---

/** Returns true if the current user holds the given system role. */
export function hasRole(role: string): boolean {
	return auth.user?.roles?.includes(role) ?? false;
}

/** Returns true if the current user is a system admin. */
export function isAdmin(): boolean {
	return hasRole('admin');
}

// --- Conversation-level guards ---

/** Returns true if the user's role meets or exceeds the minimum. */
export function hasConversationRole(
	userRole: ConversationUserRole | undefined,
	minimum: ConversationUserRole
): boolean {
	return (ROLE_HIERARCHY[userRole ?? 'member'] ?? 0) >= ROLE_HIERARCHY[minimum];
}

/** Returns true if the user can manage conversation settings (admin+). */
export function canManageConversation(role?: ConversationUserRole): boolean {
	return hasConversationRole(role, 'admin');
}

/** Returns true if the user can delete a conversation (owner only). */
export function canDeleteConversation(role?: ConversationUserRole): boolean {
	return hasConversationRole(role, 'owner');
}

// --- Plugin-level guards ---

/** Returns true if the user can modify/delete this plugin. */
export function canModifyPlugin(plugin: Plugin): boolean {
	if (plugin.scope === 'system') return isAdmin();
	if (plugin.scope === 'user') return plugin.owner_id === auth.user?.id;
	// workspace: owner or system admin
	return plugin.owner_id === auth.user?.id || isAdmin();
}
```

- [ ] **Step 2: Run frontend checks**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check`

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/guards/index.ts
git commit -m "feat(frontend): add reusable authorization guards module"
```

---

### Task 8: Frontend Guard Components

**Files:**
- Create: `frontend/src/lib/components/guards/RequireRole.svelte`
- Create: `frontend/src/lib/components/guards/RequireConversationRole.svelte`

- [ ] **Step 1: Create `RequireRole.svelte`**

```svelte
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { hasRole } from '$lib/guards';

	interface Props {
		role: string;
		children: Snippet;
		fallback?: Snippet;
	}

	let { role, children, fallback }: Props = $props();
</script>

{#if hasRole(role)}
	{@render children()}
{:else if fallback}
	{@render fallback()}
{/if}
```

- [ ] **Step 2: Create `RequireConversationRole.svelte`**

```svelte
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ConversationUserRole } from '$lib/types';
	import { hasConversationRole } from '$lib/guards';

	interface Props {
		userRole: ConversationUserRole | undefined;
		minimum: ConversationUserRole;
		children: Snippet;
		fallback?: Snippet;
	}

	let { userRole, minimum, children, fallback }: Props = $props();
</script>

{#if hasConversationRole(userRole, minimum)}
	{@render children()}
{:else if fallback}
	{@render fallback()}
{/if}
```

- [ ] **Step 3: Run frontend checks**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check`

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/components/guards/
git commit -m "feat(frontend): add RequireRole and RequireConversationRole components"
```

---

### Task 9: Frontend Plugin Type Update

**Files:**
- Modify: `frontend/src/lib/types/plugin.ts` (line 7)

- [ ] **Step 1: Add `owner_id` to the `Plugin` interface**

In `frontend/src/lib/types/plugin.ts`, add `owner_id` after `scope`:

```typescript
export interface Plugin {
	id: string;
	name: string;
	kind: PluginKind;
	version: string;
	description: string;
	status: PluginStatus;
	scope: PluginScope;
	owner_id: string | null;
	config: Record<string, unknown>;
	installed_at: string;
}
```

- [ ] **Step 2: Run frontend checks**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check`

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/types/plugin.ts
git commit -m "feat(frontend): add owner_id to Plugin type"
```

---

### Task 10: Migrate Frontend Settings Layout to Guards

**Files:**
- Modify: `frontend/src/routes/(app)/settings/+layout.svelte`

- [ ] **Step 1: Replace inline admin check**

In `frontend/src/routes/(app)/settings/+layout.svelte`, replace the import and derived:

Replace line 5 (`import { auth } from '$lib/stores/auth.svelte';`) with:
```typescript
import { isAdmin } from '$lib/guards';
```

Replace line 14 (`const isAdmin = $derived(auth.user?.roles?.includes('admin') ?? false);`) with:
```typescript
const showAdmin = $derived(isAdmin());
```

Update the template on line 26 from `{#if isAdmin}` to `{#if showAdmin}`.

- [ ] **Step 2: Add evolution route guard**

Create `frontend/src/routes/(app)/settings/evolution/+layout.ts`:

```typescript
import { redirect } from '@sveltejs/kit';
import { isAdmin } from '$lib/guards';

export async function load({ parent }: { parent: () => Promise<void> }) {
	await parent();
	if (!isAdmin()) {
		redirect(302, '/settings');
	}
}
```

- [ ] **Step 3: Run frontend checks**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check`

- [ ] **Step 4: Commit**

```bash
git add frontend/src/routes/\(app\)/settings/
git commit -m "refactor(frontend): use guards in settings layout"
```

---

### Task 11: Migrate Frontend Conversation Components to Guards

**Files:**
- Modify: `frontend/src/lib/components/ConversationSettings.svelte`
- Modify: `frontend/src/lib/components/CollaboratorList.svelte`

- [ ] **Step 1: Update ConversationSettings**

In `frontend/src/lib/components/ConversationSettings.svelte`, add import:

```typescript
import { canManageConversation } from '$lib/guards';
```

Replace the inline derived (line 173):
```typescript
let canEditAgentMode = $derived(currentUserRole === 'owner' || currentUserRole === 'admin');
```

With:
```typescript
let canEditAgentMode = $derived(canManageConversation(currentUserRole));
```

- [ ] **Step 2: Update CollaboratorList**

In `frontend/src/lib/components/CollaboratorList.svelte`, add import:

```typescript
import { canManageConversation, hasConversationRole } from '$lib/guards';
```

Replace line 27:
```typescript
let canManage = $derived(currentUserRole === 'owner' || currentUserRole === 'admin');
```

With:
```typescript
let canManage = $derived(canManageConversation(currentUserRole));
```

The `canKick` and `canChangeRole` functions (lines 40-51) encode specific multi-target logic (comparing caller vs target roles). These are not simple role checks — they involve two roles. Keep them inline since they're component-specific logic that doesn't generalize cleanly.

- [ ] **Step 3: Run frontend checks and tests**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check && pnpm test --silent`

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/components/ConversationSettings.svelte frontend/src/lib/components/CollaboratorList.svelte
git commit -m "refactor(frontend): use guards in conversation components"
```

---

### Task 12: Final Verification and Version Bump

**Files:**
- Modify: All affected `Cargo.toml` files (version bump)

- [ ] **Step 1: Full backend verification**

Run all three in sequence:
```bash
cd /home/harri/Projects/Repos/sober/backend && cargo fmt --check -q && cargo clippy -q -- -D warnings && cargo test --workspace -q
```

- [ ] **Step 2: Full frontend verification**

```bash
cd /home/harri/Projects/Repos/sober/frontend && pnpm check && pnpm test --silent
```

- [ ] **Step 3: Version bump**

This is a `feat/` branch — bump **minor** version. Update the version in all affected crate `Cargo.toml` files:
- `backend/crates/sober-api/Cargo.toml`
- `backend/crates/sober-core/Cargo.toml` (if proto changes affect it)
- Any other crates with changes

- [ ] **Step 4: Move plan to active**

```bash
git mv docs/plans/pending/050-authorization-guards docs/plans/active/050-authorization-guards
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: bump versions for #050 authorization guards"
```
