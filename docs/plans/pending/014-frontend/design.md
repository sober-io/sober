# 014 — Frontend

Date: 2026-03-06

## Stack

- SvelteKit with static adapter (Caddy serves built files)
- Svelte 5 (runes only — no legacy patterns)
- Tailwind CSS via `@tailwindcss/vite` plugin
- TypeScript strict mode
- Connects to Rust backend API at `/api/v1/`

---

## Pages

| Route | File | Description |
|---|---|---|
| `/login` | `(public)/login/+page.svelte` | Login form |
| `/register` | `(public)/register/+page.svelte` | Registration form, shows "pending approval" after submit |
| `/` | `(app)/+page.svelte` | Conversation list (redirects to `/login` if unauth) |
| `/chat/[id]` | `(app)/chat/[id]/+page.svelte` | Chat view with streaming |
| `/settings/mcp` | `(app)/settings/mcp/+page.svelte` | MCP server configuration |

### Layout Groups

- `(public)` — No auth required, minimal layout. Login, registration.
- `(app)` — Auth required, sidebar layout with conversation list.

---

## Routing and Auth

### Root `+layout.ts`

Calls `GET /api/v1/auth/me`. If the response is 401, stores `null` in auth state.
Does NOT redirect here — that is the responsibility of child layout groups.

### `(app)/+layout.ts`

Checks auth state from parent. If null, redirects to `/login` using SvelteKit's
`redirect()` helper.

### `(public)/+layout.ts`

No auth check. If the user is already authenticated, optionally redirects to `/`.

---

## API Client — `$lib/utils/api.ts`

Thin typed wrapper around fetch:

```ts
class ApiError extends Error {
  status: number;
  body: unknown;

  constructor(status: number, body: unknown) {
    super(`API error ${status}`);
    this.status = status;
    this.body = body;
  }
}

export async function api<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include', // send cookies
    ...options,
  });
  if (!res.ok) {
    const error = await res.json().catch(() => ({}));
    throw new ApiError(res.status, error);
  }
  return res.json();
}
```

---

## WebSocket — `$lib/stores/websocket.svelte.ts`

Singleton reactive WebSocket connection using Svelte 5 runes. Connects to `/api/v1/ws`
(no conversation ID in URL). All messages include `conversation_id` in the payload.
The store routes incoming messages to conversation-specific handlers.

```ts
type MessageHandler = (data: ServerWsMessage) => void;

export const websocket = (() => {
  let ws = $state<WebSocket | null>(null);
  let connected = $state(false);
  let error = $state<string | null>(null);
  const handlers = new Map<string, MessageHandler>(); // keyed by conversation_id

  function connect() {
    if (ws) return;
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(`${protocol}//${location.host}/api/v1/ws`);
    ws.onopen = () => { connected = true; error = null; };
    ws.onclose = () => { connected = false; ws = null; };
    ws.onerror = () => { error = 'Connection lost'; };
    ws.onmessage = (e) => {
      const msg: ServerWsMessage = JSON.parse(e.data);
      const handler = handlers.get(msg.conversation_id);
      if (handler) handler(msg);
    };
  }

  function disconnect() {
    ws?.close();
    ws = null;
    connected = false;
    handlers.clear();
  }

  function send(msg: ClientWsMessage) {
    ws?.send(JSON.stringify(msg));
  }

  function subscribe(conversationId: string, handler: MessageHandler) {
    handlers.set(conversationId, handler);
    return () => { handlers.delete(conversationId); };
  }

  return {
    get connected() { return connected; },
    get error() { return error; },
    connect,
    disconnect,
    send,
    subscribe,
  };
})();
```

---

## Auth State — `$lib/stores/auth.svelte.ts`

```ts
import type { User } from '$lib/types';

export const auth = (() => {
  let user = $state<User | null>(null);
  let loading = $state(true);

  return {
    get user() { return user; },
    get loading() { return loading; },
    get isAuthenticated() { return user !== null; },
    setUser(u: User | null) { user = u; },
    setLoading(l: boolean) { loading = l; },
  };
})();
```

---

## Key Components

### `ChatMessage.svelte`

Single message bubble (user or assistant).

```ts
interface Props {
  message: Message;
}

let { message }: Props = $props();
```

- Renders markdown in assistant messages (lightweight markdown renderer).
- Shows tool calls inline (collapsible via `ToolCallDisplay`).

### `ChatInput.svelte`

Message input area.

```ts
interface Props {
  onsend: (content: string) => void;
  disabled?: boolean;
}

let { onsend, disabled = false }: Props = $props();
```

- Textarea with Shift+Enter for newlines, Enter to send.
- Send button disabled when empty or when `disabled` is true.

### `ConversationList.svelte`

Sidebar conversation list.

```ts
interface Props {
  conversations: Conversation[];
  activeId?: string;
  oncreate: () => void;
  onselect: (id: string) => void;
}

let { conversations, activeId, oncreate, onselect }: Props = $props();
```

- Shows conversation titles with relative timestamps.
- "New chat" button at the top.

### `ToolCallDisplay.svelte`

Inline tool call display.

```ts
interface Props {
  toolName: string;
  input: unknown;
  output?: string;
  loading?: boolean;
}

let { toolName, input, output, loading = false }: Props = $props();
```

- Collapsible panel showing tool name, input JSON, and output text.
- Loading spinner while tool is executing.

### `StreamingText.svelte`

Renders streaming text with a cursor indicator.

```ts
interface Props {
  content: string;
  streaming?: boolean;
}

let { content, streaming = false }: Props = $props();
```

- Shows a blinking cursor at the end while `streaming` is true.

---

## Data Loading

| Data | Where Loaded | Method |
|---|---|---|
| Conversation list | `(app)/+page.ts` | `api('/conversations')` |
| Conversation messages | `(app)/chat/[id]/+page.ts` | `api('/conversations/${id}')` |
| Real-time chat | `(app)/chat/[id]/+page.svelte` | WebSocket singleton (`websocket.subscribe`) |
| MCP servers | `(app)/settings/mcp/+page.ts` | `api('/mcp/servers')` |
| Auth state | Root `+layout.ts` | `api('/auth/me')` |

---

## Styling Approach

- Tailwind CSS utility classes throughout.
- Dark mode support using Tailwind's `dark:` variant, toggled via `prefers-color-scheme`.
- Responsive layout: sidebar collapses to a hamburger menu on mobile.
- Minimal, functional design. Clean typography, adequate spacing, no decorative elements.

---

## Types — `$lib/types/`

Mirror backend response types:

```ts
interface User {
  id: string;
  email: string;
  username: string;
  roles: string[];
}

interface Conversation {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
}

interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  tool_calls?: ToolCall[];
  created_at: string;
}

interface ToolCall {
  id: string;
  name: string;
  input: unknown;
  output?: string;
}

interface McpServer {
  id: string;
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  enabled: boolean;
}
```

### WebSocket Message Types

```ts
// Client-to-server messages — all include conversation_id
type ClientWsMessage =
  | { type: 'chat.message'; conversation_id: string; content: string }
  | { type: 'chat.cancel'; conversation_id: string };

// Server-to-client messages — all include conversation_id for routing
type ServerWsMessage =
  | { type: 'chat.delta'; conversation_id: string; content: string }
  | { type: 'chat.tool_use'; conversation_id: string; tool_call: ToolCall }
  | { type: 'chat.tool_result'; conversation_id: string; tool_call_id: string; output: string }
  | { type: 'chat.done'; conversation_id: string; message_id: string }
  | { type: 'chat.error'; conversation_id: string; error: string };
```
