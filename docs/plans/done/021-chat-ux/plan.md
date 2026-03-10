# Plan 021: Chat UX Improvements

## Goal

Improve chat UX with thinking indicators, model reasoning display, message queuing,
and smart scroll behavior.

## Changes

### Frontend
1. **ThinkingIndicator.svelte** — animated dots shown before first response delta
2. **ScrollToBottom.svelte** — floating button when user scrolls up during streaming
3. **ChatMessage.svelte** — `thinking` prop, `thinkingContent` prop for model reasoning
   display in collapsible section, tool calls during thinking in collapsible section
4. **ChatInput.svelte** — replace `disabled` with `busy`, input always enabled
5. **+page.svelte** — phase state machine (idle/thinking/streaming), message queue
   with edit/remove, smart scroll, scroll-on-open, `chat.thinking` handler
6. **types/index.ts** — add `chat.thinking` to `ServerWsMessage`

### Backend
7. **agent.proto** — add `ThinkingDelta` message and field 6 to `AgentEvent.oneof`
8. **sober-api/ws.rs** — add `ChatThinking` variant to `ServerWsMessage`, map
   `ThinkingDelta` proto event to `chat.thinking` WebSocket message

## Acceptance Criteria

- Thinking dots appear immediately after sending, before first delta
- Model reasoning (extended thinking) streamed via `chat.thinking` and shown in
  collapsible "Reasoning" section on assistant messages
- Tool calls during thinking shown in collapsible section
- Input never blocks; messages queue while assistant is busy
- Queued messages can be edited/removed
- Conversation opens scrolled to bottom
- Auto-scroll only when user is at bottom
- "Jump to present" button appears when scrolled up
- `pnpm check` and `pnpm build` pass
- `cargo build -p sober-api`, `cargo clippy -p sober-api`, `cargo test -p sober-api` pass
