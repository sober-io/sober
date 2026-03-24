# 040: Onboarding — Implementation Plan

## Step 1: Add `has_users()` to `UserRepo`

**File:** `backend/crates/sober-core/src/types/repo.rs` — add to trait (after `search_by_username`):
```rust
fn has_users(&self) -> impl Future<Output = Result<bool, AppError>> + Send;
```

**File:** `backend/crates/sober-db/src/repos/users.rs` — implement:
```rust
async fn has_users(&self) -> Result<bool, AppError> {
    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users)")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    Ok(exists)
}
```

## Step 2: Modify `AuthService::register()` — first user gets admin + Active

**File:** `backend/crates/sober-auth/src/service.rs`

Same signature (`Result<User, AppError>`), just change the logic:

```rust
// After password hash...
let has_users = self.users.has_users().await?;
let roles = if has_users {
    vec![RoleKind::User]
} else {
    vec![RoleKind::Admin, RoleKind::User]
};
let user = self.users.create_with_roles(input, &roles).await?;
if !has_users {
    self.users.update_status(user.id, UserStatus::Active).await?;
}
```

No new types, no return type change. First user comes back with `Active` status, rest with `Pending`.

## Step 3: Add `GET /api/v1/system/status`

**New file:** `backend/crates/sober-api/src/routes/system.rs`

Unauthenticated endpoint (no `AuthUser` extractor, same pattern as `health.rs`):
```json
{ "data": { "initialized": true|false } }
```

Just calls `PgUserRepo::has_users()`.

**File:** `backend/crates/sober-api/src/routes/mod.rs` — add `pub mod system;` + `.merge(system::routes())`

## Step 4: Frontend — show welcome text on register page

**New file:** `frontend/src/lib/services/system.ts` — fetch `/system/status`

**File:** `frontend/src/routes/(public)/register/+page.svelte`
- On mount, fetch system status
- If not initialized: show welcome text ("Welcome to Sober! Create your admin account to get started.") and change button to "Create Admin Account"
- On successful registration: if user status is `Active` → show "Account created! You can now sign in." with link to `/login`. If `Pending` → show existing "pending approval" message.

## Step 5: Tests

### Backend integration tests (`backend/crates/sober-api/tests/http.rs`)

**Update existing:** `register_creates_pending_user` — pre-seed a user via `register_and_approve` before the test register call so it tests the normal (second user) path.

**New tests:**

- `system_status_uninitialized` — `GET /api/v1/system/status` on empty DB → 200, `data.initialized == false`
- `system_status_initialized_after_register` — Register a user, then `GET /system/status` → `data.initialized == true`
- `first_user_register_gets_admin_and_active` — `POST /auth/register` on empty DB → 200, `data.status == "Active"`. Verify the user can login. Verify admin role via admin-only endpoint.
- `second_user_register_gets_pending` — Pre-seed a user. Register second user → 200, `data.status == "Pending"`. Verify cannot login.

### Backend unit tests (`backend/crates/sober-db/src/repos/users.rs`)

- `has_users_returns_false_on_empty_db`
- `has_users_returns_true_after_user_created`

### Frontend component tests (`frontend/src/routes/(public)/register/register.test.ts`)

Uses vitest + @testing-library/svelte:

- `shows welcome text when system is not initialized` — mock `/system/status` → `{ initialized: false }`, assert welcome banner + "Create Admin Account" button
- `shows normal form when system is initialized` — mock `/system/status` → `{ initialized: true }`, assert no banner + "Create account" button
- `shows sign-in link after first user registration` — mock register → `{ status: "Active" }`, assert "You can now sign in" + link to `/login`
- `shows pending message after normal registration` — mock register → `{ status: "Pending" }`, assert "pending approval" message

## Step 6: `cargo sqlx prepare`

New `SELECT EXISTS` query needs offline data.

## Verification

```bash
cd backend && cargo build -q && cargo clippy -q -- -D warnings && cargo test --workspace -q
cd frontend && pnpm check && pnpm test --silent
```

## Files

| File | Change |
|------|--------|
| `backend/crates/sober-core/src/types/repo.rs` | Add `has_users()` to `UserRepo` |
| `backend/crates/sober-db/src/repos/users.rs` | Implement `has_users()` + tests |
| `backend/crates/sober-auth/src/service.rs` | First-user conditional in `register()` |
| `backend/crates/sober-api/src/routes/system.rs` | **New** — `/system/status` endpoint |
| `backend/crates/sober-api/src/routes/mod.rs` | Register system module |
| `backend/crates/sober-api/tests/http.rs` | Update + add tests |
| `frontend/src/lib/services/system.ts` | **New** — system status fetch |
| `frontend/src/routes/(public)/register/+page.svelte` | Welcome text + active-user handling |
| `frontend/src/routes/(public)/register/register.test.ts` | **New** — component tests |
