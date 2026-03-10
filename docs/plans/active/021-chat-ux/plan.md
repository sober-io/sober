# Plan 021: Chat UX Improvements

## Goal

Improve chat UX with thinking indicators, message queuing, and smart scroll behavior.

## Changes

1. **ThinkingIndicator.svelte** — animated dots shown before first response delta
2. **ScrollToBottom.svelte** — floating button when user scrolls up during streaming
3. **ChatMessage.svelte** — add `thinking` prop for thinking state rendering
4. **ChatInput.svelte** — replace `disabled` with `busy`, input always enabled
5. **+page.svelte** — phase state machine, message queue, smart scroll

## Acceptance Criteria

- Thinking dots appear immediately after sending, before first delta
- Tool calls during thinking shown in collapsible section
- Input never blocks; messages queue while assistant is busy
- Queued messages can be edited/removed
- Conversation opens scrolled to bottom
- Auto-scroll only when user is at bottom
- "Jump to present" button appears when scrolled up
- `pnpm check` and `pnpm build` pass
