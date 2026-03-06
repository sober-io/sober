# 016 --- Workspaces, Worktrees & Artifact Management: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add workspace, worktree, and artifact management --- the collaboration layer where users and the agent produce, version, and track work artifacts.

**Architecture:** No new crate — workspace operations split across existing crates. Types and enums live in `sober-core`. Workspace CRUD, repo management, worktree lifecycle, artifact tracking, and blob storage live in `sober-agent` (the natural home for workspace-aware operations). Database schema is added via sqlx migrations. Filesystem operations create and manage the `/var/lib/sober/` directory tree.

**Tech Stack:** Rust, sqlx (PostgreSQL), tokio (async fs), serde/serde_json, toml (config parsing), sha2 (blob hashing), git2 (worktree management), thiserror

**Depends on:** 002 (project skeleton), 003 (sober-core types + config)

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
//! Workspace, worktree, and artifact management for the Sober agent system.
//!
//! This crate owns workspace CRUD, repo registration, worktree lifecycle,
//! artifact tracking, and blob storage.

pub mod error;
pub mod workspace;
pub mod repo;
pub mod worktree;
pub mod artifact;
pub mod blob;
pub mod fs;

pub use error::WorkspaceError;
```

Create empty module files for each submodule (each containing just a doc comment):
- `backend/crates/sober-workspace/src/workspace.rs`
- `backend/crates/sober-workspace/src/repo.rs`
- `backend/crates/sober-workspace/src/worktree.rs`
- `backend/crates/sober-workspace/src/artifact.rs`
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

Content-addressed blob storage under `/var/lib/sober/blobs/`.

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

**Files:**
- Modify: `backend/crates/sober-workspace/src/workspace.rs`
- Test: `backend/crates/sober-workspace/tests/workspace_db.rs` (integration test, requires DB)

**Step 1: Write failing integration test**

Create `backend/crates/sober-workspace/tests/workspace_db.rs`:

```rust
//! Integration tests for workspace DB operations.
//! Requires a running PostgreSQL instance with migrations applied.
//! Set DATABASE_URL env var to run.

use sober_core::*;
use sober_workspace::workspace::WorkspaceRepo as WorkspaceRepository;
use sqlx::PgPool;

async fn setup_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&url).await.expect("Failed to connect to DB")
}

#[sqlx::test]
async fn create_and_get_workspace(pool: PgPool) {
    let repo = WorkspaceRepository::new(pool);
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
    let repo = WorkspaceRepository::new(pool);
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
    let repo = WorkspaceRepository::new(pool);
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

Run: `cargo test -p sober-workspace --test workspace_db`
Expected: FAIL --- `WorkspaceRepository` not defined

**Step 3: Implement workspace DB operations**

In `backend/crates/sober-workspace/src/workspace.rs`:

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

/// Repository for workspace DB operations.
pub struct WorkspaceRepo {
    pool: PgPool,
}

impl WorkspaceRepo {
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

Run: `cargo test -p sober-workspace --test workspace_db`
Expected: PASS (requires DATABASE_URL)

**Step 5: Commit**

```bash
git add backend/crates/sober-workspace/src/workspace.rs backend/crates/sober-workspace/tests/
git commit -m "feat(workspace): add workspace CRUD with lifecycle state machine"
```

---

## Task 8: Repo Management (DB Layer)

**Files:**
- Modify: `backend/crates/sober-workspace/src/repo.rs`
- Test: `backend/crates/sober-workspace/tests/repo_db.rs`

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
git add backend/crates/sober-workspace/src/repo.rs backend/crates/sober-workspace/tests/repo_db.rs
git commit -m "feat(workspace): add repo registration and linked path discovery"
```

---

## Task 9: Worktree Lifecycle (DB + Git)

**Files:**
- Modify: `backend/crates/sober-workspace/src/worktree.rs`
- Test: `backend/crates/sober-workspace/tests/worktree_db.rs` (integration)
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
git add backend/crates/sober-workspace/src/worktree.rs backend/crates/sober-workspace/tests/worktree_db.rs
git commit -m "feat(workspace): add worktree lifecycle with git integration"
```

---

## Task 10: Artifact Tracking (DB Layer)

**Files:**
- Modify: `backend/crates/sober-workspace/src/artifact.rs`
- Test: `backend/crates/sober-workspace/tests/artifact_db.rs`

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
git add backend/crates/sober-workspace/src/artifact.rs backend/crates/sober-workspace/tests/artifact_db.rs
git commit -m "feat(workspace): add artifact tracking with state machine and relations"
```

---

## Task 11: Integration --- Wire Workspace into sober-core Config

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

**Step 1: Add workspace config section**

Add to `AppConfig`:

```rust
pub struct WorkspaceSystemConfig {
    /// Root directory for all workspace data.
    /// Default: /var/lib/sober
    pub data_root: PathBuf,
    /// Blob retention period after workspace deletion (days).
    /// Default: 90
    pub blob_retention_days: u32,
    /// Workspace archive grace period before hard delete (days).
    /// Default: 30
    pub archive_grace_period_days: u32,
    /// Worktree stale threshold (hours).
    /// Default: 24
    pub worktree_stale_hours: u32,
}
```

Load from env vars:
- `SOBER_DATA_ROOT` (default: `/var/lib/sober`)
- `SOBER_BLOB_RETENTION_DAYS` (default: `90`)
- `SOBER_ARCHIVE_GRACE_PERIOD_DAYS` (default: `30`)
- `SOBER_WORKTREE_STALE_HOURS` (default: `24`)

**Step 2: Test config loading**

```rust
#[test]
fn workspace_config_defaults() {
    // No env vars set for workspace config
    let config = AppConfig::load_from_env().unwrap();
    assert_eq!(config.workspace.data_root, PathBuf::from("/var/lib/sober"));
    assert_eq!(config.workspace.blob_retention_days, 90);
}
```

**Step 3: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add workspace system configuration"
```

---

## Task 12: Update ARCHITECTURE.md and Existing Designs

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/plans/pending/009-sober-mind/design.md`
- Modify: `docs/plans/pending/003-sober-core/design.md`

**Step 1: Update ARCHITECTURE.md**

- Replace all `~/.sõber/` references with `~/.sober/`
- Add `sober-workspace` to the crate map table
- Add workspace concept to the system architecture diagram
- Document the filesystem layout under a new "Workspace & Artifact System" section
- Update the crate dependency flow to include `sober-workspace`

**Step 2: Update sober-mind design (009)**

- Change `~/.sõber/SOUL.md` to `~/.sober/SOUL.md`
- Change `./.sõber/SOUL.md` to `.sober/soul.md`
- Note that `PromptContext` gains `workspace_id: Option<WorkspaceId>`

**Step 3: Update sober-core design (003)**

- `WorkspaceId` is already defined in sober-core (decided in C13). Add `WorkspaceRepoId`, `WorktreeId`, `ArtifactId` to the ID types list
- Add `WorkspaceState`, `WorktreeState`, `ArtifactKind`, `ArtifactState`, `ArtifactRelation` to the enums list
- Add `toml` to the dependencies table

**Step 4: Commit**

```bash
git add ARCHITECTURE.md docs/plans/pending/009-sober-mind/design.md docs/plans/pending/003-sober-core/design.md
git commit -m "docs(arch): update for workspace system, fix sober path naming"
```

---

## Task 13: Clippy, Docs, Final Verification

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
- [ ] `ARCHITECTURE.md` updated with `~/.sober/` paths and workspace crate
- [ ] `cargo test --workspace` passes
- [ ] All public items in `sober-workspace` have doc comments
