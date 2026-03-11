# 017 --- Workspaces, Worktrees & Artifact Management: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add workspace, worktree, and artifact management --- the collaboration layer where users and the agent produce, version, and track work artifacts.

**Architecture:** New `sober-workspace` library crate for filesystem/git/blob business logic. Types, enums, and repo traits live in `sober-core`. PostgreSQL repo implementations (`PgWorkspaceRepo`, `PgArtifactRepo`, etc.) live in `sober-db`. Agent orchestration stays in `sober-agent`. This split allows `sober-cli` and `sober-scheduler` to use workspace logic without depending on the agent binary.

**Tech Stack:** Rust, tokio (async fs), serde/serde_json, toml (config parsing), sha2 (blob hashing), git2 (worktree management), thiserror

**Depends on:** 002 (project skeleton), 003 (sober-core types + config), 005 (sober-db repo pattern)

---

## Task 1: Add New Types to sober-core

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs`
- Modify: `backend/crates/sober-core/src/types/enums.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`
- Test: `backend/crates/sober-core/src/types/ids.rs` (inline tests)
- Test: `backend/crates/sober-core/src/types/enums.rs` (inline tests)

**Step 1: Write failing tests for new ID types**

Add to the existing test module in `ids.rs`:

```rust
#[test]
fn workspace_id_roundtrip() {
    let id = WorkspaceId::new();
    let s = id.to_string();
    let parsed: WorkspaceId = serde_json::from_str(&format!("\"{s}\"")).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn workspace_repo_id_roundtrip() {
    let id = WorkspaceRepoId::new();
    assert_ne!(id, WorkspaceRepoId::new());
}

#[test]
fn worktree_id_roundtrip() {
    let id = WorktreeId::new();
    let s = id.to_string();
    assert!(!s.is_empty());
}

#[test]
fn artifact_id_roundtrip() {
    let id = ArtifactId::new();
    let uuid = id.as_uuid();
    let reconstructed = ArtifactId::from_uuid(*uuid);
    assert_eq!(id, reconstructed);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-core -- workspace_id_roundtrip workspace_repo_id_roundtrip worktree_id_roundtrip artifact_id_roundtrip`
Expected: FAIL --- types not defined

**Step 3: Add ID types using the existing `define_id!` macro**

In `ids.rs`, add four invocations:

```rust
define_id!(WorkspaceId);
define_id!(WorkspaceRepoId);
define_id!(WorktreeId);
define_id!(ArtifactId);
```

Ensure these are re-exported in `types/mod.rs`.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-core -- workspace_id worktree_id artifact_id workspace_repo_id`
Expected: PASS

**Step 5: Write failing tests for new enums**

Add to the test module in `enums.rs`:

```rust
#[test]
fn workspace_state_serde_roundtrip() {
    let variants = [WorkspaceState::Active, WorkspaceState::Archived, WorkspaceState::Deleted];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: WorkspaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn worktree_state_serde_roundtrip() {
    let variants = [WorktreeState::Active, WorktreeState::Stale, WorktreeState::Removing];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: WorktreeState = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn artifact_kind_serde_roundtrip() {
    let variants = [
        ArtifactKind::CodeChange,
        ArtifactKind::Document,
        ArtifactKind::Proposal,
        ArtifactKind::Snapshot,
        ArtifactKind::Trace,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: ArtifactKind = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn artifact_state_serde_roundtrip() {
    let variants = [
        ArtifactState::Draft,
        ArtifactState::Proposed,
        ArtifactState::Approved,
        ArtifactState::Rejected,
        ArtifactState::Archived,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: ArtifactState = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn artifact_relation_serde_roundtrip() {
    let variants = [
        ArtifactRelation::SpawnedBy,
        ArtifactRelation::Supersedes,
        ArtifactRelation::References,
        ArtifactRelation::Implements,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: ArtifactRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}
```

**Step 6: Run tests to verify they fail**

Run: `cargo test -p sober-core -- workspace_state_serde worktree_state_serde artifact_kind_serde artifact_state_serde artifact_relation_serde`
Expected: FAIL --- enums not defined

**Step 7: Implement new enums**

In `enums.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "workspace_state", rename_all = "lowercase")]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Active,
    Archived,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "worktree_state", rename_all = "lowercase")]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    Active,
    Stale,
    Removing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "artifact_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    CodeChange,
    Document,
    Proposal,
    Snapshot,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "artifact_state", rename_all = "lowercase")]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Draft,
    Proposed,
    Approved,
    Rejected,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "artifact_relation", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRelation {
    SpawnedBy,
    Supersedes,
    References,
    Implements,
}
```

Ensure all five enums are re-exported in `types/mod.rs`.

**Step 8: Run all tests to verify they pass**

Run: `cargo test -p sober-core`
Expected: PASS

**Step 9: Commit**

```bash
git add backend/crates/sober-core/src/types/
git commit -m "feat(core): add workspace, worktree, and artifact types"
```

---

## Task 2: Add Workspace Config Types to sober-core

**Files:**
- Modify: `backend/crates/sober-core/Cargo.toml` (add `toml` dependency)
- Create: `backend/crates/sober-core/src/workspace_config.rs`
- Modify: `backend/crates/sober-core/src/lib.rs`

**Step 1: Write failing test for workspace config parsing**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_config() {
        let config = WorkspaceConfig::from_toml("").unwrap();
        assert!(config.llm.is_none());
        assert!(config.style.is_none());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[llm]
model = "anthropic/claude-sonnet-4"
context_budget = 2048

[style]
tone = "formal"
commit_convention = "conventional"
"#;
        let config = WorkspaceConfig::from_toml(toml).unwrap();
        assert_eq!(config.llm.as_ref().unwrap().model, "anthropic/claude-sonnet-4");
        assert_eq!(config.llm.as_ref().unwrap().context_budget, Some(2048));
        assert_eq!(config.style.as_ref().unwrap().tone.as_deref(), Some("formal"));
    }

    #[test]
    fn parse_workspace_state_json() {
        let state = WorkspaceAgentState::default();
        let json = serde_json::to_string(&state).unwrap();
        let back: WorkspaceAgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(state.observations.len(), back.observations.len());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sober-core -- parse_empty_config parse_full_config parse_workspace_state_json`
Expected: FAIL --- types not defined

**Step 3: Implement workspace config types**

Add `toml` to `backend/crates/sober-core/Cargo.toml`:

```toml
toml = { version = "0.8", features = ["parse"] }
```

Create `backend/crates/sober-core/src/workspace_config.rs`:

```rust
use serde::{Deserialize, Serialize};

/// User-editable workspace configuration (parsed from `.sober/config.toml`).
///
/// Additional sections (`[sandbox]`, `[shell]`) are added by plan 022
/// (shell execution). This struct uses `#[serde(deny_unknown_fields)]`-free
/// deserialization, so unknown TOML sections are silently ignored until
/// their config structs are added.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub llm: Option<WorkspaceLlmConfig>,
    pub style: Option<WorkspaceStyleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLlmConfig {
    pub model: String,
    pub context_budget: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStyleConfig {
    pub tone: Option<String>,
    pub commit_convention: Option<String>,
}

impl WorkspaceConfig {
    /// Parse a workspace config from TOML text.
    /// Empty or missing sections produce `None` fields.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        toml::from_str(text)
    }
}

/// Agent-managed workspace state (serialized to `.sober/state.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceAgentState {
    pub observations: Vec<Observation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub observed_at: String,
}
```

Add `pub mod workspace_config;` to `lib.rs` and re-export:
`pub use workspace_config::{WorkspaceConfig, WorkspaceAgentState};`

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-core -- parse_empty_config parse_full_config parse_workspace_state_json`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/crates/sober-core/src/workspace_config.rs backend/crates/sober-core/src/lib.rs backend/crates/sober-core/Cargo.toml
git commit -m "feat(core): add workspace config and agent state types"
```

---

## Task 3: SQL Migration

**Files:**
- Create: `backend/migrations/YYYYMMDDHHMMSS_create_workspace_tables.sql`

**Step 1: Write the migration**

```sql
-- Workspace management types
CREATE TYPE workspace_state AS ENUM ('active', 'archived', 'deleted');
CREATE TYPE worktree_state AS ENUM ('active', 'stale', 'removing');
CREATE TYPE artifact_kind AS ENUM ('code_change', 'document', 'proposal', 'snapshot', 'trace');
CREATE TYPE artifact_state AS ENUM ('draft', 'proposed', 'approved', 'rejected', 'archived');
CREATE TYPE artifact_relation AS ENUM ('spawned_by', 'supersedes', 'references', 'implements');

-- Workspaces
CREATE TABLE workspaces (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id),
    name        TEXT NOT NULL,
    description TEXT,
    root_path   TEXT NOT NULL,
    state       workspace_state NOT NULL DEFAULT 'active',
    created_by  UUID NOT NULL REFERENCES users(id),
    archived_at TIMESTAMPTZ,
    deleted_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, name)
);

-- Git repos within workspaces
CREATE TABLE workspace_repos (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    path            TEXT NOT NULL,
    is_linked       BOOLEAN NOT NULL DEFAULT false,
    remote_url      TEXT,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, path)
);

-- Git worktrees (tracked for lifecycle management)
CREATE TABLE worktrees (
    id              UUID PRIMARY KEY,
    repo_id         UUID NOT NULL REFERENCES workspace_repos(id) ON DELETE CASCADE,
    branch          TEXT NOT NULL,
    path            TEXT NOT NULL,
    state           worktree_state NOT NULL DEFAULT 'active',
    created_by      UUID REFERENCES users(id),
    task_id         UUID,
    conversation_id UUID REFERENCES conversations(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_active_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(repo_id, branch)
);

-- Artifacts (any output produced by agent or user)
CREATE TABLE artifacts (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id),
    user_id         UUID NOT NULL REFERENCES users(id),
    kind            artifact_kind NOT NULL,
    state           artifact_state NOT NULL DEFAULT 'draft',
    title           TEXT NOT NULL,
    description     TEXT,

    -- Location
    storage_type    TEXT NOT NULL,
    git_repo        TEXT,
    git_ref         TEXT,
    blob_key        TEXT,
    inline_content  TEXT,

    -- Provenance
    created_by      UUID REFERENCES users(id),
    conversation_id UUID REFERENCES conversations(id),
    task_id         UUID,
    parent_id       UUID REFERENCES artifacts(id),

    -- Review
    reviewed_by     UUID REFERENCES users(id),
    reviewed_at     TIMESTAMPTZ,

    -- Extensible
    metadata        JSONB NOT NULL DEFAULT '{}',

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Artifact relationships
CREATE TABLE artifact_relations (
    source_id       UUID NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    target_id       UUID NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    relation        artifact_relation NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_id, target_id, relation)
);

-- Indexes
CREATE INDEX idx_workspaces_user_id ON workspaces(user_id);
CREATE INDEX idx_workspaces_state ON workspaces(state);
CREATE INDEX idx_workspace_repos_workspace_id ON workspace_repos(workspace_id);
CREATE INDEX idx_worktrees_repo_id ON worktrees(repo_id);
CREATE INDEX idx_worktrees_state ON worktrees(state);
CREATE INDEX idx_artifacts_workspace_id ON artifacts(workspace_id);
CREATE INDEX idx_artifacts_user_id ON artifacts(user_id);
CREATE INDEX idx_artifacts_kind ON artifacts(kind);
CREATE INDEX idx_artifacts_state ON artifacts(state);
CREATE INDEX idx_artifacts_parent_id ON artifacts(parent_id);
CREATE INDEX idx_artifact_relations_target_id ON artifact_relations(target_id);
```

**Step 2: Verify migration syntax**

Run: `psql -f backend/migrations/YYYYMMDDHHMMSS_create_workspace_tables.sql --single-transaction` against a test database with prior migrations applied (requires `users`, `conversations` tables to exist).

Alternatively, if using sqlx migrate: `cargo sqlx migrate run` from `backend/`.

**Step 3: Commit**

```bash
git add backend/migrations/
git commit -m "feat(db): add workspace, worktree, and artifact tables"
```

---

## Task 4: Create sober-workspace Crate --- Scaffold

**Files:**
- Create: `backend/crates/sober-workspace/Cargo.toml`
- Create: `backend/crates/sober-workspace/src/lib.rs`
- Create: `backend/crates/sober-workspace/src/error.rs`
- Modify: `backend/Cargo.toml` (workspace members already include `crates/*`)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "sober-workspace"
version = "0.1.0"
edition.workspace = true

[dependencies]
sober-core = { path = "../sober-core" }
sqlx = { workspace = true }
tokio = { workspace = true, features = ["fs"] }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
sha2 = "0.10"
git2 = "0.20"
thiserror = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
```

**Step 2: Create error type**

`backend/crates/sober-workspace/src/error.rs`:

```rust
use sober_core::AppError;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace not found: {0}")]
    NotFound(String),

    #[error("workspace already exists: {0}")]
    AlreadyExists(String),

    #[error("workspace is archived")]
    Archived,

    #[error("repo not found: {0}")]
    RepoNotFound(String),

    #[error("worktree conflict: branch '{branch}' already checked out by {held_by}")]
    WorktreeConflict { branch: String, held_by: String },

    #[error("filesystem error: {0}")]
    Filesystem(#[source] std::io::Error),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },
}

impl From<WorkspaceError> for AppError {
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::NotFound(msg) => AppError::NotFound(msg),
            WorkspaceError::AlreadyExists(msg) => AppError::Conflict(msg),
            WorkspaceError::Archived => AppError::Validation("workspace is archived".into()),
            WorkspaceError::RepoNotFound(msg) => AppError::NotFound(msg),
            WorkspaceError::WorktreeConflict { branch, held_by } => {
                AppError::Conflict(format!("branch '{branch}' already checked out by {held_by}"))
            }
            WorkspaceError::InvalidStateTransition { from, to } => {
                AppError::Validation(format!("invalid state transition: {from} -> {to}"))
            }
            WorkspaceError::Filesystem(_)
            | WorkspaceError::Git(_)
            | WorkspaceError::Database(_) => AppError::Internal(err.into()),
        }
    }
}
```

**Step 3: Create lib.rs scaffold**

```rust
//! Workspace business logic for the Sober agent system.
//!
//! This crate owns filesystem layout, git operations (via git2), blob storage,
//! workspace config parsing, and worktree management. Database operations
//! (Pg*Repo) live in `sober-db`, not here.

pub mod error;
pub mod worktree;
pub mod blob;
pub mod fs;

pub use error::WorkspaceError;
```

Create empty module files for each submodule (each containing just a doc comment):
- `backend/crates/sober-workspace/src/worktree.rs` (git2 worktree operations only)
- `backend/crates/sober-workspace/src/blob.rs`
- `backend/crates/sober-workspace/src/fs.rs`

**Step 4: Verify it compiles**

Run: `cargo check -p sober-workspace`
Expected: PASS (no errors)

**Step 5: Commit**

```bash
git add backend/crates/sober-workspace/
git commit -m "feat(workspace): scaffold sober-workspace crate with error types"
```

---

## Task 5: Filesystem Initialization

**Files:**
- Modify: `backend/crates/sober-workspace/src/fs.rs`
- Test: inline `#[cfg(test)]` module

This module handles creating the `.sober/` directory structure and template files.

**Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn init_workspace_dir_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        assert!(root.join(".sober").is_dir());
        assert!(root.join(".sober/config.toml").is_file());
        assert!(root.join(".sober/state.json").is_file());
        assert!(root.join(".sober/proposals").is_dir());
        assert!(root.join(".sober/traces").is_dir());
        assert!(root.join(".sober/snapshots").is_dir());
        assert!(root.join(".sober/worktrees").is_dir());
    }

    #[tokio::test]
    async fn init_workspace_dir_default_config_is_valid_toml() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        let content = tokio::fs::read_to_string(root.join(".sober/config.toml"))
            .await
            .unwrap();
        let config: sober_core::WorkspaceConfig =
            sober_core::WorkspaceConfig::from_toml(&content).unwrap();
        assert!(config.llm.is_none()); // template has no values set
    }

    #[tokio::test]
    async fn init_workspace_dir_default_state_is_valid_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        let content = tokio::fs::read_to_string(root.join(".sober/state.json"))
            .await
            .unwrap();
        let state: sober_core::WorkspaceAgentState =
            serde_json::from_str(&content).unwrap();
        assert!(state.observations.is_empty());
    }
}
```

Add `tempfile` as a dev-dependency in `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sober-workspace -- init_workspace_dir`
Expected: FAIL --- `init_workspace_dir` not defined

**Step 3: Implement filesystem initialization**

```rust
//! Filesystem operations for workspace directory management.

use std::path::Path;
use tokio::fs;

use crate::WorkspaceError;

const DEFAULT_CONFIG_TOML: &str = "\
# Workspace configuration for Sober agent.
# Uncomment and modify settings as needed.

# [llm]
# model = \"anthropic/claude-sonnet-4\"
# context_budget = 4096

# [style]
# tone = \"neutral\"
# commit_convention = \"conventional\"
";

const DEFAULT_STATE_JSON: &str = r#"{"observations":[]}"#;

/// Initialize the `.sober/` directory structure inside a workspace root.
/// Creates the root directory if it does not exist.
pub async fn init_workspace_dir(workspace_root: &Path) -> Result<(), WorkspaceError> {
    let sober_dir = workspace_root.join(".sober");

    fs::create_dir_all(&sober_dir)
        .await
        .map_err(WorkspaceError::Filesystem)?;
    fs::create_dir_all(sober_dir.join("proposals"))
        .await
        .map_err(WorkspaceError::Filesystem)?;
    fs::create_dir_all(sober_dir.join("traces"))
        .await
        .map_err(WorkspaceError::Filesystem)?;
    fs::create_dir_all(sober_dir.join("snapshots"))
        .await
        .map_err(WorkspaceError::Filesystem)?;
    fs::create_dir_all(sober_dir.join("worktrees"))
        .await
        .map_err(WorkspaceError::Filesystem)?;

    let config_path = sober_dir.join("config.toml");
    if !config_path.exists() {
        fs::write(&config_path, DEFAULT_CONFIG_TOML)
            .await
            .map_err(WorkspaceError::Filesystem)?;
    }

    let state_path = sober_dir.join("state.json");
    if !state_path.exists() {
        fs::write(&state_path, DEFAULT_STATE_JSON)
            .await
            .map_err(WorkspaceError::Filesystem)?;
    }

    Ok(())
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-workspace -- init_workspace_dir`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/crates/sober-workspace/src/fs.rs backend/crates/sober-workspace/Cargo.toml
git commit -m "feat(workspace): add filesystem initialization for .sober/ directory"
```

---

## Task 6: Blob Storage

**Files:**
- Modify: `backend/crates/sober-workspace/src/blob.rs`
- Test: inline `#[cfg(test)]` module

Content-addressed blob storage under `/opt/sober/data/blobs/`.

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn store_and_retrieve_blob() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let data = b"hello world";
        let key = store.store(data).await.unwrap();

        let retrieved = store.retrieve(&key).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn store_deduplicates() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let data = b"duplicate content";
        let key1 = store.store(data).await.unwrap();
        let key2 = store.store(data).await.unwrap();

        assert_eq!(key1, key2);
    }

    #[tokio::test]
    async fn retrieve_missing_blob_errors() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let result = store.retrieve("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_blob() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let key = store.store(b"to be deleted").await.unwrap();
        assert!(store.exists(&key).await);

        store.delete(&key).await.unwrap();
        assert!(!store.exists(&key).await);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-workspace -- store_and_retrieve_blob store_deduplicates retrieve_missing delete_blob`
Expected: FAIL --- `BlobStore` not defined

**Step 3: Implement blob storage**

```rust
//! Content-addressed blob storage.
//!
//! Blobs are stored as `{root}/{sha256_prefix}/{sha256}` where the prefix
//! is the first 2 hex characters. This prevents directory listing from
//! becoming too large.

use std::path::PathBuf;
use sha2::{Sha256, Digest};
use tokio::fs;

use crate::WorkspaceError;

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Store data and return its content-addressed key (hex SHA-256).
    pub async fn store(&self, data: &[u8]) -> Result<String, WorkspaceError> {
        let key = hex_sha256(data);
        let path = self.blob_path(&key);

        if path.exists() {
            return Ok(key);
        }

        let parent = path.parent().expect("blob path has parent");
        fs::create_dir_all(parent)
            .await
            .map_err(WorkspaceError::Filesystem)?;
        fs::write(&path, data)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        Ok(key)
    }

    /// Retrieve blob data by key.
    pub async fn retrieve(&self, key: &str) -> Result<Vec<u8>, WorkspaceError> {
        let path = self.blob_path(key);
        fs::read(&path)
            .await
            .map_err(WorkspaceError::Filesystem)
    }

    /// Check if a blob exists.
    pub async fn exists(&self, key: &str) -> bool {
        self.blob_path(key).exists()
    }

    /// Delete a blob by key.
    pub async fn delete(&self, key: &str) -> Result<(), WorkspaceError> {
        let path = self.blob_path(key);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(WorkspaceError::Filesystem)?;
        }
        Ok(())
    }

    fn blob_path(&self, key: &str) -> PathBuf {
        let prefix = &key[..2];
        self.root.join(prefix).join(key)
    }
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-workspace -- store_and_retrieve_blob store_deduplicates retrieve_missing delete_blob`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/crates/sober-workspace/src/blob.rs
git commit -m "feat(workspace): add content-addressed blob storage"
```

---

## Task 7: Workspace CRUD (DB Layer)

> **IMPORTANT:** Per CLAUDE.md architecture rules, all PostgreSQL repo implementations
> (`Pg*Repo`) live in `sober-db`, not in `sober-workspace`. The `sober-workspace` crate
> contains only business logic (filesystem, git, blob). This task adds DB repos to `sober-db`.

**Files:**
- Create: `backend/crates/sober-db/src/repos/workspace.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`
- Test: `backend/crates/sober-db/tests/workspace_db.rs` (integration test, requires DB)

**Step 1: Write failing integration test**

Create `backend/crates/sober-db/tests/workspace_db.rs`:

```rust
//! Integration tests for workspace DB operations.
//! Requires a running PostgreSQL instance with migrations applied.
//! Set DATABASE_URL env var to run.

use sober_core::*;
use sober_db::repos::workspace::PgWorkspaceRepo;
use sqlx::PgPool;

async fn setup_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&url).await.expect("Failed to connect to DB")
}

#[sqlx::test]
async fn create_and_get_workspace(pool: PgPool) {
    let repo = PgWorkspaceRepo::new(pool);
    let user_id = UserId::new();

    // Assumes users table is seeded by test fixtures
    let ws = repo
        .create("test-project", Some("A test workspace"), user_id, "/tmp/test")
        .await
        .unwrap();

    assert_eq!(ws.name, "test-project");
    assert_eq!(ws.state, WorkspaceState::Active);

    let fetched = repo.get(ws.id).await.unwrap();
    assert_eq!(fetched.id, ws.id);
}

#[sqlx::test]
async fn archive_and_restore_workspace(pool: PgPool) {
    let repo = PgWorkspaceRepo::new(pool);
    let user_id = UserId::new();

    let ws = repo
        .create("archive-test", None, user_id, "/tmp/archive")
        .await
        .unwrap();

    repo.archive(ws.id).await.unwrap();
    let archived = repo.get(ws.id).await.unwrap();
    assert_eq!(archived.state, WorkspaceState::Archived);
    assert!(archived.archived_at.is_some());

    repo.restore(ws.id).await.unwrap();
    let restored = repo.get(ws.id).await.unwrap();
    assert_eq!(restored.state, WorkspaceState::Active);
    assert!(restored.archived_at.is_none());
}

#[sqlx::test]
async fn delete_requires_archived(pool: PgPool) {
    let repo = PgWorkspaceRepo::new(pool);
    let user_id = UserId::new();

    let ws = repo
        .create("delete-test", None, user_id, "/tmp/delete")
        .await
        .unwrap();

    // Cannot delete active workspace
    let result = repo.delete(ws.id).await;
    assert!(result.is_err());

    // Archive first, then delete
    repo.archive(ws.id).await.unwrap();
    repo.delete(ws.id).await.unwrap();

    let deleted = repo.get(ws.id).await.unwrap();
    assert_eq!(deleted.state, WorkspaceState::Deleted);
    assert!(deleted.deleted_at.is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sober-db --test workspace_db`
Expected: FAIL --- `PgWorkspaceRepo` not defined

**Step 3: Implement workspace DB operations**

In `backend/crates/sober-db/src/repos/workspace.rs`:

```rust
//! Workspace CRUD operations backed by PostgreSQL.

use sober_core::*;
use sqlx::PgPool;

use crate::WorkspaceError;

/// Database row for a workspace.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub user_id: UserId,
    pub name: String,
    pub description: Option<String>,
    pub root_path: String,
    pub state: WorkspaceState,
    pub created_by: UserId,
    pub archived_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// PostgreSQL repository for workspace DB operations.
pub struct PgWorkspaceRepo {
    pool: PgPool,
}

impl PgWorkspaceRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        created_by: UserId,
        root_path: &str,
    ) -> Result<Workspace, WorkspaceError> {
        let id = WorkspaceId::new();
        let ws = sqlx::query_as::<_, Workspace>(
            r#"
            INSERT INTO workspaces (id, user_id, name, description, root_path, created_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(created_by)
        .bind(name)
        .bind(description)
        .bind(root_path)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(ws)
    }

    pub async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| WorkspaceError::NotFound(id.to_string()))
    }

    pub async fn list_by_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<Workspace>, WorkspaceError> {
        let workspaces = sqlx::query_as::<_, Workspace>(
            "SELECT * FROM workspaces WHERE user_id = $1 AND state != 'deleted' ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(workspaces)
    }

    pub async fn archive(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let ws = self.get(id).await?;
        if ws.state != WorkspaceState::Active {
            return Err(WorkspaceError::InvalidStateTransition {
                from: format!("{:?}", ws.state),
                to: "archived".into(),
            });
        }
        sqlx::query(
            "UPDATE workspaces SET state = 'archived', archived_at = now(), updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn restore(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let ws = self.get(id).await?;
        if ws.state != WorkspaceState::Archived {
            return Err(WorkspaceError::InvalidStateTransition {
                from: format!("{:?}", ws.state),
                to: "active".into(),
            });
        }
        sqlx::query(
            "UPDATE workspaces SET state = 'active', archived_at = NULL, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let ws = self.get(id).await?;
        if ws.state != WorkspaceState::Archived {
            return Err(WorkspaceError::InvalidStateTransition {
                from: format!("{:?}", ws.state),
                to: "deleted".into(),
            });
        }
        sqlx::query(
            "UPDATE workspaces SET state = 'deleted', deleted_at = now(), updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
```

**Step 4: Run integration tests**

Run: `cargo test -p sober-db --test workspace_db`
Expected: PASS (requires DATABASE_URL)

**Step 5: Commit**

```bash
git add backend/crates/sober-db/src/repos/workspace.rs backend/crates/sober-db/tests/
git commit -m "feat(db): add PgWorkspaceRepo with lifecycle state machine"
```

---

## Task 8: Repo Management (DB Layer)

> **Note:** DB repos live in `sober-db`, not `sober-workspace`.

**Files:**
- Create: `backend/crates/sober-db/src/repos/workspace_repo.rs`
- Test: `backend/crates/sober-db/tests/repo_db.rs`

**Step 1: Write failing integration test**

```rust
#[sqlx::test]
async fn register_managed_repo(pool: PgPool) {
    // setup workspace first...
    let repo_mgr = RepoManager::new(pool);

    let repo = repo_mgr
        .register(workspace_id, "my-repo", "repos/my-repo", false, Some("https://github.com/user/repo"), "main")
        .await
        .unwrap();

    assert_eq!(repo.name, "my-repo");
    assert!(!repo.is_linked);
}

#[sqlx::test]
async fn register_linked_repo(pool: PgPool) {
    let repo_mgr = RepoManager::new(pool);

    let repo = repo_mgr
        .register(workspace_id, "external", "/home/user/Projects/app", true, None, "main")
        .await
        .unwrap();

    assert!(repo.is_linked);
}

#[sqlx::test]
async fn find_workspace_for_linked_path(pool: PgPool) {
    let repo_mgr = RepoManager::new(pool);
    // register linked repo...

    let result = repo_mgr
        .find_by_linked_path("/home/user/Projects/app", user_id)
        .await
        .unwrap();

    assert!(result.is_some());
}
```

**Step 2: Implement repo manager**

```rust
//! Git repository registration and lookup within workspaces.

use sober_core::*;
use sqlx::PgPool;

use crate::WorkspaceError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkspaceRepoRow {
    pub id: WorkspaceRepoId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub path: String,
    pub is_linked: bool,
    pub remote_url: Option<String>,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
}

pub struct RepoManager {
    pool: PgPool,
}

impl RepoManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn register(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
        path: &str,
        is_linked: bool,
        remote_url: Option<&str>,
        default_branch: &str,
    ) -> Result<WorkspaceRepoRow, WorkspaceError> {
        let id = WorkspaceRepoId::new();
        let repo = sqlx::query_as::<_, WorkspaceRepoRow>(
            r#"
            INSERT INTO workspace_repos (id, workspace_id, name, path, is_linked, remote_url, default_branch)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(workspace_id)
        .bind(name)
        .bind(path)
        .bind(is_linked)
        .bind(remote_url)
        .bind(default_branch)
        .fetch_one(&self.pool)
        .await?;
        Ok(repo)
    }

    pub async fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<WorkspaceRepoRow>, WorkspaceError> {
        let repos = sqlx::query_as::<_, WorkspaceRepoRow>(
            "SELECT * FROM workspace_repos WHERE workspace_id = $1 ORDER BY name",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(repos)
    }

    pub async fn find_by_linked_path(
        &self,
        path: &str,
        user_id: UserId,
    ) -> Result<Option<(WorkspaceId, WorkspaceRepoRow)>, WorkspaceError> {
        let row = sqlx::query_as::<_, WorkspaceRepoRow>(
            r#"
            SELECT wr.* FROM workspace_repos wr
            JOIN workspaces w ON wr.workspace_id = w.id
            WHERE wr.path = $1
              AND wr.is_linked = true
              AND w.user_id = $2
              AND w.state = 'active'
            "#,
        )
        .bind(path)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.workspace_id, r)))
    }

    pub async fn remove(
        &self,
        repo_id: WorkspaceRepoId,
    ) -> Result<(), WorkspaceError> {
        sqlx::query("DELETE FROM workspace_repos WHERE id = $1")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
```

**Step 3: Run tests, verify pass, commit**

```bash
git add backend/crates/sober-db/src/repos/workspace_repo.rs backend/crates/sober-db/tests/repo_db.rs
git commit -m "feat(db): add PgWorkspaceRepoRepo for repo registration and discovery"
```

---

## Task 9: Worktree Lifecycle (DB + Git)

> **Note:** DB repos live in `sober-db`. Git operations live in `sober-workspace`.

**Files:**
- Create: `backend/crates/sober-db/src/repos/worktree.rs` (DB operations)
- Modify: `backend/crates/sober-workspace/src/worktree.rs` (git2 operations only)
- Test: `backend/crates/sober-db/tests/worktree_db.rs` (integration)
- Test: inline unit tests for git operations

**Step 1: Write failing tests**

Integration test for DB operations:

```rust
#[sqlx::test]
async fn create_worktree_tracks_in_db(pool: PgPool) {
    let mgr = WorktreeManager::new(pool);
    let wt = mgr
        .create(repo_id, "feat/new-feature", "/path/to/worktree", Some(user_id), None, None)
        .await
        .unwrap();

    assert_eq!(wt.state, WorktreeState::Active);
    assert_eq!(wt.branch, "feat/new-feature");
}

#[sqlx::test]
async fn duplicate_branch_rejected(pool: PgPool) {
    let mgr = WorktreeManager::new(pool);
    mgr.create(repo_id, "feat/same", "/path/1", None, None, None).await.unwrap();

    let result = mgr.create(repo_id, "feat/same", "/path/2", None, None, None).await;
    assert!(result.is_err());
}

#[sqlx::test]
async fn mark_stale_and_cleanup(pool: PgPool) {
    let mgr = WorktreeManager::new(pool);
    let wt = mgr.create(repo_id, "feat/old", "/path/old", None, None, None).await.unwrap();

    mgr.mark_stale(wt.id).await.unwrap();
    let stale = mgr.get(wt.id).await.unwrap();
    assert_eq!(stale.state, WorktreeState::Stale);

    mgr.mark_removing(wt.id).await.unwrap();
    let removing = mgr.get(wt.id).await.unwrap();
    assert_eq!(removing.state, WorktreeState::Removing);
}
```

Unit test for git worktree creation (uses tempdir + git2):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_git_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        let repo = git2::Repository::init(&repo_path).unwrap();

        // Create initial commit so we have a HEAD
        let sig = repo.signature().unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();

        let wt_path = tmp.path().join("worktree");
        create_git_worktree(&repo_path, &wt_path, "feat/test").unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn remove_git_worktree() {
        let tmp = TempDir::new().unwrap();
        // setup repo + worktree...

        remove_git_worktree(&repo_path, &wt_path).unwrap();
        assert!(!wt_path.exists());
    }
}
```

**Step 2: Implement worktree manager**

DB operations (`WorktreeManager`) for create/get/list/mark_stale/mark_removing/delete.

Git operations (`create_git_worktree`, `remove_git_worktree`) using `git2`.

The `create` method:
1. Inserts DB row (catches unique constraint violation → `WorktreeConflict` error)
2. Calls `create_git_worktree` for the filesystem operation
3. If git fails, rolls back the DB row

The `cleanup` method (called by scheduler):
1. Queries all worktrees with `state = 'stale'` and `last_active_at` older than threshold
2. For each: mark `removing`, delete filesystem, delete DB row

**Step 3: Run tests, verify pass, commit**

```bash
git add backend/crates/sober-db/src/repos/worktree.rs backend/crates/sober-workspace/src/worktree.rs backend/crates/sober-db/tests/worktree_db.rs
git commit -m "feat(workspace): add worktree lifecycle with git integration"
```

---

## Task 10: Artifact Tracking (DB Layer)

> **Note:** DB repos live in `sober-db`, not `sober-workspace`.

**Files:**
- Create: `backend/crates/sober-db/src/repos/artifact.rs`
- Test: `backend/crates/sober-db/tests/artifact_db.rs`

**Step 1: Write failing tests**

```rust
#[sqlx::test]
async fn create_and_retrieve_artifact(pool: PgPool) {
    let store = ArtifactStore::new(pool);

    let artifact = store.create(CreateArtifact {
        workspace_id,
        user_id,
        kind: ArtifactKind::Document,
        title: "Design doc".into(),
        description: Some("Architecture design".into()),
        storage_type: "inline".into(),
        inline_content: Some("# Design\n\nContent here.".into()),
        created_by: Some(user_id),
        ..Default::default()
    }).await.unwrap();

    assert_eq!(artifact.state, ArtifactState::Draft);

    let fetched = store.get(artifact.id).await.unwrap();
    assert_eq!(fetched.title, "Design doc");
}

#[sqlx::test]
async fn artifact_state_transitions(pool: PgPool) {
    let store = ArtifactStore::new(pool);
    let artifact = /* create draft artifact */;

    store.transition(artifact.id, ArtifactState::Proposed).await.unwrap();
    store.transition(artifact.id, ArtifactState::Approved).await.unwrap();

    // Can't go back to draft from approved
    let result = store.transition(artifact.id, ArtifactState::Draft).await;
    assert!(result.is_err());
}

#[sqlx::test]
async fn artifact_relations(pool: PgPool) {
    let store = ArtifactStore::new(pool);
    let proposal = /* create proposal artifact */;
    let code_change = /* create code_change artifact */;

    store.add_relation(code_change.id, proposal.id, ArtifactRelation::Implements).await.unwrap();

    let relations = store.get_relations(code_change.id).await.unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].relation, ArtifactRelation::Implements);
}

#[sqlx::test]
async fn list_artifacts_by_visibility(pool: PgPool) {
    let store = ArtifactStore::new(pool);
    // Create artifacts of different kinds...

    // Workspace member sees code_change and document, not trace
    let visible = store.list_visible(workspace_id, false /* not admin */).await.unwrap();
    assert!(visible.iter().all(|a| a.kind != ArtifactKind::Trace));
}
```

**Step 2: Implement artifact store**

`ArtifactStore` with methods:
- `create(input: CreateArtifact) -> Result<Artifact>`
- `get(id: ArtifactId) -> Result<Artifact>`
- `transition(id: ArtifactId, new_state: ArtifactState) -> Result<()>` --- validates legal transitions
- `add_relation(source: ArtifactId, target: ArtifactId, relation: ArtifactRelation) -> Result<()>`
- `get_relations(artifact_id: ArtifactId) -> Result<Vec<ArtifactRelationRow>>`
- `list_visible(workspace_id: WorkspaceId, is_admin: bool) -> Result<Vec<Artifact>>`
- `list_by_workspace(workspace_id: WorkspaceId) -> Result<Vec<Artifact>>`

State transition validation:

```rust
fn valid_transition(from: ArtifactState, to: ArtifactState) -> bool {
    matches!(
        (from, to),
        (ArtifactState::Draft, ArtifactState::Proposed)
            | (ArtifactState::Draft, ArtifactState::Archived)
            | (ArtifactState::Proposed, ArtifactState::Approved)
            | (ArtifactState::Proposed, ArtifactState::Rejected)
            | (ArtifactState::Proposed, ArtifactState::Draft) // revision requested
            | (ArtifactState::Rejected, ArtifactState::Draft) // rework
            | (ArtifactState::Approved, ArtifactState::Archived)
            | (ArtifactState::Rejected, ArtifactState::Archived)
    )
}
```

Visibility filtering in `list_visible`:

```rust
let kind_filter = if is_admin {
    // Admins see everything
    vec![]
} else {
    // Non-admins don't see traces
    vec![ArtifactKind::Trace]
};
```

**Step 3: Run tests, verify pass, commit**

```bash
git add backend/crates/sober-db/src/repos/artifact.rs backend/crates/sober-db/tests/artifact_db.rs
git commit -m "feat(workspace): add artifact tracking with state machine and relations"
```

---

## Task 11: Workspace Snapshots

**Files:**
- Create: `backend/crates/sober-workspace/src/snapshot.rs`
- Modify: `backend/crates/sober-workspace/src/lib.rs`
- Test: inline `#[cfg(test)]` module

Snapshots capture the state of a workspace before potentially destructive
operations. Used by plan 022 (shell execution) to auto-snapshot before
dangerous commands.

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn create_snapshot_produces_tar() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"content").await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir.clone());
        let snap = mgr.create(&ws_root, "pre-shell").await.unwrap();

        assert!(snap.path.exists());
        assert!(snap.path.extension().is_some_and(|e| e == "tar"));
        assert_eq!(snap.label, "pre-shell");
    }

    #[tokio::test]
    async fn restore_snapshot_overwrites_workspace() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"original").await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        let snap = mgr.create(&ws_root, "backup").await.unwrap();

        // Modify workspace
        tokio::fs::write(ws_root.join("file.txt"), b"modified").await.unwrap();

        mgr.restore(&snap, &ws_root).await.unwrap();
        let content = tokio::fs::read_to_string(ws_root.join("file.txt")).await.unwrap();
        assert_eq!(content, "original");
    }

    #[tokio::test]
    async fn list_snapshots() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        mgr.create(&ws_root, "snap-1").await.unwrap();
        mgr.create(&ws_root, "snap-2").await.unwrap();

        let snaps = mgr.list().await.unwrap();
        assert_eq!(snaps.len(), 2);
    }

    #[tokio::test]
    async fn prune_removes_oldest_snapshots() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"data").await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);

        // Create 4 snapshots with small delay so filenames differ
        for i in 0..4 {
            mgr.create(&ws_root, &format!("snap-{i}")).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        assert_eq!(mgr.list().await.unwrap().len(), 4);

        let removed = mgr.prune(2).await.unwrap();
        assert_eq!(removed, 2);
        assert_eq!(mgr.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn prune_noop_when_under_limit() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        mgr.create(&ws_root, "only-one").await.unwrap();

        let removed = mgr.prune(10).await.unwrap();
        assert_eq!(removed, 0);
        assert_eq!(mgr.list().await.unwrap().len(), 1);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-workspace -q -- snapshot`
Expected: FAIL --- `SnapshotManager` not defined

**Step 3: Implement snapshot manager**

```rust
//! Workspace snapshot creation and restoration.
//!
//! Snapshots are tar archives of the workspace root directory. They provide
//! a simple rollback mechanism before destructive operations.

use std::path::{Path, PathBuf};
use chrono::Utc;
use tokio::fs;
use tokio::process::Command;

use crate::WorkspaceError;

/// Metadata for a created snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub path: PathBuf,
    pub label: String,
    pub created_at: chrono::DateTime<Utc>,
}

/// Manages workspace snapshots (tar archives).
pub struct SnapshotManager {
    snapshot_dir: PathBuf,
}

impl SnapshotManager {
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self { snapshot_dir }
    }

    /// Create a tar snapshot of the workspace root.
    pub async fn create(
        &self,
        workspace_root: &Path,
        label: &str,
    ) -> Result<Snapshot, WorkspaceError> {
        fs::create_dir_all(&self.snapshot_dir)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        let now = Utc::now();
        let filename = format!("{}-{}.tar", now.format("%Y%m%d%H%M%S"), label);
        let snap_path = self.snapshot_dir.join(&filename);

        let output = Command::new("tar")
            .arg("cf")
            .arg(&snap_path)
            .arg("-C")
            .arg(workspace_root)
            .arg(".")
            .output()
            .await
            .map_err(|e| WorkspaceError::Snapshot(format!("tar failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WorkspaceError::Snapshot(
                format!("tar exited {}: {stderr}", output.status),
            ));
        }

        Ok(Snapshot {
            path: snap_path,
            label: label.to_string(),
            created_at: now,
        })
    }

    /// Restore a snapshot by extracting it over the workspace root.
    pub async fn restore(
        &self,
        snapshot: &Snapshot,
        workspace_root: &Path,
    ) -> Result<(), WorkspaceError> {
        let output = Command::new("tar")
            .arg("xf")
            .arg(&snapshot.path)
            .arg("-C")
            .arg(workspace_root)
            .output()
            .await
            .map_err(|e| WorkspaceError::Snapshot(format!("tar restore failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WorkspaceError::Snapshot(
                format!("tar restore exited {}: {stderr}", output.status),
            ));
        }

        Ok(())
    }

    /// List all snapshots in the snapshot directory, sorted oldest first.
    pub async fn list(&self) -> Result<Vec<Snapshot>, WorkspaceError> {
        let mut entries = fs::read_dir(&self.snapshot_dir)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        let mut snapshots = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(WorkspaceError::Filesystem)? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "tar") {
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                // Parse label from filename: "YYYYMMDDHHMMSS-label"
                let label = name.get(15..).unwrap_or(name).to_string();
                snapshots.push(Snapshot {
                    path,
                    label,
                    created_at: Utc::now(), // approximate; could parse from filename
                });
            }
        }

        // Sort by filename (which starts with timestamp) — oldest first
        snapshots.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(snapshots)
    }

    /// Prune oldest snapshots exceeding `max_snapshots`. Called after creating
    /// a new snapshot when the workspace has a configured limit.
    pub async fn prune(&self, max_snapshots: u32) -> Result<u32, WorkspaceError> {
        let snapshots = self.list().await?;
        let to_remove = snapshots.len().saturating_sub(max_snapshots as usize);
        let mut removed = 0u32;

        for snap in snapshots.iter().take(to_remove) {
            fs::remove_file(&snap.path)
                .await
                .map_err(WorkspaceError::Filesystem)?;
            removed += 1;
        }

        Ok(removed)
    }
}
```

**Step 4: Add Snapshot error variant to WorkspaceError**

```rust
// In sober-workspace/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    // ... existing variants ...
    #[error("Snapshot error: {0}")]
    Snapshot(String),
}
```

**Step 5: Add module to lib.rs**

```rust
pub mod snapshot;
pub use snapshot::SnapshotManager;
```

**Step 6: Run tests to verify they pass**

Run: `cargo test -p sober-workspace -q -- snapshot`
Expected: PASS

**Step 7: Commit**

```bash
git add backend/crates/sober-workspace/src/snapshot.rs backend/crates/sober-workspace/src/error.rs backend/crates/sober-workspace/src/lib.rs
git commit -m "feat(workspace): add snapshot creation and restoration"
```

---

## Task 12: Integration --- Workspace System Defaults in sober-workspace

**Design decision:** Workspace operational defaults (`data_root`, retention periods,
stale thresholds) are **not** part of `AppConfig`. `AppConfig` is strictly for
infrastructure config (DB URLs, ports, API keys). Workspace defaults are owned by
`sober-workspace` and loaded at runtime. Per-project settings come from
`.sober/config.toml` via the workspace config resolution chain.

**Files:**
- Modify: `backend/crates/sober-workspace/src/config.rs`

**Step 1: Define workspace system defaults**

In `sober-workspace`:

```rust
/// Operational defaults for the workspace system.
/// These are compile-time defaults overridable via `.sober/config.toml`.
pub struct WorkspaceDefaults {
    /// Root directory for all workspace data.
    pub data_root: PathBuf,
    /// Blob retention period after workspace deletion (days).
    pub blob_retention_days: u32,
    /// Workspace archive grace period before hard delete (days).
    pub archive_grace_period_days: u32,
    /// Worktree stale threshold (hours).
    pub worktree_stale_hours: u32,
}

impl Default for WorkspaceDefaults {
    fn default() -> Self {
        Self {
            data_root: PathBuf::from("/opt/sober/data"),
            blob_retention_days: 90,
            archive_grace_period_days: 30,
            worktree_stale_hours: 24,
        }
    }
}
```

These defaults are used as fallbacks when `.sober/config.toml` does not specify
a value. Environment variable overrides (`SOBER_DATA_ROOT`, etc.) are read by
`sober-workspace` directly --- not routed through `AppConfig`.

**Step 2: Test defaults**

```rust
#[test]
fn workspace_defaults() {
    let defaults = WorkspaceDefaults::default();
    assert_eq!(defaults.data_root, PathBuf::from("/opt/sober/data"));
    assert_eq!(defaults.blob_retention_days, 90);
}
```

**Step 3: Commit**

```bash
git add backend/crates/sober-workspace/src/config.rs
git commit -m "feat(workspace): add workspace system defaults"
```

---

## Task 13: Update ARCHITECTURE.md and Existing Designs

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/plans/pending/010-sober-mind/design.md`
- Modify: `docs/plans/pending/003-sober-core/design.md`

**Step 1: Update ARCHITECTURE.md**

- Replace all `~/.sõber/` references with `~/.sober/`
- Add `sober-workspace` to the crate map table
- Add workspace concept to the system architecture diagram
- Document the filesystem layout under a new "Workspace & Artifact System" section
- Update the crate dependency flow to include `sober-workspace`

**Step 2: Update sober-mind design (010)**

- Change `~/.sõber/SOUL.md` to `~/.sober/SOUL.md`
- Change `./.sõber/SOUL.md` to `.sober/soul.md`
- Note that `PromptContext` gains `workspace_id: Option<WorkspaceId>`

**Step 3: Update sober-core design (003)**

- `WorkspaceId` is already defined in sober-core (decided in C13). Add `WorkspaceRepoId`, `WorktreeId`, `ArtifactId` to the ID types list
- Add `WorkspaceState`, `WorktreeState`, `ArtifactKind`, `ArtifactState`, `ArtifactRelation` to the enums list
- Add `toml` to the dependencies table

**Step 4: Commit**

```bash
git add ARCHITECTURE.md docs/plans/pending/010-sober-mind/design.md docs/plans/pending/003-sober-core/design.md
git commit -m "docs(arch): update for workspace system, fix sober path naming"
```

---

## Task 14: Clippy, Docs, Final Verification

**Step 1: Run clippy on the new crate**

Run: `cargo clippy -p sober-workspace -- -D warnings`
Expected: PASS (zero warnings)

Fix any issues.

**Step 2: Run all workspace crate tests**

Run: `cargo test -p sober-workspace`
Expected: PASS (unit tests; integration tests require DATABASE_URL)

**Step 3: Generate docs**

Run: `cargo doc -p sober-workspace --no-deps`
Expected: No warnings. All public items documented.

**Step 4: Run full workspace build**

Run: `cargo build --workspace`
Expected: PASS

**Step 5: Commit any fixes**

```bash
git add -A
git commit -m "chore(workspace): clippy fixes and documentation"
```

---

## Acceptance Criteria

- [ ] `sober-core` exports `WorkspaceId`, `WorkspaceRepoId`, `WorktreeId`, `ArtifactId`
- [ ] `sober-core` exports `WorkspaceState`, `WorktreeState`, `ArtifactKind`, `ArtifactState`, `ArtifactRelation`
- [ ] `sober-core` exports `WorkspaceConfig` and `WorkspaceAgentState`
- [ ] SQL migration creates all 5 tables with correct constraints and indexes
- [ ] `sober-workspace` crate compiles and passes clippy with zero warnings
- [ ] Filesystem initialization creates correct `.sober/` structure
- [ ] Blob storage correctly deduplicates by content hash
- [ ] Workspace lifecycle enforces active -> archived -> deleted transitions
- [ ] Worktree creation rejects duplicate branches with clear error
- [ ] Artifact state machine validates legal transitions
- [ ] Artifact visibility filtering excludes traces from non-admins
- [ ] Linked repo discovery queries work with user_id filtering
- [ ] Workspace snapshots can be created, listed, and restored
- [ ] `ARCHITECTURE.md` updated with `~/.sober/` paths and workspace crate
- [ ] `cargo test --workspace` passes
- [ ] All public items in `sober-workspace` have doc comments

---

## Phase 2: Workspace Enforcement, Remote Integration & SOUL.md Rules

---

### Task 15: Add workspace_id to CallerContext

**Files:**
- Modify: `backend/crates/sober-core/src/types/access.rs`

**Step 1: Add workspace_id field to CallerContext**

```rust
pub struct CallerContext {
    pub user_id: Option<UserId>,
    pub trigger: TriggerKind,
    pub permissions: Vec<Permission>,
    pub scope_grants: Vec<ScopeId>,
    /// The workspace this operation is scoped to, if any.
    pub workspace_id: Option<WorkspaceId>,
}
```

Add `WorkspaceId` to the imports from `super::ids`.

**Step 2: Update existing tests**

Update `caller_context_for_human` and `caller_context_for_scheduler` tests to include `workspace_id: None`.

Add a new test:

```rust
#[test]
fn caller_context_with_workspace() {
    let user_id = UserId::new();
    let scope = ScopeId::new();
    let workspace_id = WorkspaceId::new();
    let ctx = CallerContext {
        user_id: Some(user_id),
        trigger: TriggerKind::Human,
        permissions: vec![Permission::ReadKnowledge(scope)],
        scope_grants: vec![scope],
        workspace_id: Some(workspace_id),
    };
    assert_eq!(ctx.workspace_id, Some(workspace_id));
}
```

**Step 3: Verify**

Run: `cargo test -p sober-core -q`

**Step 4: Commit**

```
feat(core): add workspace_id to CallerContext
```

---

### Task 16: Add workspace_id to Conversation + Migration

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs`
- Modify: `backend/crates/sober-core/src/types/repo.rs`
- Modify: `backend/crates/sober-db/src/rows.rs`
- Modify: `backend/crates/sober-db/src/repos/conversations.rs`
- Create: `backend/migrations/20260311000002_conversation_workspace.sql`

**Step 1: Migration**

```sql
ALTER TABLE conversations
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id);

CREATE INDEX idx_conversations_workspace_id ON conversations(workspace_id);
```

**Step 2: Update Conversation domain type**

In `domain.rs`, add to the `Conversation` struct:

```rust
/// The workspace this conversation is scoped to, if any.
pub workspace_id: Option<WorkspaceId>,
```

Add `WorkspaceId` to the imports.

**Step 3: Update ConversationRow**

In `rows.rs`, add `pub workspace_id: Option<Uuid>` to `ConversationRow` and
update its `From` impl:

```rust
workspace_id: row.workspace_id.map(WorkspaceId::from_uuid),
```

**Step 4: Update ConversationRepo trait**

In `repo.rs`, change `create` signature:

```rust
fn create(
    &self,
    user_id: UserId,
    title: Option<&str>,
    workspace_id: Option<WorkspaceId>,
) -> impl Future<Output = Result<Conversation, AppError>> + Send;
```

**Step 5: Update PgConversationRepo**

In `conversations.rs`, update the `create` impl to accept and bind `workspace_id`.
Update all SELECT queries to include `workspace_id`.

**Step 6: Update downstream callers**

In `backend/crates/sober-api/src/routes/conversations.rs`:
- Add `workspace_id: Option<String>` to `CreateConversationRequest`
- Parse it to `Option<WorkspaceId>` and pass to `repo.create()`
- Include `workspace_id` in JSON responses

**Step 7: Verify**

Run: `cargo test --workspace -q`

All existing tests should pass (workspace_id is optional/nullable everywhere).

**Step 8: Commit**

```
feat(core): add workspace_id to conversations
```

---

### Task 17: Add remote detection to sober-workspace

**Files:**
- Create: `backend/crates/sober-workspace/src/remote.rs`
- Modify: `backend/crates/sober-workspace/src/lib.rs`

**Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo_with_remote(path: &Path, url: &str) -> git2::Repository {
        let repo = git2::Repository::init(path).unwrap();
        repo.remote("origin", url).unwrap();
        // Create initial commit so HEAD exists
        {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn detect_remote_url_finds_origin() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        init_repo_with_remote(&repo_path, "git@github.com:user/repo.git");

        let url = detect_remote_url(&repo_path).unwrap();
        assert_eq!(url.as_deref(), Some("git@github.com:user/repo.git"));
    }

    #[test]
    fn detect_remote_url_returns_none_without_remotes() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        git2::Repository::init(&repo_path).unwrap();

        let url = detect_remote_url(&repo_path).unwrap();
        assert!(url.is_none());
    }

    #[test]
    fn detect_remote_url_falls_back_to_first_remote() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        let repo = git2::Repository::init(&repo_path).unwrap();
        repo.remote("upstream", "https://example.com/repo.git").unwrap();

        let url = detect_remote_url(&repo_path).unwrap();
        assert_eq!(url.as_deref(), Some("https://example.com/repo.git"));
    }
}
```

**Step 2: Implement detect_remote_url**

```rust
//! Git remote operations.

use std::path::Path;

use crate::WorkspaceError;

/// Detect the remote URL for a repository.
///
/// Tries "origin" first, then falls back to the first available remote.
/// Returns `None` if no remotes are configured.
pub fn detect_remote_url(repo_path: &Path) -> Result<Option<String>, WorkspaceError> {
    let repo = git2::Repository::open(repo_path).map_err(WorkspaceError::Git)?;

    // Try "origin" first
    if let Ok(remote) = repo.find_remote("origin") {
        if let Some(url) = remote.url() {
            return Ok(Some(url.to_string()));
        }
    }

    // Fall back to first available remote
    let remotes = repo.remotes().map_err(WorkspaceError::Git)?;
    for name in remotes.iter().flatten() {
        if let Ok(remote) = repo.find_remote(name) {
            if let Some(url) = remote.url() {
                return Ok(Some(url.to_string()));
            }
        }
    }

    Ok(None)
}
```

**Step 3: Add module to lib.rs**

Add `pub mod remote;` and `pub use remote::detect_remote_url;`

**Step 4: Verify**

Run: `cargo test -p sober-workspace -q`

**Step 5: Commit**

```
feat(workspace): add git remote URL auto-detection
```

---

### Task 18: Add push_branch to sober-workspace

**Files:**
- Modify: `backend/crates/sober-workspace/src/remote.rs`
- Modify: `backend/crates/sober-workspace/src/lib.rs`

**Step 1: Implement push_branch**

```rust
/// Push a local branch to a remote.
///
/// Defaults to "origin" if no remote name is provided. Uses the
/// environment's SSH credentials (SSH agent, `~/.ssh/`).
pub fn push_branch(
    repo_path: &Path,
    branch: &str,
    remote: Option<&str>,
) -> Result<(), WorkspaceError> {
    let repo = git2::Repository::open(repo_path).map_err(WorkspaceError::Git)?;
    let remote_name = remote.unwrap_or("origin");
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(WorkspaceError::Git)?;

    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed_types| {
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            let username = username_from_url.unwrap_or("git");
            git2::Cred::ssh_key_from_agent(username)
        } else {
            Err(git2::Error::from_str("unsupported credential type"))
        }
    });

    let mut push_options = git2::PushOptions::new();
    push_options.remote_callbacks(callbacks);

    remote
        .push(&[&refspec], Some(&mut push_options))
        .map_err(WorkspaceError::Git)?;

    Ok(())
}
```

**Step 2: Add re-export to lib.rs**

Update: `pub use remote::{detect_remote_url, push_branch};`

**Step 3: Verify compilation**

Run: `cargo build -p sober-workspace -q`

Note: `push_branch` is difficult to unit test (requires a real remote). We verify
it compiles and rely on integration testing. The `detect_remote_url` tests
already validate the git2 remote plumbing.

**Step 4: Commit**

```
feat(workspace): add git push_branch helper
```

---

### Task 19: Add SOUL.md workspace discipline rules

**Files:**
- Modify: `backend/soul/SOUL.md`

**Step 1: Add Workspace Discipline section**

Insert between "Security Rules" and "Safety Guardrails":

```markdown
## Workspace Discipline

- All file modifications, git operations, and artifact creation must happen
  within an active workspace context. If no workspace is resolved for the
  current conversation, ask the user to select or create one before proceeding.
- Never modify files outside the workspace root or linked repo paths.
- Use git worktrees for code changes --- never modify the user's current branch
  directly. Create a worktree, do the work, propose the result.
- Track all meaningful outputs as artifacts with proper provenance
  (conversation, task, parent artifact).
- Before destructive filesystem operations, create a snapshot.
- Casual conversation (questions, explanations, brainstorming) does not
  require a workspace. Workspace enforcement activates only when producing
  persistent artifacts.
```

**Step 2: Commit**

```
feat(soul): add workspace discipline rules to SOUL.md
```

---

### Task 20: Version bumps and final verification

**Files:**
- Modify: `backend/crates/sober-core/Cargo.toml` (0.5.0 -> 0.6.0)
- Modify: `backend/crates/sober-db/Cargo.toml` (0.4.0 -> 0.5.0)
- Modify: `backend/crates/sober-workspace/Cargo.toml` (0.1.0 -> 0.2.0)

**Step 1: Bump versions**

MINOR bumps for feat/ branch (ABSOLUTE RULE).

**Step 2: Run full verification**

```bash
cargo test --workspace -q
cargo clippy -q -- -D warnings
```

**Step 3: Commit**

```
chore: bump sober-core 0.6.0, sober-db 0.5.0, sober-workspace 0.2.0
```

---

## Phase 2 Acceptance Criteria

- [ ] `CallerContext` has `workspace_id: Option<WorkspaceId>` field
- [ ] `Conversation` domain type has `workspace_id: Option<WorkspaceId>`
- [ ] Migration adds `workspace_id` column to `conversations` table
- [ ] `ConversationRepo::create()` accepts `workspace_id` parameter
- [ ] `PgConversationRepo` stores and retrieves `workspace_id`
- [ ] Conversation API routes pass through `workspace_id`
- [ ] `detect_remote_url()` auto-discovers origin or first remote
- [ ] `push_branch()` pushes via SSH credentials from environment
- [ ] SOUL.md has "Workspace Discipline" section
- [ ] Version bumps: sober-core 0.6.0, sober-db 0.5.0, sober-workspace 0.2.0
- [ ] `cargo test --workspace -q` passes
- [ ] `cargo clippy -q -- -D warnings` clean
