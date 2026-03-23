# #019 Plan E: Plugin Artifact Integration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Store generated WASM plugins as workspace artifacts (content-addressed blobs) instead of loose filesystem files. Track them via `ArtifactRepo` for versioning, deduplication, and lifecycle management.

**Problem:** Currently `GeneratePluginTool` writes raw files to `.sober/plugins/<name>/` and stores the filesystem path in the plugin's `config.wasm_path`. This means:
- No content-addressing or deduplication
- No artifact versioning — regenerating overwrites without history
- No integration with the artifact lifecycle (state transitions, cleanup)
- `PluginHost::load()` reads from a loose path that could be deleted or moved

**Architecture:** Generated WASM bytes are stored in `BlobStore` (SHA-256 addressed). An `Artifact` record is created linking the blob to the workspace. The plugin's `config` stores the `blob_ref` instead of a filesystem path. `PluginHost::load()` resolves WASM bytes from the blob store.

**Prerequisites:** Plan B (WASM Runtime) and Plan C (Integration) implemented.

---

## Existing infrastructure

| Component | Location | Status |
|-----------|----------|--------|
| `BlobStore` | `sober-workspace/src/blob.rs` | Working — `put(bytes) -> blob_ref`, `get(blob_ref) -> bytes` |
| `ArtifactRepo` | `sober-core/src/types/repo.rs` | Working — CRUD for artifact records |
| `CreateArtifact` | `sober-core/src/types/input.rs` | Working — input type for artifact creation |
| `ArtifactState` | `sober-core/src/types/enums.rs` | Working — `Draft`, `Published`, `Archived`, `Deleted` |
| `ToolBootstrap` | `sober-agent/src/tools/bootstrap.rs` | Has `blob_store: Arc<BlobStore>` already |

---

## Task 1: Wire BlobStore and ArtifactRepo into GeneratePluginTool

**Files:**
- Modify: `sober-agent/src/tools/generate_plugin.rs`
- Modify: `sober-agent/src/tools/bootstrap.rs`

- [ ] **Step 1:** Add `blob_store: Arc<BlobStore>` and artifact repo access to `GeneratePluginTool`
- [ ] **Step 2:** Update `ToolBootstrap::build()` to pass `blob_store` when creating the tool
- [ ] **Step 3:** Build, verify compilation

---

## Task 2: Store WASM bytes in BlobStore

**Files:**
- Modify: `sober-agent/src/tools/generate_plugin.rs`

- [ ] **Step 1:** After WASM generation, store bytes via `blob_store.put(&generated.wasm_bytes)`
- [ ] **Step 2:** Store `blob_ref` in plugin config instead of `wasm_path`:
  ```json
  {
    "blob_ref": "sha256:abc123...",
    "manifest_toml": "...",
    "source_path": ".sober/plugins/<name>/src_lib.rs"
  }
  ```
- [ ] **Step 3:** Still save the source and manifest to `.sober/plugins/<name>/` for human inspection — but the WASM binary comes from the blob store
- [ ] **Step 4:** Create an `Artifact` record:
  ```rust
  CreateArtifact {
      workspace_id,
      name: format!("plugin:{name}"),
      artifact_type: "wasm_plugin",
      blob_ref,
      state: ArtifactState::Published,
      metadata: json!({ "plugin_name": name, "version": "0.1.0" }),
      created_by: user_id,
      conversation_id,
  }
  ```

---

## Task 3: Load WASM from BlobStore in PluginHost

**Files:**
- Modify: `sober-plugin/src/manager.rs` (wasm_tools method)

- [ ] **Step 1:** In `wasm_tools()`, check for `config.blob_ref` first, fall back to `config.wasm_path`
- [ ] **Step 2:** If `blob_ref` present, resolve bytes via `blob_store.get(blob_ref)`
- [ ] **Step 3:** Pass `blob_store: Option<Arc<BlobStore>>` through `PluginManager`
- [ ] **Step 4:** Backward compatible — existing plugins with `wasm_path` still work

---

## Task 4: Plugin update / regeneration versioning

**Files:**
- Modify: `sober-agent/src/tools/generate_plugin.rs`

- [ ] **Step 1:** When regenerating an existing plugin, create a new blob (don't overwrite)
- [ ] **Step 2:** Update the plugin's `config.blob_ref` to the new blob
- [ ] **Step 3:** Transition old artifact to `Archived` state, create new `Published` artifact
- [ ] **Step 4:** Evict the old `PluginHost` from the WASM cache via `plugin_manager.evict_wasm_host()`

---

## Task 5: Verification

- [ ] **Step 1:** `cargo build --workspace -q --exclude sober-web`
- [ ] **Step 2:** `cargo test --workspace -q --exclude sober-web`
- [ ] **Step 3:** `cargo clippy --workspace -q --exclude sober-web -- -D warnings`
- [ ] **Step 4:** End-to-end: generate WASM plugin → verify blob exists in store → load plugin from blob → execute tool

---

## Acceptance Criteria

- [ ] Generated WASM bytes stored in BlobStore (content-addressed)
- [ ] Artifact record created for each generated plugin
- [ ] Plugin config uses `blob_ref` instead of `wasm_path`
- [ ] `PluginHost` loads from blob store when `blob_ref` present
- [ ] Backward compatible — `wasm_path` still works for manually placed plugins
- [ ] Regeneration creates new blob + artifact, archives old
- [ ] Source and manifest files still saved to `.sober/plugins/` for inspection
