# #050: Authorization Guards — Centralized ACL/RBAC Enforcement

## Problem

Authorization checks are scattered across service methods as ad-hoc inline
conditionals. Each service re-implements role/ownership checks differently,
making it hard to audit, easy to miss, and inconsistent in error responses.
The frontend mirrors this problem — role checks are repeated inline in every
component that needs them.

Plugin endpoints have **zero authorization checks** despite the data model
supporting scope (system/user/workspace) and ownership.

## Goals

1. Centralized, reusable guard functions for backend authorization.
2. Layered defense: coarse route-level extractors + fine-grained service-level guards.
3. Reusable frontend guards: functions for logic, wrapper components for rendering.
4. Consistent error responses across all authorization failures.
5. Plugin endpoints enforce scope-based ownership rules.

## Non-Goals

- Fine-grained ABAC / per-resource permissions (the `Permission` enum stays as a stub).
- New database tables or schema changes.
- Changes to the authentication flow (session tokens, middleware).

---

## Design

### Backend: Two-Layer Authorization

#### Layer 1 — Extractors (unchanged)

Existing extractors continue to handle coarse route-level gating:

- **`AuthUser`** — ensures request is authenticated. Returns 401 if not.
- **`RequireAdmin`** — wraps `AuthUser`, additionally checks for admin role.
  Returns 403 if user lacks admin role.

These remain on route definitions. No changes needed.

#### Layer 2 — Guard Functions (new)

A `guards` module in `sober-api` provides composable check functions that
services call after fetching the relevant context (membership, plugin, etc.).

**Location:** `backend/crates/sober-api/src/guards.rs`

##### System-Level Guards

```rust
pub fn require_admin(user: &AuthUser) -> Result<(), AppError>;
pub fn require_active(user: &AuthUser) -> Result<(), AppError>;
```

##### Conversation-Level Guards

```rust
pub fn require_membership(
    membership: &ConversationMembership,
) -> Result<(), AppError>;

pub fn require_conversation_role(
    membership: &ConversationMembership,
    minimum: ConversationUserRole,
) -> Result<(), AppError>;

pub fn require_owner(
    membership: &ConversationMembership,
) -> Result<(), AppError>;

pub fn require_owner_or_sender(
    membership: &ConversationMembership,
    sender_id: UserId,
    acting_user_id: UserId,
) -> Result<(), AppError>;
```

`ConversationUserRole` has a natural hierarchy: `Owner > Admin > Member`.
`require_conversation_role` checks `>=` against the minimum.

##### Plugin-Level Guards

```rust
pub fn can_view_plugin(
    user: &AuthUser,
    plugin_scope: PluginScope,
    plugin_owner_id: Option<UserId>,
) -> Result<(), AppError>;

pub fn can_modify_plugin(
    user: &AuthUser,
    plugin_scope: PluginScope,
    plugin_owner_id: Option<UserId>,
) -> Result<(), AppError>;
```

Plugin authorization rules:

| Operation | System plugin | User plugin | Workspace plugin |
|-----------|--------------|-------------|-----------------|
| View/list | Any authenticated | Owner + admin | Owner + admin |
| Modify    | Admin only        | Owner only   | Owner or system admin |
| Delete    | Admin only        | Owner only   | Owner or system admin |
| Install   | Admin only        | Any user (own scope) | Any user (own workspace) |

"Admin" in this table always means system-level admin role, not conversation admin.

##### Error Format

All guards return `AppError::Forbidden` with a consistent message:
- `"admin role required"`
- `"conversation owner role required"`
- `"conversation admin role required"`
- `"not authorized to modify this plugin"`

#### Service Integration Pattern

Services separate "fetch context" from "check permission":

```rust
// Before (ad-hoc)
let membership = verify_membership(&self.db, conv_id, user_id).await?;
if membership.role != ConversationUserRole::Owner {
    return Err(AppError::Forbidden("only owner can delete".into()));
}

// After (guard function)
let membership = verify_membership(&self.db, conv_id, user_id).await?;
guards::require_owner(&membership)?;
```

### Backend Refactoring Targets

#### ConversationService
- `delete()` → `require_owner`
- `update_settings()` → `require_conversation_role(Admin)`
- `convert_to_group()` → `require_owner`
- `clear_messages()` → `require_owner`

#### MessageService
- `delete()` → `require_owner_or_sender`

#### CollaboratorService
- `add()` → `require_conversation_role(Admin)`
- `remove()` → `require_owner` (or admin removing member)
- `change_role()` → `require_owner`

#### AuthService (admin operations)
- `approve_user()` → `guards::require_admin` in service (defense-in-depth)
- `disable_user()` → `guards::require_admin` in service

#### PluginService
- `list()` → filter results by `can_view_plugin` per item
- `install()` → validate scope matches user's authority
- `update()` → `can_modify_plugin` after fetching plugin
- `uninstall()` → `can_modify_plugin` after fetching plugin
- `import()` → validate each plugin's scope
- `reload()` → `require_admin` (system-wide operation)

#### EvolutionService
- Already behind `RequireAdmin` extractor. Add `guards::require_admin`
  in service methods for defense-in-depth.

---

### Frontend: Guards Module + Components

#### Guard Functions (`$lib/guards/index.ts`)

Reusable functions that encapsulate role-checking logic:

```typescript
// System-level
function hasRole(role: string): boolean;
function isAdmin(): boolean;

// Conversation-level
function hasConversationRole(
  userRole: ConversationUserRole | undefined,
  minimum: ConversationUserRole
): boolean;
function canManageConversation(role?: ConversationUserRole): boolean;
function canDeleteConversation(role?: ConversationUserRole): boolean;

// Plugin-level
function canModifyPlugin(plugin: PluginInfo): boolean;
function canDeletePlugin(plugin: PluginInfo): boolean;
```

Conversation role hierarchy mirrors backend: `owner > admin > member`.

#### Guard Components (`$lib/components/guards/`)

Wrapper components for declarative conditional rendering:

**`RequireRole.svelte`** — renders children if user has the specified system role.
Accepts optional `fallback` snippet.

```svelte
<RequireRole role="admin">
  {#snippet children()}
    <AdminPanel />
  {/snippet}
</RequireRole>
```

**`RequireConversationRole.svelte`** — renders children if user has at least
the specified conversation role. Accepts `userRole` and `minimum` props.

```svelte
<RequireConversationRole userRole={currentRole} minimum="admin">
  {#snippet children()}
    <ManageMembers />
  {/snippet}
</RequireConversationRole>
```

Both components accept an optional `fallback` snippet for rendering
alternative content when the check fails.

#### Route-Level Guards

Keep existing `+layout.ts` redirect for authenticated routes. Add admin
route guards for admin-only pages:

```typescript
// routes/(app)/settings/evolution/+layout.ts
export async function load({ parent }) {
  await parent();
  if (!isAdmin()) {
    redirect(302, '/settings');
  }
}
```

### Frontend Refactoring Targets

1. **Settings layout** — replace `auth.user?.roles?.includes('admin')` with `isAdmin()`
2. **ConversationSettings** — replace inline role checks with `canManageConversation(role)`
3. **CollaboratorList** — replace `canKick`/`canChangeRole` with guard functions
4. **Evolution settings route** — add `+layout.ts` admin redirect guard
5. **Plugin UI** — gate modify/delete actions behind `canModifyPlugin(plugin)`

---

## Testing

### Backend
- Unit tests for each guard function (all role combinations).
- Integration tests for service methods verifying 403 on unauthorized access.
- Plugin service tests: system plugin modification by non-admin returns 403,
  user plugin modification by different user returns 403.

### Frontend
- Unit tests for guard functions (role hierarchy, edge cases).
- Component tests for `RequireRole` and `RequireConversationRole` rendering.

## Migration

This is a pure refactoring with one behavioral change (plugin authorization
enforcement). No database migrations. No API contract changes beyond new 403
responses on previously-unguarded plugin endpoints.
