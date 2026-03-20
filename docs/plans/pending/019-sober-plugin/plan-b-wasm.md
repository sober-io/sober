# #019 Plan B: WASM Plugin Runtime

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add WASM plugin execution via Extism — load compiled modules, wire capability-gated host functions, execute plugin tools, and enforce the three-layer capability model (compile-time, load-time, runtime).

**Architecture:** `sober-plugin` gains an Extism-based `PluginHost` that loads WASM bytecode and wires only declared host functions. `PluginTool` implements the `Tool` trait so WASM tools are indistinguishable from MCP or built-in tools. `sober-pdk` (in `sdks/sober-pdk/`) provides the guest-side SDK with feature-gated capability modules. The audit pipeline gains sandbox, capability, and test stages for WASM.

**Tech Stack:** Rust, Extism, extism-pdk, serde, toml, thiserror, tracing.

**Prerequisites:** Plan A (Unified Plugin Registry) must be implemented.

**Design doc:** `docs/plans/pending/019-sober-plugin/design.md` — sections 4, 5, 6.

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-plugin/src/host.rs` | `PluginHost` — Extism wrapper, host function wiring |
| `backend/crates/sober-plugin/src/host_fns.rs` | Host function implementations (log, kv, network, etc.) |
| `backend/crates/sober-plugin/src/tool.rs` | `PluginTool` — `Tool` trait impl for WASM plugin tools |
| `sdks/sober-pdk/Cargo.toml` | Guest-side PDK crate manifest |
| `sdks/sober-pdk/src/lib.rs` | PDK module declarations, re-exports |
| `sdks/sober-pdk/src/log.rs` | Logging wrappers (always available) |
| `sdks/sober-pdk/src/kv.rs` | Key-value host function wrappers |
| `sdks/sober-pdk/src/http.rs` | HTTP request host function wrappers |
| `sdks/sober-pdk/src/secret.rs` | Secret read host function wrappers |
| `sdks/sober-pdk/src/tool.rs` | Tool call host function wrappers |
| `sdks/sober-pdk/src/metrics.rs` | Metrics emit host function wrappers |
| `sdks/sober-pdk/build_support/src/lib.rs` | `emit_capability_flags()` for plugin build.rs |

### Modified files

| File | Change |
|------|--------|
| `backend/crates/sober-plugin/Cargo.toml` | Add `extism` dependency |
| `backend/crates/sober-plugin/src/lib.rs` | Add `host`, `host_fns`, `tool` modules |
| `backend/crates/sober-plugin/src/audit.rs` | Add sandbox, capability, test stages for WASM |

---

## Task 1: Add Extism dependency to sober-plugin

**Files:**
- Modify: `backend/crates/sober-plugin/Cargo.toml`

- [ ] **Step 1:** Add `extism = "1"` to `[dependencies]`
- [ ] **Step 2:** Run `cargo build -p sober-plugin -q`
- [ ] **Step 3:** Commit

---

## Task 2: Implement host_fns — host function definitions

**Files:**
- Create: `backend/crates/sober-plugin/src/host_fns.rs`

Implement the host functions that get wired into WASM plugin instances.
Phase 1 capabilities (functional): `host_log`, `host_kv_get`, `host_kv_set`,
`host_http_request`, `host_read_secret`, `host_call_tool`, `host_emit_metric`.
Phase 2+ capabilities: stub functions returning "not yet connected" errors.

Each host function follows the Extism `host_fn!` pattern.

- [ ] **Step 1:** Write `host_log` (always wired, no capability gate)
- [ ] **Step 2:** Write `host_kv_get` and `host_kv_set` (KeyValue capability)
- [ ] **Step 3:** Write `host_http_request` (Network capability) — use `reqwest` or `ureq` for actual HTTP
- [ ] **Step 4:** Write `host_read_secret` (SecretRead capability) — stub initially, needs sober-crypto integration
- [ ] **Step 5:** Write `host_call_tool` (ToolCall capability) — stub initially, needs ToolRegistry reference
- [ ] **Step 6:** Write `host_emit_metric` (Metrics capability) — stub initially
- [ ] **Step 7:** Write stub functions for Phase 2+ capabilities (memory, conversation, schedule, filesystem, llm)
- [ ] **Step 8:** Tests and commit

---

## Task 3: Implement PluginHost — Extism wrapper

**Files:**
- Create: `backend/crates/sober-plugin/src/host.rs`

`PluginHost` loads WASM bytes, reads the manifest, and wires only the
declared host functions into the Extism instance.

- [ ] **Step 1:** Write tests (load trivial WASM, verify tool call works)
- [ ] **Step 2:** Implement `PluginHost::load()` — create Extism manifest, register host functions per capability
- [ ] **Step 3:** Implement `PluginHost::call_tool()` — invoke exported function, deserialize output
- [ ] **Step 4:** Run tests, commit

---

## Task 4: Implement PluginTool — Tool trait for WASM

**Files:**
- Create: `backend/crates/sober-plugin/src/tool.rs`

`PluginTool` wraps a `PluginHost` and implements `Tool` from sober-core.
Uses `std::sync::Mutex` and `spawn_blocking` for WASM execution.

- [ ] **Step 1:** Write tests (metadata returns correct values, execute calls host)
- [ ] **Step 2:** Implement `PluginTool` struct and `Tool` trait
- [ ] **Step 3:** Run tests, commit

---

## Task 5: Extend audit pipeline for WASM

**Files:**
- Modify: `backend/crates/sober-plugin/src/audit.rs`

Add WASM-specific audit stages: sandbox (Extism loads), capability (imports
satisfied), test (embedded tests pass).

- [ ] **Step 1:** Write tests for WASM audit stages
- [ ] **Step 2:** Implement `validate_wasm_sandbox` — load WASM in Extism, verify no import errors
- [ ] **Step 3:** Implement `validate_wasm_capability` — verify only declared host functions are wired
- [ ] **Step 4:** Implement `validate_wasm_test` — run exported test function if present
- [ ] **Step 5:** Wire new stages into `AuditPipeline::audit()` for `PluginKind::Wasm`
- [ ] **Step 6:** Run tests, commit

---

## Task 6: Scaffold sober-pdk crate

**Files:**
- Create: `sdks/sober-pdk/Cargo.toml`
- Create: `sdks/sober-pdk/src/lib.rs`

The PDK is the guest-side SDK. It wraps `extism-pdk` with ergonomic,
feature-gated modules.

- [ ] **Step 1:** Create directory structure `sdks/sober-pdk/src/`
- [ ] **Step 2:** Write `Cargo.toml` with `extism-pdk`, `serde`, `serde_json` deps and capability features
- [ ] **Step 3:** Write `lib.rs` with feature-gated module declarations
- [ ] **Step 4:** Verify `cargo build -p sober-pdk --target wasm32-wasi -q`
- [ ] **Step 5:** Commit

---

## Task 7: Implement PDK modules

**Files:**
- Create: `sdks/sober-pdk/src/log.rs`
- Create: `sdks/sober-pdk/src/kv.rs`
- Create: `sdks/sober-pdk/src/http.rs`
- Create: `sdks/sober-pdk/src/secret.rs`
- Create: `sdks/sober-pdk/src/tool.rs`
- Create: `sdks/sober-pdk/src/metrics.rs`

Each module wraps the corresponding host function with an ergonomic Rust API.

- [ ] **Step 1:** Implement `log` module (info, warn, error, debug functions)
- [ ] **Step 2:** Implement `kv` module (get, set, delete, list)
- [ ] **Step 3:** Implement `http` module (get, post, request)
- [ ] **Step 4:** Implement `secret` module (read)
- [ ] **Step 5:** Implement `tool` module (call)
- [ ] **Step 6:** Implement `metrics` module (emit)
- [ ] **Step 7:** Verify `cargo build -p sober-pdk --target wasm32-wasi -q`
- [ ] **Step 8:** Commit

---

## Task 8: Implement PDK build support

**Files:**
- Create: `sdks/sober-pdk/build_support/Cargo.toml`
- Create: `sdks/sober-pdk/build_support/src/lib.rs`

A helper crate used in plugin `build.rs` to read `plugin.toml` and emit
`cargo:rustc-cfg=feature="..."` lines for each declared capability.

- [ ] **Step 1:** Write tests (parse sample plugin.toml, verify correct cfg flags emitted)
- [ ] **Step 2:** Implement `emit_capability_flags(manifest_path: &str)`
- [ ] **Step 3:** Run tests, commit

---

## Task 9: End-to-end test with sample plugin

- [ ] **Step 1:** Create a minimal test plugin in `backend/crates/sober-plugin/tests/fixtures/`
- [ ] **Step 2:** Pre-compile it to WASM (or use a trivial WASM binary)
- [ ] **Step 3:** Write integration test: load WASM → create PluginHost → call tool → verify output
- [ ] **Step 4:** Write integration test: audit pipeline → install via registry → load → execute
- [ ] **Step 5:** Run tests, commit

---

## Task 10: Final verification

- [ ] **Step 1:** `cargo build -q` (full workspace)
- [ ] **Step 2:** `cargo test -p sober-plugin -q`
- [ ] **Step 3:** `cargo clippy -p sober-plugin -q -- -D warnings`
- [ ] **Step 4:** `cargo build -p sober-pdk --target wasm32-wasi -q`
- [ ] **Step 5:** `cargo test --workspace -q` (no regressions)

---

## Acceptance Criteria

- [ ] `PluginHost::load()` creates an Extism instance with capability-gated host functions
- [ ] `PluginHost::call_tool()` invokes WASM exports and returns `ToolOutput`
- [ ] `PluginTool` implements `Tool` trait with `spawn_blocking` execution
- [ ] Host functions: `host_log` (functional), `host_kv_*` (functional), others (stubs)
- [ ] WASM audit stages: sandbox, capability, test — all functional
- [ ] `sober-pdk` compiles to `wasm32-wasi` with feature-gated capability modules
- [ ] `build_support::emit_capability_flags()` reads `plugin.toml` and sets cfg flags
- [ ] End-to-end test: load WASM → call tool → get output
- [ ] No `.unwrap()` in library code
- [ ] `cargo clippy -- -D warnings` clean
