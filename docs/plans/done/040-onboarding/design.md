# 040: Onboarding — First-User Auto-Admin

## Problem

A fresh Sober instance requires manual CLI commands to create and approve the first admin user before the web UI is usable. This is friction-heavy and undiscoverable for new users.

## Goal

When the first user registers via the web UI, they are automatically assigned the admin role with Active status. The register page shows a welcome message when no users exist. No new pages or routes beyond a simple system status endpoint.

## Design

### Backend

**New repo method: `has_users()`**

`UserRepo` gains a single new method that returns `true` if any user exists. Implementation: `SELECT EXISTS(SELECT 1 FROM users)`.

**Modified registration logic**

`AuthService::register()` keeps its existing signature (`Result<User, AppError>`). Before creating the user, it checks `has_users()`:

- No users exist: creates with roles `[Admin, User]`, then sets status to `Active`
- Users exist: creates with role `[User]` and `Pending` status (unchanged)

Uses existing `create_with_roles()` and `update_status()` methods. No new types or enums.

**New endpoint: `GET /api/v1/system/status`**

Unauthenticated endpoint returning whether the instance has been initialized:

```json
{ "data": { "initialized": false } }
```

### Frontend

**Register page (`/register`)**

On mount, fetches `/api/v1/system/status`. When `initialized` is `false`:
- Shows welcome banner: "Welcome to Sober! Create your admin account to get started."
- Button text changes to "Create Admin Account"

After registration:
- If user status is `Active`: shows "Account created! You can now sign in." with link to `/login`
- If user status is `Pending`: shows existing "pending approval" message (unchanged)

No new routes or pages.

## What does NOT change

- Registration endpoint response shape (still `{ id, email, username, status }`)
- Auth middleware, session handling, login flow
- CLI user management commands
- Database schema (no new tables or migrations)
