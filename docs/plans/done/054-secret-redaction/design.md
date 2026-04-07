# Secret Redaction Design

## Problem

Secrets are leaked in three places:

1. **`store_secret` tool input** — the `data` field containing plaintext secret values is persisted to `conversation_tool_executions.input` and broadcast over WebSocket.
2. **`fetch_url` tool input** — when the agent uses a secret (read via `read_secret`) in HTTP headers, the headers are persisted and broadcast with the secret in plaintext.
3. **User messages** — when a user pastes a secret (e.g., "store my API key sk-abc123"), the message is stored verbatim in `conversation_messages.content`.

Additionally, audit log entries (e.g., shell command details) could contain secret values.

## Design

Two redaction layers cover all cases.

### Layer 1: Per-Turn Secret Registry

A `SecretRegistry` scoped to a single `run_turn` invocation. It holds a list of `(plaintext_value, secret_name)` pairs. Populated by:

- **`read_secret`** — registers each sensitive leaf value from the decrypted JSON.
- **`store_secret`** — self-registers the `data` field values before dispatch persists the tool execution.

The dispatch layer calls `registry.redact(text)` which does string replacement of every known plaintext with `[REDACTED: secret-name]`. This runs on:

| Storage path | Field | When |
|---|---|---|
| `conversation_tool_executions.input` | Tool input JSON | Before `create_pending()` |
| `conversation_tool_executions.output` | Tool output string | Before `update_status()` with completed |
| `audit_logs.details` | Shell command, confirmation details | Before `log_shell_exec()` / `log_confirmation()` |

**What gets registered:** Only sensitive leaf string values from the secret JSON — not metadata fields (`provider`, `server`, `base_url`, `model`, `description`) which are already stored in plaintext metadata.

**What stays unredacted:** The in-memory tool input/output passed back to the LLM context window. The LLM needs real values to reason correctly.

**Scope:** Created at `run_turn` start, dropped when the turn ends. No long-lived decrypted material.

### Layer 2: Post-hoc User Message Redaction

After `store_secret` successfully stores a secret:

1. Load the current turn's user message from DB (message ID available on turn context).
2. Replace each sensitive value from the `data` field with `[REDACTED: secret-name]` in the message content.
3. Write the updated message back to DB.
4. Send a `MessageUpdated` WebSocket event so the frontend replaces the displayed message.

**Why only on `store_secret`:** This is the only moment a brand-new secret (not previously in the vault) enters the system. Existing secrets are accessed via `read_secret` (fully redacted — no DB/WS trace).

**If store fails:** No redaction happens. The secret remains in the user message, but storage failed so the user will retry. Acceptable trade-off — the alternative risks losing the secret entirely.

### Redaction Coverage Matrix

| Scenario | Registry (Layer 1) | Post-hoc (Layer 2) |
|---|---|---|
| `store_secret` input with new secret | Yes (self-registers) | N/A |
| `fetch_url` headers with read secret | Yes (from `read_secret`) | N/A |
| `fetch_url` URL path with secret | Yes (string match) | N/A |
| `fetch_url` response echoing secret | Yes (output scan) | N/A |
| `shell` command with secret | Yes (input + audit log) | N/A |
| User message with pasted secret | N/A | Yes (after `store_secret`) |
| Any tool using a vault secret | Yes | N/A |

### Changes Required

**New types:**
- `SecretRegistry` — `Vec<(String, String)>` with `register(plaintext, name)` and `redact(&self, text: &str) -> String` methods. Lives in `sober-agent`.

**Modified:**
- `TurnParams` / `DispatchContext` — carries `Arc<Mutex<SecretRegistry>>` (mutex because `read_secret` and `store_secret` register during execution, dispatch reads during persistence).
- `read_secret` — after decryption, registers sensitive values into the registry.
- `store_secret` — registers `data` values into the registry before returning. After success, performs post-hoc user message redaction.
- `dispatch.rs` — calls `registry.redact()` on tool input before `create_pending()`, on tool output before `update_status()`.
- `dispatch.rs` audit paths — calls `registry.redact()` on shell command and confirmation details.
- WS protocol — add `MessageUpdated` event type for post-hoc message updates.
- Frontend — handle `MessageUpdated` to replace displayed message content.

**Unchanged:**
- `read_secret` stays `redacted: true` (fully invisible to DB/WS).
- `ToolMetadata.redacted` field keeps existing semantics.
- `Tool` trait — no new methods.
