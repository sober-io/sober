# Thinking & Tool Execution UX Improvements

## Context

After the #038 agent rewrite, tool execution events changed from `ToolCallStart`/`ToolCallResult` to `ToolExecutionUpdate` with status transitions (`pending` -> `running` -> `completed`/`failed`). The tool loading spinner broke due to a Svelte 5 reactivity issue with in-place mutation of nested objects. Additionally, the thinking phase only shows bouncing dots — reasoning content streams into a collapsed `<details>` section that users miss.

## Changes

### 1. Streaming Reasoning During Thinking Phase

**Files:** `ChatMessage.svelte`, `ThinkingIndicator.svelte`

Current behavior: When `thinking && !content`, only `ThinkingIndicator` (3 bouncing dots) is shown. Reasoning content goes into a collapsed `<details>` below.

New behavior:
- Before reasoning arrives: bouncing dots + "Thinking..." label (unchanged)
- Once `thinkingContent` starts arriving: dots + label remain as header, reasoning text streams below in dimmed, smaller style (`text-xs text-zinc-400`)
- Max height with auto-scroll to bottom as content streams
- When real content (`chat.delta`) starts arriving, the reasoning section auto-collapses into the existing `<details>` (label changes to "Reasoning")

Update `ThinkingIndicator` to accept optional `thinkingContent` prop:
```svelte
// ThinkingIndicator.svelte
interface Props {
  thinkingContent?: string;
}
```

When `thinkingContent` is non-empty, render it below the dots in a scrollable container:
```
[dots] Thinking...
─────────────────
The user is asking about...   ← text-xs text-zinc-400, max-h-40, overflow-y-auto
I need to check the...        ← auto-scrolls to bottom
```

In `ChatMessage.svelte`, change the thinking branch:
```svelte
{#if thinking && !content}
  <ThinkingIndicator {thinkingContent} />
{:else if streaming}
  ...
```

The existing `<details>` section for `hasThinkingContent` stays for post-thinking display.

### 2. Fix Tool Execution Loading Spinner

**File:** `+page.svelte` (chat page)

Bug: When updating an existing tool execution in-place (`existing.status = msg.status`), Svelte 5 reactivity doesn't propagate because the `toolExecutions` array reference on the message object doesn't change. The `messages = [...messages]` at the end creates a new top-level array but the nested mutation isn't tracked through props.

Fix: Replace the in-place mutation with a new array containing a new object:
```typescript
case 'chat.tool_execution_update': {
  // ... find target message ...

  const idx = target.toolExecutions.findIndex((te) => te.id === msg.id);
  if (idx >= 0) {
    // Replace with new object instead of mutating in place
    target.toolExecutions = target.toolExecutions.map((te) =>
      te.id === msg.id
        ? { ...te, status: msg.status, output: msg.output ?? te.output, error: msg.error ?? te.error }
        : te
    );
  } else {
    // New execution — append (existing code is fine, already creates new array)
    target.toolExecutions = [...target.toolExecutions, { ... }];
  }

  messages = [...messages];
}
```

### 3. Tool Execution Summary Line

**File:** `ChatMessage.svelte`

Add a compact summary line above tool execution panels when any tools are running:

```svelte
{#if runningToolCount > 0}
  <div class="mt-2 flex items-center gap-2 text-xs text-zinc-400">
    <span class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-zinc-400 border-t-transparent"></span>
    <span class="animate-pulse">{runningToolCount} tool{runningToolCount > 1 ? 's' : ''} running</span>
  </div>
{/if}
```

Derived state:
```typescript
const runningToolCount = $derived(
  toolExecutions?.filter((te) => te.status === 'pending' || te.status === 'running').length ?? 0
);
```

### 4. Improve Tool Input JSON Formatting

**File:** `ToolCallDisplay.svelte`

Replace raw `JSON.stringify` with formatted, readable display:
- Pretty-print with 2-space indent (already done)
- Add key-value label styling: keys in a muted color, values in normal text
- Truncate long string values with expandable "show more"
- For simple inputs (1-2 keys), show inline instead of code block

Implementation: Create a simple `formatToolInput` function that renders JSON with basic syntax highlighting via CSS classes, or use a lightweight approach with `<code>` blocks and colored spans for keys vs values.

## Files to Modify

| File | Change |
|------|--------|
| `frontend/src/lib/components/ThinkingIndicator.svelte` | Accept `thinkingContent` prop, render streaming reasoning below dots |
| `frontend/src/lib/components/ChatMessage.svelte` | Pass `thinkingContent` to ThinkingIndicator, add tool summary line |
| `frontend/src/lib/components/ToolCallDisplay.svelte` | Improve JSON input formatting |
| `frontend/src/routes/(app)/chat/[id]/+page.svelte` | Fix tool execution reactivity bug (replace in-place mutation) |

## Verification

1. Send a message that triggers thinking — verify reasoning text streams below the dots
2. Send a message that triggers tool calls — verify spinner animates on pending/running tools
3. Verify summary line shows "N tools running" and disappears when all complete
4. Expand a tool panel — verify input JSON is nicely formatted
5. Verify failed tools show red X and error text
6. Run `pnpm check` and `pnpm test --silent`
