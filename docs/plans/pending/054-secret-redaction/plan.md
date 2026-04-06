# Secret Redaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent secrets from leaking into persisted tool execution records, audit logs, WebSocket events, and user messages.

**Architecture:** A per-turn `SecretRegistry` collects plaintext secret values as they pass through `read_secret` and `store_secret`. The dispatch layer redacts these values from all data before persistence and broadcast. After `store_secret` succeeds, the user's original message is retroactively redacted in the database and a `MessageUpdated` WebSocket event notifies the frontend.

**Tech Stack:** Rust (sober-agent, sober-core, sober-api), Protobuf, Svelte 5 (frontend WebSocket handler)

---

### Task 1: Create `SecretRegistry`

**Files:**
- Create: `backend/crates/sober-agent/src/secret_registry.rs`
- Modify: `backend/crates/sober-agent/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In the new file, add the module with tests:

```rust
// backend/crates/sober-agent/src/secret_registry.rs

//! Per-turn registry of known secret values for redaction.
//!
//! Populated by `read_secret` and `store_secret` during tool execution.
//! The dispatch layer uses [`SecretRegistry::redact`] to strip secret
//! values from strings before persistence and broadcast.

use std::sync::Mutex;

/// Collects plaintext secret values during a turn for redaction.
///
/// Thread-safe via internal [`Mutex`] — tools register values during
/// execution, and the dispatch layer reads them for redaction.
#[derive(Debug, Default)]
pub struct SecretRegistry {
    /// (plaintext_value, secret_name) pairs.
    entries: Mutex<Vec<(String, String)>>,
}

impl SecretRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a secret value for redaction.
    ///
    /// Short or empty values are ignored to avoid false-positive replacements.
    pub fn register(&self, plaintext: &str, secret_name: &str) {
        // Skip values that are too short to meaningfully redact — single
        // characters or empty strings would cause excessive false positives.
        if plaintext.len() < 4 {
            return;
        }
        let mut entries = self.entries.lock().expect("secret registry lock poisoned");
        // Deduplicate — don't register the same value twice.
        if !entries.iter().any(|(v, _)| v == plaintext) {
            entries.push((plaintext.to_owned(), secret_name.to_owned()));
        }
    }

    /// Replaces all registered secret values in `text` with `[REDACTED: name]`.
    ///
    /// Longer values are replaced first to avoid partial matches when one
    /// secret value is a substring of another.
    pub fn redact(&self, text: &str) -> String {
        let entries = self.entries.lock().expect("secret registry lock poisoned");
        if entries.is_empty() {
            return text.to_owned();
        }
        // Sort by length descending so longer matches are replaced first.
        let mut sorted: Vec<_> = entries.iter().collect();
        sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut result = text.to_owned();
        for (plaintext, name) in sorted {
            result = result.replace(plaintext.as_str(), &format!("[REDACTED: {name}]"));
        }
        result
    }

    /// Returns `true` if no secrets have been registered.
    pub fn is_empty(&self) -> bool {
        self.entries.lock().expect("secret registry lock poisoned").is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_returns_input_unchanged() {
        let reg = SecretRegistry::new();
        assert_eq!(reg.redact("hello world"), "hello world");
        assert!(reg.is_empty());
    }

    #[test]
    fn redacts_registered_value() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "my-api-key");
        assert_eq!(
            reg.redact("Authorization: Bearer sk-abc123"),
            "Authorization: Bearer [REDACTED: my-api-key]"
        );
    }

    #[test]
    fn redacts_multiple_values() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "openai-key");
        reg.register("ghp_xyz789", "github-token");
        let input = "keys: sk-abc123 and ghp_xyz789";
        let result = reg.redact(input);
        assert_eq!(result, "keys: [REDACTED: openai-key] and [REDACTED: github-token]");
    }

    #[test]
    fn longer_values_replaced_first() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc", "short-key");
        reg.register("sk-abc123456", "long-key");
        let result = reg.redact("token: sk-abc123456");
        assert_eq!(result, "token: [REDACTED: long-key]");
    }

    #[test]
    fn ignores_short_values() {
        let reg = SecretRegistry::new();
        reg.register("abc", "too-short");
        assert!(reg.is_empty());
        assert_eq!(reg.redact("abc def"), "abc def");
    }

    #[test]
    fn deduplicates_registrations() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "key-1");
        reg.register("sk-abc123", "key-2");
        // First registration wins.
        assert_eq!(
            reg.redact("sk-abc123"),
            "[REDACTED: key-1]"
        );
    }

    #[test]
    fn redacts_in_json_string() {
        let reg = SecretRegistry::new();
        reg.register("secret-value-here", "my-secret");
        let json = r#"{"headers":{"Authorization":"Bearer secret-value-here"}}"#;
        let result = reg.redact(json);
        assert_eq!(
            result,
            r#"{"headers":{"Authorization":"Bearer [REDACTED: my-secret]"}}"#
        );
    }
}
```

- [ ] **Step 2: Register the module in lib.rs**

Add to `backend/crates/sober-agent/src/lib.rs`:

```rust
pub mod secret_registry;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p sober-agent -q -- secret_registry`
Expected: All 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/secret_registry.rs backend/crates/sober-agent/src/lib.rs
git commit -m "feat(agent): add SecretRegistry for per-turn secret redaction"
```

---

### Task 2: Thread `SecretRegistry` through `TurnParams` and `DispatchRequest`

**Files:**
- Modify: `backend/crates/sober-agent/src/turn.rs:50-75` (TurnParams struct)
- Modify: `backend/crates/sober-agent/src/turn.rs:87-100` (run_turn — create registry, pass to dispatch)
- Modify: `backend/crates/sober-agent/src/turn.rs:343-351` (dispatch_req construction)
- Modify: `backend/crates/sober-agent/src/dispatch.rs:62-77` (DispatchRequest struct)

- [ ] **Step 1: Add `SecretRegistry` to `TurnParams`**

In `backend/crates/sober-agent/src/turn.rs`, add import:

```rust
use crate::secret_registry::SecretRegistry;
```

Add field to `TurnParams` (after `skill_catalog_xml`):

```rust
    /// Per-turn secret registry for redacting sensitive values from persistence.
    pub secret_registry: Arc<SecretRegistry>,
```

- [ ] **Step 2: Add `SecretRegistry` to `DispatchRequest`**

In `backend/crates/sober-agent/src/dispatch.rs`, add import:

```rust
use crate::secret_registry::SecretRegistry;
```

Add field to `DispatchRequest` (after `workspace_id`):

```rust
    /// Per-turn secret registry for redacting sensitive values.
    pub secret_registry: &'a Arc<SecretRegistry>,
```

- [ ] **Step 3: Pass registry from `run_turn` to `DispatchRequest`**

In `turn.rs`, where the `dispatch_req` is constructed (around line 343):

```rust
let dispatch_req = dispatch::DispatchRequest {
    tool_calls: &tool_calls,
    assistant_message_id: assistant_msg_id,
    conversation_id: params.conversation_id,
    tool_registry: params.tool_registry,
    event_tx: params.event_tx,
    user_id: params.user_id,
    workspace_id: params.workspace_id,
    secret_registry: &params.secret_registry,
};
```

- [ ] **Step 4: Update all `TurnParams` construction sites**

Search for where `TurnParams` is constructed and add `secret_registry: Arc::new(SecretRegistry::new())`. The `SecretRegistry` must be the same `Arc` passed to `TurnContext` (for the secret tools) so they share state.

Run: `grep -rn 'TurnParams {' backend/crates/sober-agent/src/`

At each site, create the registry once and pass the same `Arc` to both `TurnParams` and `TurnContext`.

- [ ] **Step 5: Build and verify**

Run: `cargo build -p sober-agent -q`
Expected: Compiles. Some warnings about unused `secret_registry` field are OK.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/turn.rs backend/crates/sober-agent/src/dispatch.rs
git commit -m "feat(agent): thread SecretRegistry through TurnParams and DispatchRequest"
```

---

### Task 3: Redact tool inputs and outputs in dispatch

**Files:**
- Modify: `backend/crates/sober-agent/src/dispatch.rs:156-217` (tool input redaction before `create_pending`)
- Modify: `backend/crates/sober-agent/src/dispatch.rs:292-347` (tool output redaction before `update_status`, audit log redaction)

- [ ] **Step 1: Redact tool input before `create_pending`**

In `dispatch.rs`, in the `execute_tool_calls` function, after `tool_redacted` is determined (line 149) and before the `create_pending` call (line 159), compute the redacted input. Keep the original `tool_input` for actual execution — only the persisted/broadcast version is redacted.

After line 149 (`let tool_redacted = ...`), add:

```rust
        // Redact sensitive values from the serialized input for persistence/broadcast.
        let redacted_input_json = if !tool_redacted {
            serde_json::to_string(&tool_input).ok().map(|s| req.secret_registry.redact(&s))
        } else {
            None
        };
```

Change the `create_pending` call to use the redacted input (line 167):

```rust
                    input: redacted_input_json.clone().unwrap_or_default(),
```

Change the pending event broadcast (line 202) to use the redacted input:

```rust
            let input_json = redacted_input_json.as_deref();
            send_execution_update(
                req.event_tx,
                &ctx.broadcast_tx,
                &conv_id_str,
                &exec_id_str,
                &msg_id_str,
                &tc.id,
                tool_name,
                ToolExecutionStatus::Pending,
                None,
                None,
                input_json,
            )
            .await;
```

- [ ] **Step 2: Redact tool output before `update_status`**

In the step 5 section (around line 292), where `db_output` and `db_error` are determined, apply redaction:

```rust
        let redacted_output = req.secret_registry.redact(&output);
        let final_status = if is_error {
            ToolExecutionStatus::Failed
        } else {
            ToolExecutionStatus::Completed
        };
        let (db_output, db_error) = if is_error {
            (None, Some(redacted_output.as_str()))
        } else {
            (Some(redacted_output.as_str()), None)
        };
```

Also use `redacted_output` in the `send_execution_update` call (around line 316):

```rust
            send_execution_update(
                req.event_tx,
                &ctx.broadcast_tx,
                &conv_id_str,
                &exec_id_str,
                &msg_id_str,
                &tc.id,
                tool_name,
                final_status,
                if !is_error { Some(&redacted_output) } else { None },
                if is_error { Some(&redacted_output) } else { None },
                None,
            )
            .await;
```

- [ ] **Step 3: Redact shell audit log details**

In the audit logging section (around line 336), redact the command:

```rust
        if let Some(ref cmd) = shell_command {
            let redacted_cmd = req.secret_registry.redact(cmd);
            let _ = crate::audit::log_shell_exec(
                ctx.repos.audit_log(),
                req.user_id,
                req.workspace_id,
                serde_json::json!({
                    "command": redacted_cmd,
                    "conversation_id": req.conversation_id.to_string(),
                }),
            )
            .await;
        }
```

- [ ] **Step 4: Build and verify**

Run: `cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-agent/src/dispatch.rs
git commit -m "feat(agent): redact secret values from tool execution persistence and broadcast"
```

---

### Task 4: Wire `read_secret` to register values in the registry

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/secrets.rs:25-31` (SecretToolContext — add registry field)
- Modify: `backend/crates/sober-agent/src/tools/secrets.rs:278-329` (ReadSecretTool::execute_inner)
- Modify: `backend/crates/sober-agent/src/tools/bootstrap.rs:302-314` (secret tool construction)

- [ ] **Step 1: Add `SecretRegistry` to `SecretToolContext`**

In `secrets.rs`, add import:

```rust
use crate::secret_registry::SecretRegistry;
```

Add field to `SecretToolContext`:

```rust
pub struct SecretToolContext<S: SecretRepo, A: AuditLogRepo> {
    pub secret_repo: Arc<S>,
    pub audit_repo: Arc<A>,
    pub mek: Arc<Mek>,
    pub user_id: UserId,
    pub conversation_id: Option<ConversationId>,
    /// Per-turn secret registry — decrypted values are registered here for redaction.
    pub secret_registry: Arc<SecretRegistry>,
}
```

- [ ] **Step 2: Add `register_secret_values` helper**

Add this helper function near `extract_metadata`:

```rust
/// Registers sensitive leaf values from a secret's decrypted JSON into the
/// per-turn [`SecretRegistry`].
///
/// Only registers string values whose keys are NOT in [`METADATA_KEYS`]
/// (those are non-sensitive and already stored in plaintext).
fn register_secret_values(
    registry: &SecretRegistry,
    data: &serde_json::Value,
    secret_name: &str,
) {
    if let Some(obj) = data.as_object() {
        for (key, value) in obj {
            if METADATA_KEYS.contains(&key.as_str()) {
                continue;
            }
            if let Some(s) = value.as_str() {
                registry.register(s, secret_name);
            }
        }
    }
}
```

- [ ] **Step 3: Register decrypted values in `ReadSecretTool::execute_inner`**

After the decryption succeeds and before the audit write (around line 313-316), parse the decrypted JSON and register sensitive values:

```rust
        let decrypted_str = String::from_utf8(plaintext)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid UTF-8 in secret: {e}")))?;

        // Register sensitive values in the per-turn secret registry for redaction.
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&decrypted_str) {
            register_secret_values(&self.ctx.secret_registry, &parsed, name);
        }

        write_audit(
```

- [ ] **Step 4: Add `SecretRegistry` to `TurnContext` and update bootstrap**

In `bootstrap.rs`, add a `secret_registry` field to `TurnContext`:

```rust
use crate::secret_registry::SecretRegistry;

pub struct TurnContext {
    // ... existing fields ...
    /// Per-turn secret registry for redaction.
    pub secret_registry: Arc<SecretRegistry>,
}
```

Update the `SecretToolContext` construction (around line 303):

```rust
            let secret_ctx = Arc::new(SecretToolContext {
                secret_repo: Arc::new(self.repos.secrets().clone()),
                audit_repo: Arc::new(self.repos.audit_log().clone()),
                mek: Arc::clone(mek),
                user_id: ctx.user_id,
                conversation_id: Some(ctx.conversation_id),
                secret_registry: Arc::clone(&ctx.secret_registry),
            });
```

Update all `TurnContext` construction sites to pass the same `Arc<SecretRegistry>` used for `TurnParams`.

- [ ] **Step 5: Build and verify**

Run: `cargo build -p sober-agent -q`
Expected: Compiles.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/tools/secrets.rs backend/crates/sober-agent/src/tools/bootstrap.rs
git commit -m "feat(agent): register decrypted secret values in per-turn SecretRegistry"
```

---

### Task 5: Wire `store_secret` to self-register data values

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/secrets.rs:134-216` (StoreSecretTool::execute_inner)

- [ ] **Step 1: Register `data` values in `store_secret`**

In `StoreSecretTool::execute_inner`, after extracting `data` (line 151-159) and before encryption (line 177), register the sensitive values:

```rust
        // Register sensitive values for redaction so the dispatch layer
        // can redact the tool input that contains them.
        register_secret_values(&self.ctx.secret_registry, data, &name);
```

This reuses the `register_secret_values` helper from Task 4.

- [ ] **Step 2: Build and verify**

Run: `cargo build -p sober-agent -q`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-agent/src/tools/secrets.rs
git commit -m "feat(agent): store_secret self-registers data values for redaction"
```

---

### Task 6: Add `MessageUpdated` to the proto and WebSocket protocol

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`
- Modify: `backend/crates/sober-api/src/ws_types.rs`
- Modify: `backend/crates/sober-api/src/subscribe.rs`

- [ ] **Step 1: Add `MessageUpdated` to proto**

In `backend/proto/sober/agent/v1/agent.proto`, after the existing message definitions (around line 130), add:

```protobuf
message MessageUpdated {
  string message_id = 1;
  string content = 2;          // JSON-encoded ContentBlock array
}
```

Add to `ConversationUpdate.oneof event` (around line 225):

```protobuf
    MessageUpdated message_updated = 11;
```

- [ ] **Step 2: Add `ChatMessageUpdated` to WebSocket types**

In `backend/crates/sober-api/src/ws_types.rs`, add a new variant to `ServerWsMessage`:

```rust
    /// A message's content was updated (e.g., secret redaction).
    #[serde(rename = "chat.message_updated")]
    ChatMessageUpdated {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the updated message.
        message_id: String,
        /// Updated content blocks (JSON-encoded).
        content: String,
    },
```

- [ ] **Step 3: Handle `MessageUpdated` in subscribe conversion**

In `backend/crates/sober-api/src/subscribe.rs`, in the `conversation_update_to_ws` function, add a match arm:

```rust
        proto::conversation_update::Event::MessageUpdated(mu) => {
            Some(ServerWsMessage::ChatMessageUpdated {
                conversation_id: cid,
                message_id: mu.message_id,
                content: mu.content,
            })
        }
```

- [ ] **Step 4: Build**

Run: `cargo build -p sober-api -q`
Expected: Compiles. Proto bindings auto-generated by tonic-build.

- [ ] **Step 5: Commit**

```bash
git add backend/proto/ backend/crates/sober-api/src/ws_types.rs backend/crates/sober-api/src/subscribe.rs
git commit -m "feat(api): add MessageUpdated proto event and WebSocket type"
```

---

### Task 7: Post-hoc user message redaction in `run_turn`

**Files:**
- Modify: `backend/crates/sober-agent/src/turn.rs`

- [ ] **Step 1: Add post-hoc redaction at the end of `run_turn`**

Find the end of the `run_turn` function — after the main `loop` exits. Before the function returns `Ok(())`, add:

```rust
    // Post-hoc redaction: if secrets were registered during this turn,
    // redact them from the user's original message (which may contain
    // pasted secret values).
    if !params.secret_registry.is_empty() {
        if let Err(e) = redact_user_message(params).await {
            warn!(error = %e, "failed to redact user message");
        }
    }
```

- [ ] **Step 2: Implement `redact_user_message`**

Add this function in `turn.rs`:

```rust
/// Redacts registered secret values from the user's original message.
///
/// Loads the message, replaces secret values with `[REDACTED: name]`,
/// and updates the DB. Sends a `MessageUpdated` event over the broadcast
/// channel so the frontend refreshes the displayed message.
async fn redact_user_message<R: AgentRepos>(
    params: &TurnParams<'_, R>,
) -> Result<(), AgentError> {
    let msg = params
        .ctx
        .repos
        .messages()
        .get_by_id(params.user_msg_id)
        .await
        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

    // Redact each content block's text.
    let mut changed = false;
    let redacted_content: Vec<ContentBlock> = msg
        .content
        .iter()
        .map(|block| {
            if let Some(text) = block.as_text() {
                let redacted = params.secret_registry.redact(text);
                if redacted != *text {
                    changed = true;
                    ContentBlock::text(redacted)
                } else {
                    block.clone()
                }
            } else {
                block.clone()
            }
        })
        .collect();

    if !changed {
        return Ok(());
    }

    // Update the message in DB.
    params
        .ctx
        .repos
        .messages()
        .update_content(params.user_msg_id, &redacted_content, msg.reasoning.as_deref())
        .await
        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

    // Notify the frontend via broadcast.
    let update = proto::ConversationUpdate {
        conversation_id: params.conversation_id.to_string(),
        event: Some(proto::conversation_update::Event::MessageUpdated(
            proto::MessageUpdated {
                message_id: params.user_msg_id.to_string(),
                content: serde_json::to_string(&redacted_content).unwrap_or_default(),
            },
        )),
    };
    let _ = params.ctx.broadcast_tx.send(update);

    info!(
        message_id = %params.user_msg_id,
        "redacted secret values from user message"
    );

    Ok(())
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p sober-agent -q`
Expected: Compiles (depends on Task 6 for the proto type).

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/turn.rs
git commit -m "feat(agent): post-hoc redact secret values from user messages"
```

---

### Task 8: Handle `chat.message_updated` in the frontend

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`
- Modify: `frontend/src/lib/types/index.ts`

- [ ] **Step 1: Add the type to `ServerWsMessage`**

In `frontend/src/lib/types/index.ts`, find the WS message type definitions. Add:

```typescript
| { type: 'chat.message_updated'; conversation_id: string; message_id: string; content: string }
```

- [ ] **Step 2: Add handler in the WebSocket switch**

In `+page.svelte`, in the WebSocket message switch (around line 458), add a new case:

```typescript
case 'chat.message_updated': {
    const target = messages.find((m) => m.id === msg.message_id);
    if (target) {
        try {
            target.contentBlocks = JSON.parse(msg.content);
        } catch {
            // Ignore malformed content updates.
        }
    }
    break;
}
```

- [ ] **Step 3: Build frontend**

Run: `cd frontend && pnpm check && pnpm build`
Expected: No type errors, builds successfully.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/
git commit -m "feat(frontend): handle chat.message_updated for secret redaction"
```

---

### Task 9: End-to-end verification

**Files:** None (testing only)

- [ ] **Step 1: Run full backend tests**

Run: `cargo test --workspace -q`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -q -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run frontend checks**

Run: `cd frontend && pnpm check && pnpm test --silent`
Expected: All checks pass.

- [ ] **Step 4: Manual smoke test**

Start the system (`just dev`) and test:

1. **Store a new secret:** Send "store my OpenAI key sk-test-abc123" — verify tool execution panel shows `[REDACTED]` for data values, user message updates to show `[REDACTED: openai-key]`.
2. **Read then use secret:** Send "use my openai key to call the API" — agent calls `read_secret` then `fetch_url` — verify `fetch_url` execution shows `Authorization: [REDACTED: openai-key]`.
3. **Page reload:** Verify redacted values persist in DB-loaded history.
