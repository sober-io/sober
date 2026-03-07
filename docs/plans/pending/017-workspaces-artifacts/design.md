# 017 --- Workspaces, Worktrees & Artifact Management

**Date:** 2026-03-06

---

## Overview

Workspaces are the top-level container for user+agent collaboration. A workspace
holds multiple repos (managed or linked), non-git artifacts, and per-workspace
agent state. All artifacts are scoped to user or group for privacy isolation.

---

## 1. Core Concepts

### Workspace

A general-purpose collaboration space. Not tied to a single repo --- can contain
multiple independent repos, documents, configs, and generated files.

- Owned by a **user** (via `user_id`). Group ownership can be added later if needed.
- Multiple users collaborate only through group-scoped workspaces
- Each workspace has a `.sober/` directory for agent state

### Repos

Git repositories registered within a workspace. Two modes:

- **Managed** --- cloned/created under the workspace root directory. Path is
  relative to workspace root.
- **Linked** --- references an external path (e.g., `~/Projects/my-app`). Path
  is absolute. The agent works in-place, never copies or symlinks the repo.

### Worktrees

Git worktrees the agent creates for parallel task execution. Tracked in the DB,
stored under the workspace's `.sober/worktrees/` directory. Ephemeral ---
cleaned up by the scheduler when stale.

### Artifacts

Any output produced by the agent or user within a workspace. Tracked in
PostgreSQL with provenance, state, and relationships. Content lives in git,
blob storage, or inline depending on type.

---

## 2. Filesystem Layout

### System-level

```
/var/lib/sober/
+-- agent/                              # agent's global workspace (git-tracked)
|   +-- .git/
|   +-- soul/
|   |   +-- SOUL.md                     # base soul (canonical copy)
|   +-- proposals/                      # global proposals (base soul, system skills)
|   +-- skills/                         # system-wide skill drafts
|   +-- audit/                          # self-evolution audit logs
|
+-- workspaces/
|   +-- user_{user_id}/
|   |   +-- my-project/                 # a workspace
|   |   |   +-- .sober/                 # per-workspace agent state
|   |   |   |   +-- config.toml         # user preferences (user-editable)
|   |   |   |   +-- state.json          # agent observations (agent-managed)
|   |   |   |   +-- soul.md             # workspace SOUL.md layer
|   |   |   |   +-- proposals/          # pending changes awaiting review
|   |   |   |   +-- traces/             # execution logs (pruned periodically)
|   |   |   |   +-- worktrees/          # git worktree checkouts
|   |   |   |       +-- {worktree_id}/
|   |   |   |       +-- {worktree_id}/
|   |   |   +-- repo-a/                 # managed repo
|   |   |   +-- repo-b/                 # managed repo
|   |   |   +-- docs/                   # non-git artifacts
|   |   +-- another-project/
|   |
|   +-- group_{group_id}/              # group-scoped workspaces
|       +-- team-project/
|           +-- .sober/
|           +-- ...
|
+-- blobs/                              # content-addressed blob storage
|   +-- {sha256-prefix}/{sha256}
|
+-- tmp/                                # truly ephemeral scratch space
```

### User home

```
~/.sober/
+-- SOUL.md                # user-level soul layer
+-- config.toml            # user-level agent preferences (global defaults)
+-- state.json             # agent's per-user learned state (cross-workspace)
```

### Naming convention

**Sober** is the brand name (docs, UI, branding). **`sober`** is the technical
identifier (filesystem paths, binary names, crate names, config keys). No
Unicode in paths --- `~/.sober/` not `~/.sober/`.

---

## 3. Config Format

Split by ownership:

| File | Format | Owner | Purpose |
|------|--------|-------|---------|
| `config.toml` | TOML | User | Preferences, overrides. Agent reads, never writes. |
| `state.json` | JSON | Agent | Learned observations, cached state. User can read, shouldn't edit. |
| `soul.md` | Markdown | User | Workspace-level SOUL.md layer. Agent can propose changes. |

The `config.toml` file includes a `[sandbox]` section for execution sandbox
policy. See 009-sober-sandbox design for profile and override configuration.

TOML for human-edited config (supports comments, readable). JSON for
agent-managed state (trivial to serialize/deserialize programmatically).

---

## 4. SOUL.md Resolution Chain Integration

The workspace's `.sober/soul.md` is the workspace layer in the SOUL.md
resolution chain:

```
backend/soul/SOUL.md           (base --- shipped with the system)
  +-- ~/.sober/SOUL.md          (user-level overrides/extensions)
       +-- .sober/soul.md       (workspace-level, if conversation has workspace)
```

When a conversation is associated with a workspace, prompt assembly loads that
workspace's soul layer automatically. Conversations without a workspace
association use only base + user layers.

Prompt assembly receives workspace context:

```rust
pub struct PromptContext {
    pub user_id: UserId,
    pub workspace_id: Option<WorkspaceId>,
    pub conversation_id: ConversationId,
    pub trigger: TriggerSource,
}
```

### Override rules (unchanged from sober-mind design)

| Layer | Override rules |
|-------|---------------|
| Base | Foundation --- defines everything |
| User (`~/.sober/`) | Full override of base. User controls their instance. |
| Workspace (`.sober/soul.md`) | Additive only. Can override style and domain emphasis. Cannot contradict ethical boundaries or security rules. |

---

## 5. Agent State: Global vs Per-Workspace

### Global agent state (`/var/lib/sober/agent/`)

Git-tracked. Contains state that doesn't belong to any user or workspace:

- Base SOUL.md evolution proposals
- System-wide skill drafts
- Cross-user pattern analysis (anonymized)
- Key rotation logs
- Self-assessment results

### Per-workspace agent state (`.sober/`)

Colocated with the workspace. Contains state specific to that project:

- Learned patterns ("this repo uses conventional commits")
- Workspace-specific proposals
- Execution traces
- Active worktree checkouts

This split means:
- Workspace export/backup is self-contained (`.sober/` travels with it)
- Global state has a dedicated home, not scattered across workspaces
- Workspace deletion doesn't affect system-wide agent state

---

## 6. Database Schema

### Workspaces

```sql
CREATE TYPE workspace_state AS ENUM (
    'active',
    'archived',
    'deleted'
);

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
```

### Workspace Repos

```sql
CREATE TABLE workspace_repos (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    path            TEXT NOT NULL,           -- relative (managed) or absolute (linked)
    is_linked       BOOLEAN NOT NULL DEFAULT false,
    remote_url      TEXT,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, path)
);
```

### Worktrees

```sql
CREATE TYPE worktree_state AS ENUM (
    'active',
    'stale',
    'removing'
);

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
```

### Artifacts

```sql
CREATE TYPE artifact_kind AS ENUM (
    'code_change',
    'document',
    'proposal',
    'snapshot',
    'trace'
);

CREATE TYPE artifact_state AS ENUM (
    'draft',
    'proposed',
    'approved',
    'rejected',
    'archived'
);

CREATE TABLE artifacts (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id),
    user_id         UUID NOT NULL REFERENCES users(id),
    kind            artifact_kind NOT NULL,
    state           artifact_state NOT NULL DEFAULT 'draft',
    title           TEXT NOT NULL,
    description     TEXT,

    -- Location
    storage_type    TEXT NOT NULL,           -- 'git', 'blob', 'inline'
    git_repo        TEXT,                    -- repo path within workspace (if git)
    git_ref         TEXT,                    -- commit SHA or branch (if git)
    blob_key        TEXT,                    -- content-addressed key (if blob)
    inline_content  TEXT,                    -- small artifacts stored directly

    -- Provenance
    created_by      UUID REFERENCES users(id),  -- NULL = agent-created
    conversation_id UUID REFERENCES conversations(id),
    task_id         UUID,
    parent_id       UUID REFERENCES artifacts(id),

    -- Review
    reviewed_by     UUID REFERENCES users(id),
    reviewed_at     TIMESTAMPTZ,

    -- Extensible metadata
    metadata        JSONB NOT NULL DEFAULT '{}',

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Artifact Relations

```sql
CREATE TYPE artifact_relation AS ENUM (
    'spawned_by',
    'supersedes',
    'references',
    'implements'
);

CREATE TABLE artifact_relations (
    source_id       UUID NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    target_id       UUID NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    relation        artifact_relation NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_id, target_id, relation)
);
```

### Indexes

```sql
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

---

## 7. Artifact Visibility

Fixed tiers by artifact kind --- not configurable.

| Kind | Visible to | Rationale |
|------|-----------|-----------|
| `code_change` | Workspace members | Their code |
| `document` | Workspace members | They requested it |
| `proposal` | Workspace owner + admins | Needs review |
| `snapshot` | Workspace owner + admins | Backup/export |
| `trace` | Admins only | Debugging, may leak cross-context reasoning |

---

## 8. Group Workspace Access Control

Group roles are the default access control for group workspaces. Per-workspace
overrides are available when needed.

- **Group admin** --- full access to all group workspaces
- **Group member** --- read/write access to group workspaces
- Per-workspace restrictions can further limit a member's access

This uses the existing `user_roles` table with group membership. No new tables
needed.

---

## 9. Workspace Lifecycle

### Creation

1. DB: insert `workspaces` row with `user_id` for ownership
2. Filesystem: create workspace root directory + `.sober/` with template files
3. Register any initial repos (clone managed, register linked)

### Archival (soft delete)

1. All active worktrees cleaned up (state -> `removing`, filesystem deleted)
2. Workspace state set to `archived`, `archived_at` set
3. Filesystem stays intact (read-only)
4. Agent stops observing/learning about this workspace

### Deletion (hard delete)

Requires archival first. After a configurable grace period (default 30 days),
or via admin force-delete:

1. Managed repos and `.sober/` removed from disk
2. Linked repos untouched (only DB reference removed)
3. DB records soft-deleted (`state = 'deleted'`, `deleted_at` set)
4. Blobs retained for 90 days after deletion (audit trail continuity)
5. Scheduler prunes unreferenced blobs past retention window

### Restoration

Archived workspaces can be restored to active. Deleted workspaces cannot be
restored (filesystem content is gone).

---

## 10. Linked Repo Discovery

When the agent needs to find the workspace for a linked external repo:

```sql
SELECT w.id, w.user_id, wr.id AS repo_id
FROM workspace_repos wr
JOIN workspaces w ON wr.workspace_id = w.id
WHERE wr.path = $1
  AND wr.is_linked = true
  AND w.user_id = $2;
```

User filtering disambiguates if the same external path is linked into multiple
workspaces by different users.

For managed repos, the workspace is found by traversing up from the repo path
to the workspace root.

---

## 11. Worktree Conflict Resolution

In group workspaces, the `UNIQUE(repo_id, branch)` constraint on the
`worktrees` table prevents two users (or user + agent) from creating worktrees
on the same branch of the same repo.

**Behavior:** reject with a clear error indicating who holds the existing
worktree. No queuing.

---

## 12. Impact on Existing Architecture

### New tables

- `workspaces`
- `workspace_repos`
- `worktrees`
- `artifacts`
- `artifact_relations`

### New filesystem paths

- `/var/lib/sober/agent/` --- agent global workspace
- `/var/lib/sober/workspaces/` --- all user/group workspaces
- `/var/lib/sober/blobs/` --- content-addressed blob storage
- `~/.sober/` --- user-level config and soul layer

### Modified designs

- **sober-mind (010)** --- `PromptContext` gains `workspace_id: Option<WorkspaceId>`.
  Resolution chain uses `.sober/soul.md` from workspace when present.
- **sober-agent (012)** --- agent loop receives workspace context for
  conversations associated with a workspace.
- **sober-scheduler (016)** --- stale worktree cleanup job. Blob retention
  pruning job.
- **sober-core (003)** --- `WorkspaceId` is already defined in sober-core
  (decided in C13). Additional new types: `WorkspaceRepoId`, `WorktreeId`,
  `ArtifactId`.
- **ARCHITECTURE.md** --- update `~/.sober/` paths (was `~/.sober/`), add
  workspace and artifact concepts to system architecture.

### Crate placement (decided)

No new `sober-workspace` crate. Split across existing crates:

- **`sober-core`** — Workspace types (`WorkspaceId`, `WorkspaceRepoId`, `WorktreeId`,
  `ArtifactId`), enums (`WorkspaceState`, `ArtifactKind`, `ArtifactState`,
  `WorktreeState`, `ArtifactRelation`), and config structs.
- **`sober-agent`** — Workspace operations: CRUD, repo management, worktree lifecycle,
  artifact tracking, filesystem operations. The agent already owns task orchestration
  and is the natural home for workspace-aware operations.

This avoids adding a 13th crate for what is fundamentally agent operational logic
with shared types.
