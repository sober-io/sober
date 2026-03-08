# 005 --- sober-db

**Date:** 2026-03-07
**Status:** Pending
**Crate:** `sober-db`

---

## Overview

`sober-db` is the centralized PostgreSQL access layer. It owns pool creation, private
row types, and all concrete repository implementations. Library crates depend only on
repo traits defined in `sober-core`; `sober-db` provides the PostgreSQL implementations
that binaries wire in at startup.

This crate exists to:

1. **Contain `sqlx`** --- library crates (`sober-auth`, `sober-memory`, `sober-agent`, etc.)
   have no `sqlx` dependency. They program against traits from `sober-core`.
2. **Eliminate duplicate queries** --- each table's queries live in one place.
3. **Ensure consistent pool configuration** --- a single `create_pool()` function used by
   all binaries.

---

## Design Decisions

### Pool Creation

A single public function constructs the pool with consistent settings:

```rust
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.url)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}
```

Each binary calls this once at startup. `sober-cli` (`sober` binary) overrides with
`max_connections(1)` via a separate helper or by constructing the pool directly.

### Row Types (Private)

Row types are `sqlx::FromRow` structs that map directly to database columns. They are
**private** to `sober-db` --- consumers never see them. Each row type has a `From<Row>`
implementation that converts to the corresponding domain type from `sober-core`.

```rust
// Private to sober-db
#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    username: String,
    password_hash: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: UserId::from_uuid(row.id),
            email: row.email,
            username: row.username,
            status: row.status.parse().expect("invalid status in DB"),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
```

Note: `password_hash` is present in `UserRow` but absent from the `User` domain type.
Fields that are internal to a specific operation (like password verification) are
accessed via dedicated repo methods that return purpose-specific types.

### Repository Implementations

Each repo is a struct holding a `PgPool`, implementing a trait from `sober-core`:

```rust
pub struct PgUserRepo {
    pool: PgPool,
}

impl PgUserRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepo for PgUserRepo {
    async fn get_by_id(&self, id: UserId) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("user".into()))?;
        Ok(row.into())
    }

    // ...
}
```

### Transaction Handling

Compound operations that span multiple tables get their own repo method. The
transaction is internal to `sober-db` --- traits do not expose transaction types.

Example: `UserRepo::create_with_roles` inserts a user and assigns one or more roles
in a single transaction.

```rust
async fn create_with_roles(&self, input: CreateUser, roles: &[RoleKind]) -> Result<User, AppError> {
    let mut tx = self.pool.begin().await.map_err(|e| AppError::Internal(e.into()))?;

    let user_row = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (id, email, username, password_hash, status) \
         VALUES ($1, $2, $3, $4, $5) RETURNING *"
    )
    .bind(UserId::new())
    .bind(&input.email)
    .bind(&input.username)
    .bind(&input.password_hash)
    .bind(UserStatus::Pending.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    for role in roles {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) \
             SELECT $1, id FROM roles WHERE name = $2"
        )
        .bind(user_row.id)
        .bind(role.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;
    Ok(user_row.into())
}
```

### Wiring at Startup (Binary Responsibility)

Binaries construct concrete repos and pass them as generic type parameters.
Repo traits use RPITIT (`-> impl Future<...> + Send`), which makes them **not
dyn-compatible** — `Arc<dyn Repo>` won't compile. Generics are used instead:

```rust
// In sober-api main()
let pool = sober_db::create_pool(&config.database).await?;
let user_repo = PgUserRepo::new(pool.clone());
let session_repo = PgSessionRepo::new(pool.clone());
let role_repo = PgRoleRepo::new(pool.clone());
// ... pass as type parameters to library crates
```

Library crates use generic type parameters in their constructors:

```rust
// In sober-auth
pub struct AuthService<U: UserRepo, S: SessionRepo, R: RoleRepo> {
    users: U,
    sessions: S,
    roles: R,
    session_ttl_seconds: u64,
}
```

---

## Repo Traits (defined in sober-core)

The following traits are defined in `sober-core` and implemented in `sober-db`:

### UserRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait UserRepo: Send + Sync {
    async fn get_by_id(&self, id: UserId) -> Result<User, AppError>;
    async fn get_by_email(&self, email: &str) -> Result<User, AppError>;
    async fn create(&self, input: CreateUser) -> Result<User, AppError>;
    async fn create_with_roles(&self, input: CreateUser, roles: &[RoleKind]) -> Result<User, AppError>;
    async fn update_status(&self, id: UserId, status: UserStatus) -> Result<(), AppError>;
}
```

### SessionRepo

Replaces `sober-auth`'s `SessionStore` trait.

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait SessionRepo: Send + Sync {
    async fn get(&self, token_hash: &[u8]) -> Result<Option<Session>, AppError>;
    async fn create(&self, session: &Session) -> Result<(), AppError>;
    async fn delete(&self, token_hash: &[u8]) -> Result<(), AppError>;
    async fn cleanup_expired(&self) -> Result<u64, AppError>;
}
```

### ConversationRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait ConversationRepo: Send + Sync {
    async fn create(&self, user_id: UserId, title: Option<&str>) -> Result<Conversation, AppError>;
    async fn get_by_id(&self, id: ConversationId) -> Result<Conversation, AppError>;
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Conversation>, AppError>;
    async fn update_title(&self, id: ConversationId, title: &str) -> Result<(), AppError>;
    async fn delete(&self, id: ConversationId) -> Result<(), AppError>;
}
```

### MessageRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait MessageRepo: Send + Sync {
    async fn create(&self, input: CreateMessage) -> Result<Message, AppError>;
    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
        limit: usize,
    ) -> Result<Vec<Message>, AppError>;
}
```

### JobRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait JobRepo: Send + Sync {
    async fn create(&self, input: CreateJob) -> Result<Job, AppError>;
    async fn get_by_id(&self, id: Uuid) -> Result<Job, AppError>;
    async fn list_active(&self) -> Result<Vec<Job>, AppError>;
    async fn update_next_run(&self, id: Uuid, next_run_at: DateTime<Utc>) -> Result<(), AppError>;
    async fn mark_last_run(&self, id: Uuid, ran_at: DateTime<Utc>) -> Result<(), AppError>;
    async fn cancel(&self, id: Uuid) -> Result<(), AppError>;
}
```

### McpServerRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait McpServerRepo: Send + Sync {
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<McpServerConfig>, AppError>;
    async fn create(&self, input: CreateMcpServer) -> Result<McpServerConfig, AppError>;
    async fn update(&self, id: McpServerId, input: UpdateMcpServer) -> Result<McpServerConfig, AppError>;
    async fn delete(&self, id: McpServerId) -> Result<(), AppError>;
}
```

### WorkspaceRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait WorkspaceRepo: Send + Sync {
    async fn create(&self, user_id: UserId, name: &str, root_path: &str) -> Result<Workspace, AppError>;
    async fn get_by_id(&self, id: WorkspaceId) -> Result<Workspace, AppError>;
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Workspace>, AppError>;
    async fn archive(&self, id: WorkspaceId) -> Result<(), AppError>;
    async fn restore(&self, id: WorkspaceId) -> Result<(), AppError>;
    async fn delete(&self, id: WorkspaceId) -> Result<(), AppError>;
}
```

### WorkspaceRepoRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait WorkspaceRepoRepo: Send + Sync {
    async fn register(&self, workspace_id: WorkspaceId, input: RegisterRepo) -> Result<WorkspaceRepo, AppError>;
    async fn list_by_workspace(&self, workspace_id: WorkspaceId) -> Result<Vec<WorkspaceRepo>, AppError>;
    async fn find_by_path(&self, path: &str, user_id: UserId) -> Result<Option<WorkspaceRepo>, AppError>;
    async fn delete(&self, id: WorkspaceRepoId) -> Result<(), AppError>;
}
```

### WorktreeRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait WorktreeRepo: Send + Sync {
    async fn create(&self, repo_id: WorkspaceRepoId, branch: &str, path: &str) -> Result<Worktree, AppError>;
    async fn list_by_repo(&self, repo_id: WorkspaceRepoId) -> Result<Vec<Worktree>, AppError>;
    async fn list_stale(&self, older_than: DateTime<Utc>) -> Result<Vec<Worktree>, AppError>;
    async fn mark_stale(&self, id: WorktreeId) -> Result<(), AppError>;
    async fn delete(&self, id: WorktreeId) -> Result<(), AppError>;
}
```

### ArtifactRepo

```rust
// Uses RPITIT — not dyn-compatible, consumers use generics
pub trait ArtifactRepo: Send + Sync {
    async fn create(&self, input: CreateArtifact) -> Result<Artifact, AppError>;
    async fn get_by_id(&self, id: ArtifactId) -> Result<Artifact, AppError>;
    async fn list_by_workspace(&self, workspace_id: WorkspaceId, filter: ArtifactFilter) -> Result<Vec<Artifact>, AppError>;
    async fn update_state(&self, id: ArtifactId, state: ArtifactState) -> Result<(), AppError>;
    async fn add_relation(&self, source: ArtifactId, target: ArtifactId, relation: ArtifactRelation) -> Result<(), AppError>;
}
```

---

## Impact on Other Crates

### sober-core (003)

- Add repo trait definitions (`UserRepo`, `SessionRepo`, etc.) and their input/domain types.
- Add domain types for entities (`User`, `Session`, `Conversation`, `Message`, `Job`, `McpServerConfig`).
- Remove `sqlx` as a dependency (keep only for type derives on ID newtypes and enums --- `sqlx::Type`).

### sober-auth (006)

- Remove `sqlx` dependency.
- Remove `SessionStore` trait and `PgSessionStore` --- replaced by `SessionRepo` in core / `PgSessionRepo` in db.
- Use generic type parameters (`<U: UserRepo, S: SessionRepo, R: RoleRepo>`) instead of `PgPool`.

### sober-memory (007)

- Remove `sqlx` dependency.
- `ContextLoader` takes generic `M: MessageRepo` instead of `&PgPool`.

### sober-agent (012)

- Remove `sqlx` dependency.
- `Agent` struct uses generic type parameters for repos (`MessageRepo`, `ConversationRepo`, `McpServerRepo`).

### sober-api (013)

- Add `sober-db` dependency.
- `AppState` holds `PgPool` (for pool lifecycle) plus constructed repos.
- Constructs `Pg*Repo` instances at startup and passes to library crates.

### sober-cli (014)

- `sober` binary depends on `sober-db` for pool creation and repo access.
- `soberctl` unchanged (talks via Unix sockets, no DB).

### sober-scheduler (016)

- Remove direct `sqlx` dependency for job queries.
- Use generic `J: JobRepo` instead of `PgPool` for job persistence.
- Binary startup creates `PgJobRepo` via `sober-db`.

---

## Dependency Flow

```
sober-api (binary)
  ├── sober-db ──► sober-core
  ├── sober-auth ──► sober-core
  └── sober-core

sober-agent (binary)
  ├── sober-db ──► sober-core
  ├── sober-mind ──► sober-core
  ├── sober-memory ──► sober-core
  └── sober-core

sober-scheduler (binary)
  ├── sober-db ──► sober-core
  └── sober-core

sober-cli (sober binary)
  ├── sober-db ──► sober-core
  └── sober-core
```

Library crates (`sober-auth`, `sober-memory`, `sober-mind`, etc.) depend only on
`sober-core` for traits. They never import `sqlx`.

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `sober-core` | Domain types, repo traits, config, errors |
| `sqlx` (with `postgres`, `runtime-tokio`, `tls-rustls`) | PostgreSQL driver |
| `async-trait` | Async trait implementations |
| `tracing` | Structured logging (query spans) |
| `uuid` | UUID handling for row conversions |
| `chrono` | Timestamp handling |

---

## Testing

- Use `#[sqlx::test]` for integration tests against a real PostgreSQL instance.
- Each repo gets a test module verifying CRUD operations.
- Tests run in transactions that roll back automatically.
- Unit tests for row-to-domain conversions (no DB required).

For unit testing library crates (e.g., `sober-auth` business logic), create mock
repo implementations in test modules:

```rust
#[cfg(test)]
struct MockUserRepo {
    users: Vec<User>,
}

#[async_trait]
impl UserRepo for MockUserRepo {
    async fn get_by_id(&self, id: UserId) -> Result<User, AppError> {
        self.users.iter()
            .find(|u| u.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("user".into()))
    }
    // ...
}
```
