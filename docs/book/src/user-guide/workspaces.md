# Workspaces

A workspace is an isolated environment where the agent performs file-based work on your behalf. Each workspace has its own directory, git repository, blob store, and configuration. The agent uses workspaces to run shell commands, create artifacts, manage snapshots, and maintain project-specific context.

Every conversation has its own dedicated working directory inside the workspace, named after the conversation UUID.

---

## What Workspaces Are For

When you start a conversation where the agent will write code, run scripts, or produce documents, it operates inside a workspace. Workspaces provide:

- **Isolation** — each project's files live in a dedicated directory, separate from other users and workspaces.
- **Persistence** — files, artifacts, and snapshots survive individual conversations.
- **Version control** — git is available within the workspace (via `libgit2`) for committing changes and tracking history.
- **Reproducibility** — snapshots let the agent capture and restore the workspace state at any point.

---

## Directory Layout

Workspaces are stored under the workspace root, which is configurable:

```
# Production default
/var/lib/sober/workspaces/

# Development default (can be overridden in config.toml)
~/.sober/workspaces/
```

Each workspace is a directory (the workspace root). The conversation UUID, not a user UUID, forms the per-conversation subdirectory name:

```
/var/lib/sober/workspaces/
  550e8400-e29b-41d4-a716-446655440000/    # workspace root (workspace UUID)
    .sober/                                 # Sõber internals for this workspace
      config.toml                           # workspace-level settings
      state.json                            # agent state (observations)
      proposals/                            # proposed soul/config changes
      traces/                               # execution traces
      snapshots/                            # tar archives created by snapshot tools
        pre-dangerous-20260323T120000.tar.gz
        my-checkpoint-20260323T130000.tar.gz
      worktrees/                            # git worktrees
      blobs/                                # content-addressed blob store
        <prefix>/
          <sha256-hash>
    .git/                                   # git repository managed by libgit2
    <conversation-uuid>/                    # per-conversation working directory
      ...                                   # agent-created files
```

The workspace root is set separately for the agent and scheduler processes:

```toml
[agent]
workspace_root = "/var/lib/sober/workspaces"

[scheduler]
workspace_root = "/var/lib/sober/workspaces"
```

---

## `.sober/` Directory

Each workspace contains a `.sober/` directory that Sõber manages for internal state and configuration.

### `config.toml`

A per-workspace configuration file created automatically when the workspace is initialised. This can override settings like the LLM model, sandbox profile, and snapshot retention limits. The format mirrors the main `config.toml` but only workspace-relevant sections are honoured.

### `soul.md` (optional)

An optional workspace-level soul layer. If present, it is appended to the agent's personality for conversations in this workspace. The workspace layer is **additive only** — it can adjust communication style or add domain-specific context, but it cannot override ethical boundaries or security rules established in the base soul.

Resolution order:

```
sober-mind/instructions/soul.md   (base — compiled into the binary)
  └── ~/.sober/soul.md             (user-level override)
       └── .sober/soul.md          (workspace-level, additive only)
```

### `state.json`

Stores agent observations and ephemeral workspace state. Managed automatically by the agent — you do not normally need to edit this file.

---

## Conversation Directories

Within each workspace, every conversation gets its own subdirectory named after the conversation UUID. This is the working directory that `shell` tool commands operate in by default. The agent creates files, runs builds, and stores output here.

```
/var/lib/sober/workspaces/<workspace-id>/<conversation-id>/
```

---

## Git Integration

Workspaces include a git repository managed via `libgit2` (the `git2` Rust crate — no `git` binary required at runtime). The agent can:

- Check out branches, stage files, and commit changes.
- Read commit history and diffs.
- Work with remote repositories via the `remote` module.

Git operations are available through the `shell` tool (using the `git` binary in the sandbox) or through agent-level workspace operations that call `libgit2` directly.

---

## Blob Storage

Large or binary content is stored content-addressed in the workspace blob store rather than inline in the database. The `BlobStore` computes a SHA-256 hash of the content and stores the bytes at a path derived from the hash. This ensures deduplication — identical content is stored once regardless of how many artifacts reference it.

Blob-backed artifacts (created with `storage_type: "blob"`) have their hash recorded in the `blob_key` field and their bytes stored in:

```
<workspace-root>/.sober/blobs/<hash-prefix>/<hash>
```

Generated WASM plugins are also stored as blobs, ensuring they survive workspace directory changes.

---

## Snapshots

The snapshot manager creates and restores tar archives of conversation directories. Snapshots are automatically triggered:

- Before `Dangerous`-classified shell commands (when `auto_snapshot` is enabled in the agent config).
- Before any `restore_snapshot` operation (a safety snapshot of the current state is always created first).

Snapshots are tracked as `Snapshot`-kind artifacts in the database, making them discoverable via `list_snapshots` and restorable via `restore_snapshot`. The number of retained snapshots per workspace is configurable (default: 10).

---

## Creating a Workspace

Workspaces are created by the agent when needed. You can also have the agent create one explicitly by asking it to set up a project. The workspace ID is a UUID assigned at creation time.

To point the agent at an existing directory, use the `soberctl` runtime tool or the API directly — the workspace registration creates the database record and `.sober/` scaffold.
