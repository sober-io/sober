# #048: Desktop Notifications — Design

## Problem

When a message arrives in a conversation the user isn't looking at (tab backgrounded or different conversation open), there's no way to know without manually checking the app. The sidebar unread badges help when the app is visible, but not when the tab is hidden.

## Solution

Use the browser Web Notifications API to show a native desktop notification when a new message arrives from another user while the tab is hidden.

## Behavior

| Condition | Notification? |
|-----------|--------------|
| Tab hidden, message from another user | Yes |
| Tab focused, any message | No |
| Message from self | No |
| Message from assistant (agent) | No |
| Notification permission denied | No (silently skip) |

**Notification content:**
- **Title:** sender's username (fallback: "New message")
- **Body:** first 200 characters of text content, or "Sent an attachment" for non-text
- **Tag:** conversation ID (collapses repeated notifications per conversation)

**Click:** focuses the tab and navigates to `/chat/{conversation_id}`.

**Permission request:** lazy — triggered on the first `chat.new_message` from another user. The `requestPermission()` call is idempotent (no-ops if already granted/denied), so no extra state tracking is needed.

## Architecture

```
WebSocket onmessage
  └── chat.new_message (global intercept, before per-conversation routing)
       ├── Skip if own message or assistant
       ├── notifications.requestPermission() (idempotent)
       └── notifications.notify({ conversationId, title, body })
            ├── Skip if Notification API unavailable
            ├── Skip if permission !== 'granted'
            ├── Skip if !document.hidden
            └── new Notification(title, { body, tag }) + onclick handler
```

Single new file: `frontend/src/lib/stores/notifications.svelte.ts` — owns permission state and the `notify()` function. The WebSocket store (`websocket.svelte.ts`) calls it from the existing `onmessage` handler.

## Non-goals

- Sound customization (rely on OS defaults)
- In-app toast/banner notifications
- Per-conversation notification muting
- Notification for assistant messages
