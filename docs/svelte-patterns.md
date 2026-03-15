## Svelte 5 Patterns — Sõber Project

### Svelte 5 Runes Only

No legacy Svelte 4 patterns. Never use `export let`, `$:`, `createEventDispatcher`, `<slot>`, `on:click`, `<svelte:component>`, `beforeUpdate`/`afterUpdate`, `$$props`/`$$restProps`.

### Runes Quick Reference

- `$state(initial)` — reactive state. Direct mutation works (e.g., `array.push()`).
- `$state<Type>()` — typed state with generics.
- `$derived(expr)` — computed value from reactive dependencies. Use for **all** derived values.
- `$derived.by(() => { ... })` — multi-line derived computation.
- `$effect(() => { ... })` — side effects only (I/O, DOM, subscriptions). Return cleanup function. Read reactive values **before** any `await`. Prefer `$derived` over `$effect` for pure computations.
- `$props()` — component props with typed interface. Destructure with defaults.
- `$bindable()` — two-way binding prop. Use sparingly.
- `$state.snapshot(obj)` — deep clone for non-Svelte code (logging, JSON.stringify).

### Component Patterns

- **Props in, callbacks out.** Accept callback props like `onsend: (content: string) => void`.
- **Type all props** with an `interface Props`. Destructure via `let { ... }: Props = $props()`.
- **Spread rest props** for wrapper components: `let { variant, ...rest }: Props = $props()`.
- **Small components** — extract sub-components at ~150 lines.
- **PascalCase** naming: `ChatMessage.svelte`.
- **Snippets** (`{@render}`) for content projection. Type with `Snippet` from `svelte`.
- **DOM events** use lowercase attributes: `onclick`, `oninput`, `onsubmit`.

### State Management

1. **Local `$state`** — single component.
2. **Props** — parent-child flow.
3. **`.svelte.ts` modules** — shared state across components. Encapsulate as IIFE with getters/setters:
   ```ts
   export const store = (() => {
     let value = $state<Type | null>(null);
     return {
       get value() { return value; },
       setValue(v: Type) { value = v; },
     };
   })();
   ```
4. **SvelteKit `load` functions** — route-level server data.

No legacy Svelte stores (`writable`/`readable`) in this codebase.

### Reactive Collections

Use `SvelteMap` and `SvelteSet` from `svelte/reactivity` for reactive Map/Set types (used in WebSocket subscription tracking).

### Data Fetching

- **Load functions** in `+page.ts` / `+page.server.ts` for route data.
- **API client** in `$lib/utils/api.ts` — generic `api<T>(path, options)` that prepends `/api/v1`, unwraps `{ data: T }` envelope, throws `ApiError` on failure.
- **Service modules** in `$lib/services/` — thin wrappers around `api()` per domain (auth, conversations, mcp).
- **WebSocket** via singleton in `$lib/stores/websocket.svelte.ts` — auto-reconnect with exponential backoff, subscription model per conversation, message queuing while disconnected, 30s ping keepalive.

### Routing

- **Layout groups**: `(app)/` for authenticated pages, `(public)/` for login/register.
- **Auth guards** in layout `load` functions — redirect unauthenticated users to `/login`, authenticated users away from auth pages.
- **Error pages** (`+error.svelte`) at multiple levels.
- **Dynamic routes**: `/chat/[id]`.

### Styling

- **Tailwind CSS v4** — all inline classes, no abstraction layer.
- Color palette: `zinc-*` (neutrals), `emerald` (success), `amber` (warning), `red` (danger).
- **Dark mode**: `dark:` prefix classes, respects system preference.
- **Conditional classes**: array syntax `class={['base', condition && 'active']}`.
- **Scoped `<style>` blocks** only for custom animations (`@keyframes`).
- **Typography plugin** for markdown content (`@tailwindcss/typography`).

### TypeScript

- **Strict mode** enabled.
- Shared types in `$lib/types/index.ts` — domain types mirror backend shapes.
- WebSocket messages: discriminated unions (`ClientWsMessage`, `ServerWsMessage`).
- `ApiError` class wraps HTTP errors with status and code.

### SSR Safety

Global `.svelte.ts` state is shared across server requests. Never mutate during SSR — only read. Mutations happen client-side (event handlers, `$effect`, `onMount`).
