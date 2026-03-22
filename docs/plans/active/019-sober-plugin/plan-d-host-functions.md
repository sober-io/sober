# #019 Plan D: WASM Host Function Implementations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire all stubbed WASM host functions to their backing services. Currently only `log`, `kv` (in-memory), `network`, and `metrics` work. This plan makes all 11 capabilities functional and backs KV with the database.

**Architecture:** `HostContext` gains service handles (`Arc<dyn PluginRepo>`, `Arc<dyn SecretRepo>`, etc.) and a `tokio::runtime::Handle` for bridging async services into Extism's synchronous host function context. The handle is captured in `PluginTool::execute()` before `spawn_blocking` and injected into the context.

**Tech Stack:** Rust, Extism, tokio, sober-core/db/crypto/llm/memory/workspace.

**Prerequisites:** Plan B (WASM Runtime) and Plan C (Integration) must be implemented.

---

## Key Design Decision: Async Bridge

Extism host functions are **synchronous**. All backing services (repos, LLM, memory) are **async**. The bridge:

1. `PluginTool::execute()` captures `tokio::runtime::Handle::current()` **before** calling `spawn_blocking`
2. The handle is stored in `HostContext` (or passed alongside it)
3. Host functions call `handle.block_on(async_operation)` to run async code synchronously

This is safe because `spawn_blocking` threads are separate from the tokio worker threads — `block_on` won't deadlock.

```rust
// In PluginTool::execute():
let rt_handle = tokio::runtime::Handle::current();
tokio::task::spawn_blocking(move || {
    // Store handle in host context before calling plugin
    host.set_runtime_handle(rt_handle);
    host.call_tool(&tool_name, input)
})

// In host function:
fn host_kv_get_impl(..., user_data: UserData<HostContext>) -> ... {
    let ctx = user_data.get()?.lock()?;
    let handle = ctx.runtime_handle.as_ref().ok_or("no runtime")?;
    let result = handle.block_on(ctx.plugin_repo.get_kv_data(ctx.plugin_id, &key))?;
    // ...
}
```

---

## Files

### Modified files

| File | Change |
|------|--------|
| `sober-plugin/src/host_fns.rs` | Expand HostContext, implement all host functions |
| `sober-plugin/src/host.rs` | Accept service handles in `PluginHost::load()` |
| `sober-plugin/src/tool.rs` | Capture runtime handle, pass to HostContext |
| `sober-plugin/src/manager.rs` | Pass service handles when loading WASM hosts |
| `sober-plugin/Cargo.toml` | Add deps: `sober-db`, `sober-crypto`, `sober-llm`, `sober-memory` |
| `sober-agent/src/tools/bootstrap.rs` | Pass service handles to PluginManager |
| `sober-agent/src/main.rs` | Wire service handles through to PluginManager |

---

## Task 1: Expand HostContext with service handles and runtime bridge

**Files:** `sober-plugin/src/host_fns.rs`, `sober-plugin/src/host.rs`, `sober-plugin/src/tool.rs`

- [ ] **Step 1:** Add `tokio::runtime::Handle` to `HostContext` (as `Option<Handle>`)
- [ ] **Step 2:** Add service fields to `HostContext`:
  ```rust
  pub struct HostContext {
      pub plugin_id: PluginId,
      pub capabilities: Vec<Capability>,
      pub kv_store: Arc<Mutex<HashMap<String, serde_json::Value>>>,
      // Async bridge
      pub runtime_handle: Option<tokio::runtime::Handle>,
      // Service handles (Option for graceful degradation)
      pub plugin_repo: Option<Arc<dyn PluginRepoSync>>,
      pub secret_repo: Option<Arc<dyn SecretRepoSync>>,
      pub mek: Option<Arc<sober_crypto::envelope::Mek>>,
      pub llm_engine: Option<Arc<dyn sober_llm::LlmEngine>>,
      pub memory_store: Option<Arc<sober_memory::MemoryStore>>,
      pub user_id: Option<UserId>,
  }
  ```
  Note: `PluginRepo` and `SecretRepo` use RPITIT (not object-safe). We need thin wrapper types (`PluginRepoSync`, `SecretRepoSync`) that erase the `impl Future` returns into boxed futures. OR pass concrete `PgPluginRepo`/`PgSecretRepo` directly. Concrete types are simpler — `HostContext` becomes generic or uses `Box<dyn Any>`.

  **Simplest approach:** Make `HostContext` hold `Arc<dyn Any + Send + Sync>` for repos and downcast. Or use `sqlx::PgPool` directly and run queries inline — avoids the trait object problem entirely.

- [ ] **Step 3:** Update `PluginHost::load()` to accept a `HostContextBuilder` or additional params
- [ ] **Step 4:** Update `PluginTool::execute()` to capture `Handle::current()` and inject into host context before `spawn_blocking`
- [ ] **Step 5:** Add helper `fn block_on_async<F: Future>(&self, f: F) -> F::Output` to HostContext
- [ ] **Step 6:** Build, test

---

## Task 2: DB-backed KV (replace in-memory HashMap)

**Files:** `sober-plugin/src/host_fns.rs`

- [ ] **Step 1:** In `host_kv_get_impl`, use `handle.block_on(plugin_repo.get_kv_data(plugin_id, key))`
- [ ] **Step 2:** In `host_kv_set_impl`, use `handle.block_on(plugin_repo.set_kv_data(plugin_id, key, value))`
- [ ] **Step 3:** In `host_kv_delete_impl`, implement via `set_kv_data` with null/remove pattern (or add `delete_kv_data` to PluginRepo)
- [ ] **Step 4:** In `host_kv_list_impl`, query the `plugin_kv_data` table's JSONB keys
- [ ] **Step 5:** Keep in-memory HashMap as fallback when `plugin_repo` is None (for tests)
- [ ] **Step 6:** Tests: verify KV persists across plugin reloads

---

## Task 3: Implement secret_read

**Files:** `sober-plugin/src/host_fns.rs`, `sober-plugin/Cargo.toml`

- [ ] **Step 1:** Add `sober-crypto` dependency
- [ ] **Step 2:** In `host_read_secret_impl`:
  - Get `SecretRepo` and `Mek` from context
  - `handle.block_on(secret_repo.get_secret_by_name(user_id, None, name))`
  - Unwrap DEK: `mek.unwrap_dek(stored_dek)`, then `dek.decrypt(encrypted_data)`
  - Return decrypted value as string
- [ ] **Step 3:** Tests with mock repo

---

## Task 4: Implement tool_call

**Files:** `sober-plugin/src/host_fns.rs`

This is the most architecturally complex capability — a plugin calling other tools creates a recursive dependency (ToolRegistry → PluginTool → HostContext → ToolRegistry).

- [ ] **Step 1:** Add `tool_executor: Option<Arc<dyn ToolExecutor>>` to HostContext where:
  ```rust
  pub trait ToolExecutor: Send + Sync {
      fn execute(&self, tool_name: &str, input: serde_json::Value)
          -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
  }
  ```
- [ ] **Step 2:** Implement `ToolExecutor` in `sober-agent` wrapping the `ToolRegistry`
- [ ] **Step 3:** In `host_call_tool_impl`, use `handle.block_on(executor.execute(tool, input))`
- [ ] **Step 4:** Add recursion guard (max depth) to prevent infinite plugin → tool → plugin loops
- [ ] **Step 5:** Check `ToolCall { tools }` restriction — only allow calling tools in the declared list

---

## Task 5: Implement memory_read and memory_write

**Files:** `sober-plugin/src/host_fns.rs`, `sober-plugin/Cargo.toml`

- [ ] **Step 1:** Add `sober-memory` dependency
- [ ] **Step 2:** In `host_memory_query_impl`:
  - `handle.block_on(memory_store.search(user_id, query))`
  - Return results as JSON
- [ ] **Step 3:** In `host_memory_write_impl`:
  - `handle.block_on(memory_store.store(user_id, chunk))`
- [ ] **Step 4:** Check `MemoryRead/Write { scopes }` restriction

---

## Task 6: Implement conversation_read

**Files:** `sober-plugin/src/host_fns.rs`

- [ ] **Step 1:** Add conversation repo access (via PgPool or trait)
- [ ] **Step 2:** In `host_conversation_read_impl`:
  - Parse conversation_id, limit
  - `handle.block_on(message_repo.list_by_conversation(conv_id, limit))`
  - Return messages as JSON

---

## Task 7: Implement schedule

**Files:** `sober-plugin/src/host_fns.rs`

- [ ] **Step 1:** Add scheduler gRPC client to HostContext
- [ ] **Step 2:** In `host_schedule_impl`:
  - Create a job via the scheduler client
  - `handle.block_on(scheduler_client.create_job(...))`
  - Return job_id

---

## Task 8: Implement llm_call

**Files:** `sober-plugin/src/host_fns.rs`

- [ ] **Step 1:** In `host_llm_complete_impl`:
  - Build CompletionRequest from plugin input
  - `handle.block_on(llm_engine.complete(req))`
  - Return response text

---

## Task 9: Implement filesystem (read/write)

**Files:** `sober-plugin/src/host_fns.rs`

- [ ] **Step 1:** In `host_fs_read_impl` / `host_fs_write_impl`:
  - Check `Filesystem { paths }` restriction — only allow paths in the declared list
  - Use `std::fs::read_to_string` / `std::fs::write` (sync — no async bridge needed)
  - Sandbox: validate path is within workspace boundary

---

## Task 10: Wire services through PluginManager → PluginHost

**Files:** `sober-plugin/src/manager.rs`, `sober-plugin/src/host.rs`, `sober-agent/src/main.rs`

- [ ] **Step 1:** Add service handles to `PluginManager` constructor
- [ ] **Step 2:** Pass them through to `PluginHost::load()` when creating WASM hosts in `wasm_tools()`
- [ ] **Step 3:** Update `sober-agent/src/main.rs` to provide the handles

---

## Task 11: Update generation prompt and verify

- [ ] **Step 1:** Update capability status in `generate.rs` prompt to mark all as functional
- [ ] **Step 2:** End-to-end test: generate a plugin that uses KV + network, install, execute

---

## Acceptance Criteria

- [ ] All 11 capabilities functional (log, kv, network, metrics, secret_read, tool_call, memory_read, memory_write, conversation_read, schedule, filesystem, llm_call)
- [ ] KV backed by `plugin_kv_data` PostgreSQL table, persists across restarts
- [ ] tool_call has recursion guard
- [ ] Network enforces domain restrictions
- [ ] Filesystem enforces path restrictions
- [ ] Memory enforces scope restrictions
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
