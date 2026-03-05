## Svelte 5 Frontend — Patterns & Approaches

### SvelteKit Setup

- Use **SvelteKit** with the **static adapter** or **node adapter** depending on deployment (start with static if the Rust backend serves the API).
- **TypeScript** everywhere — `<script lang="ts">`.
- **Svelte 5 runes only** — do NOT use legacy `$:` reactive declarations, `export let` for props, `createEventDispatcher`, or `<slot>`. These are all deprecated patterns.

### Runes — The Reactivity System

Svelte 5 replaces implicit reactivity with explicit **runes** — compiler-recognized symbols that make reactive intent clear. Runes work in both `.svelte` files and `.svelte.ts` / `.svelte.js` modules.

**`$state` — Reactive state:**
```svelte
<script lang="ts">
  let count = $state(0);
  let items = $state<string[]>([]);

  // Direct mutation works — no need for reassignment like in Svelte 4
  function addItem(item: string) {
    items.push(item); // this triggers updates!
  }
</script>
```

**`$derived` — Computed values (pure, no side effects):**
```svelte
<script lang="ts">
  let width = $state(10);
  let height = $state(20);

  // Simple expression — recalculates when width or height change
  const area = $derived(width * height);

  // Complex computation — use $derived.by for multi-line logic
  const summary = $derived.by(() => {
    if (area > 100) return 'large';
    if (area > 50) return 'medium';
    return 'small';
  });
</script>
```
Use `$derived` for **all** computed values. Never duplicate state that can be derived.

**`$effect` — Side effects (sparingly!):**
```svelte
<script lang="ts">
  let searchQuery = $state('');

  // Runs after DOM update when dependencies change
  $effect(() => {
    console.log('Query changed:', searchQuery);

    // Return a cleanup function (runs before re-execution or unmount)
    return () => {
      console.log('Cleaning up previous effect');
    };
  });
</script>
```

**Critical `$effect` rules:**
- **Prefer `$derived` over `$effect`** for pure computations. `$effect` is for I/O, DOM interactions, and imperative side effects only.
- **Keep effects focused** — one effect, one job. Don't combine data fetching and DOM manipulation.
- **Dependencies are tracked synchronously.** Values read after `await` or inside `setTimeout` are NOT tracked — access reactive values before any async boundary:
  ```ts
  // BAD: size is not tracked (read after await)
  $effect(() => {
    const res = await fetch('/api');
    console.log(size); // NOT a dependency!
  });

  // GOOD: read size synchronously, then await
  $effect(() => {
    const currentSize = size; // tracked
    fetch(`/api?size=${currentSize}`).then(/* ... */);
  });
  ```
- **Don't use `$effect` to synchronize state** — this is almost always a sign you should use `$derived` instead.
- `$effect` does NOT run during SSR. Use `onMount` for lifecycle work that also needs server awareness.

### Props — `$props()`

Replace `export let` with the `$props()` rune. Always type your props with an interface:

```svelte
<!-- Button.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'primary' | 'secondary';
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  }

  let {
    variant = 'primary',
    disabled = false,
    onclick,
    children,
  }: Props = $props();
</script>

<button class={variant} {disabled} {onclick}>
  {#if children}
    {@render children()}
  {/if}
</button>
```

**Spread remaining props** with the rest pattern for wrapper components:
```svelte
<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  interface Props extends HTMLButtonAttributes {
    variant?: 'primary' | 'secondary';
  }

  let { variant = 'primary', ...rest }: Props = $props();
</script>

<button class={variant} {...rest} />
```

**Bindable props** — use `$bindable` when the parent needs two-way binding:
```svelte
<!-- TextInput.svelte -->
<script lang="ts">
  let { value = $bindable('') }: { value: string } = $props();
</script>

<input bind:value />
```
Only mark props as `$bindable` when genuinely needed. Don't mutate state you don't own.

### Events — Callback Props

Svelte 5 replaces `createEventDispatcher` and `on:` directives with **callback props** and **native DOM event attributes**:

```svelte
<!-- Svelte 5: callback props (do this) -->
<script lang="ts">
  let { onSave }: { onSave?: (data: FormData) => void } = $props();
</script>
<button onclick={() => onSave?.(formData)}>Save</button>

<!-- DOM events use lowercase attributes -->
<input oninput={(e) => (query = e.currentTarget.value)} />
<form onsubmit|preventDefault={handleSubmit}>
```

This increases type safety — the compiler can verify that a component actually accepts a given callback.

### Snippets — Replacing Slots

Svelte 5 replaces `<slot>` with **snippets** and `{@render}`. Snippets are more powerful, more explicit, and composable:

```svelte
<!-- Card.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    header: Snippet;
    children: Snippet;
    footer?: Snippet;
  }

  let { header, children, footer }: Props = $props();
</script>

<div class="card">
  <div class="card-header">{@render header()}</div>
  <div class="card-body">{@render children()}</div>
  {#if footer}
    <div class="card-footer">{@render footer()}</div>
  {/if}
</div>
```

Usage:
```svelte
<Card>
  {#snippet header()}
    <h2>Title</h2>
  {/snippet}

  <p>Body content — this is the implicit `children` snippet.</p>

  {#snippet footer()}
    <button>Action</button>
  {/snippet}
</Card>
```

**Snippets with parameters** (replaces `let:` directive):
```svelte
<!-- List.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props<T> {
    items: T[];
    row: Snippet<[T, number]>;
    empty?: Snippet;
  }

  let { items, row, empty }: Props<any> = $props();
</script>

{#if items.length === 0 && empty}
  {@render empty()}
{:else}
  {#each items as item, index}
    {@render row(item, index)}
  {/each}
{/if}
```

### Shared / Global State with Runes

Runes work in `.svelte.ts` / `.svelte.js` files — this enables **universal reactivity** outside components:

```ts
// $lib/stores/auth.svelte.ts
interface User {
  id: string;
  email: string;
  name: string;
}

// Encapsulate state in a function that returns getters/setters
export const auth = (() => {
  let user = $state<User | null>(null);
  let loading = $state(false);

  return {
    get user() { return user; },
    get loading() { return loading; },
    get isAuthenticated() { return user !== null; },
    setUser(u: User | null) { user = u; },
    setLoading(l: boolean) { loading = l; },
  };
})();
```

**SSR safety warning:** Global module-level state is shared across all requests on the server. Never mutate global state during SSR rendering — only read it. Mutations should happen client-side only (in `$effect`, event handlers, or after `onMount`). Use SvelteKit's `load` functions to provide per-request data.

### Component Patterns

- **Small, single-responsibility components.** If a component file exceeds ~150 lines, extract sub-components.
- **Props in, callbacks out.** Components receive data via `$props()` and communicate upward via callback props.
- Colocate component styles in `<style>` blocks. Svelte's scoped styles are the default; reach for global styles sparingly.
- Name components in **PascalCase**: `UserCard.svelte`.
- **Don't recreate objects/functions inside templates** — memoize or move creation outside the render path to avoid unnecessary work.
- Use `$state.snapshot(obj)` when you need to pass reactive state to non-Svelte code (e.g., `console.log`, third-party libraries, `JSON.stringify`).

### State Management Hierarchy

1. **Local `$state`** — for state used by a single component.
2. **Passed via `$props`** — for parent-child data flow.
3. **Shared `.svelte.ts` modules** — for state shared across unrelated components (replaces most store use cases).
4. **Svelte stores** (`writable`, `readable`) — still work in Svelte 5 and are NOT deprecated, but runes-based shared state is preferred for new code.
5. **SvelteKit `load` functions** — for route-level data that comes from the server.

Keep state as close to where it's used as possible. Avoid deeply nested global state.

### Data Fetching

- Use SvelteKit's **`load` functions** in `+page.ts` / `+page.server.ts` for route-level data loading. This is the primary data-fetching pattern.
- `+page.server.ts` runs only on the server — use for private credentials, DB access.
- `+page.ts` (universal load) runs on both server and client — use for public API calls.
- **Avoid request waterfalls.** Use `Promise.all` in load functions when fetching multiple independent resources. Don't `await parent()` before fetching your own data unless you actually need the parent data.
- Keep API calls in `$lib/utils/api.ts` — a thin typed wrapper around `fetch`:
  ```ts
  export async function api<T>(
    path: string,
    options?: RequestInit
  ): Promise<T> {
    const res = await fetch(`/api/v1${path}`, {
      headers: { 'Content-Type': 'application/json' },
      ...options,
    });
    if (!res.ok) {
      const error = await res.json().catch(() => ({}));
      throw new ApiError(res.status, error);
    }
    return res.json();
  }
  ```
- Handle loading and error states explicitly — use `{#await}` blocks for inline async or manage state manually with `$state`.
- For error handling in load functions, use SvelteKit's `error()` helper to throw proper HTTP errors that render `+error.svelte`.

### Routing

- File-based routing via SvelteKit's `src/routes/` directory.
- Use **layout groups** `(group)` for shared layouts (e.g., `(app)` for authenticated pages, `(public)` for marketing).
- Use `+error.svelte` pages at every level for graceful error boundaries.
- Use `+layout.ts` / `+layout.server.ts` for data shared across child routes (e.g., auth state, navigation data).
- Use **`+server.ts`** files for API endpoints when you need custom server-side logic beyond load functions.

### Styling

- Pick ONE approach and commit: **Tailwind CSS** or **scoped Svelte styles** (not both mixed randomly).
- If Tailwind: install with SvelteKit using the `@tailwindcss/vite` plugin.
- Design tokens (colors, spacing, fonts) in a central config — either Tailwind config or CSS custom properties.
- For dynamic classes, use array syntax: `class={['base', condition && 'active']}`.

### TypeScript

- Strict mode enabled in `tsconfig.json`.
- Shared types in `$lib/types/`.
- API response types should mirror the backend's response shapes.
- Always define a `Props` interface for component props — don't inline anonymous types.
- Use `import type { Snippet } from 'svelte'` for snippet prop types.
- Use `import type { HTMLButtonAttributes } from 'svelte/elements'` for wrapper components extending native elements.
- SvelteKit auto-generates types for `load` functions — use `import type { PageData } from './$types'` in pages.

### What NOT To Use (Legacy / Deprecated)

These patterns are from Svelte 4 and should NOT be used in new Svelte 5 code:
- `export let` for props → use `$props()`
- `$:` reactive declarations → use `$derived` or `$effect`
- `createEventDispatcher()` → use callback props
- `<slot>` and `let:` → use snippets and `{@render}`
- `on:click` directive syntax → use `onclick` attribute
- `<svelte:component>` → dynamic components work directly now
- `beforeUpdate` / `afterUpdate` → use `$effect` or `$effect.pre`
- `$$props` / `$$restProps` → use rest pattern with `$props()`

---
---
