# Frontend

The Sõber web interface is a SvelteKit Progressive Web App (PWA) built with Svelte 5 and Tailwind CSS v4. It is served by `sober-web`, which either embeds the compiled assets directly in the binary or serves them from a configured directory on disk.

---

## Prerequisites

| Tool | Version | Installation |
|------|---------|-------------|
| Node.js | 24 | [nodejs.org](https://nodejs.org) or `nvm install 24` |
| pnpm | latest | `npm install -g pnpm` |

---

## Setup

```bash
cd frontend
pnpm install
```

This installs all dependencies from `package.json` into `node_modules/`.

---

## Development

```bash
pnpm dev
```

Starts the Vite development server on port **5173**. The dev server:

- Hot-reloads on code changes.
- Proxies API requests to `sober-api` (configured in `vite.config.ts`).
- Proxies WebSocket connections to `sober-api`.

For the full stack to work in development, `sober-api` and `sober-agent` must be running. The quickest way to start all services is:

```bash
# From the repository root
docker compose up -d         # Start PostgreSQL, Qdrant, SearXNG
just dev                     # Start backend services + frontend dev server
```

---

## Production Build

```bash
pnpm build
```

Compiles the SvelteKit app to `frontend/build/`. The output is a Node.js-compatible SvelteKit adapter build (adapter-node) that `sober-web` can serve.

To inspect the build output size:

```bash
pnpm build && ls -lh frontend/build/
```

---

## Type Checking

```bash
pnpm check
```

Runs `svelte-check` and TypeScript's type checker across the entire frontend. This validates:

- Svelte component props and slot types.
- TypeScript types in `.ts` and `.svelte` files.
- SvelteKit route parameter and layout types.

Run this before committing frontend changes.

---

## Testing

```bash
pnpm test
```

Runs the Vitest test suite. Tests are located alongside their source files with a `.test.ts` suffix or in `__tests__/` directories.

To run tests in watch mode during development:

```bash
pnpm test --watch
```

---

## How sober-web Serves the Frontend

`sober-web` is a Rust binary (`sober-web` crate) that:

1. **Serves static files** — either embedded at compile time via `rust-embed` or served from a configurable directory on disk.
2. **Reverse-proxies API requests** — all requests to `/api/*` are forwarded to `sober-api`.
3. **Reverse-proxies WebSocket connections** — `/api/v1/ws` is proxied to maintain the WebSocket upgrade.
4. **Handles SPA fallback** — unknown routes return `index.html` so SvelteKit's client-side router handles navigation.

```
Browser ──/── sober-web ──/api/*──► sober-api
                │
                └── /static/* ──► embedded assets (or disk)
                └── /* ──► index.html (SPA fallback)
```

### Embedded vs. disk assets

**Embedded (default):** The SvelteKit build output is embedded in the `sober-web` binary at compile time using `rust-embed`. No external files are needed at runtime. This is the production default.

**Disk (development/override):** Set `static_dir` in `config.toml` to serve files from a directory:

```toml
[web]
static_dir = "/var/lib/sober/static"
```

This is useful when iterating on the frontend without recompiling `sober-web`.

### Web server configuration

```toml
[web]
host = "0.0.0.0"
port = 8080                               # Public-facing port
api_upstream_url = "http://localhost:3000"  # sober-api address
# static_dir = "/path/to/static"          # Uncomment for disk-based assets
```

---

## Project Structure

```
frontend/
  src/
    lib/
      components/     # Reusable Svelte components
      services/       # API service modules (auth, conversations, mcp)
      stores/         # Shared reactive state (websocket.svelte.ts, etc.)
      types/          # TypeScript type definitions mirroring backend shapes
      utils/          # API client, helpers
    routes/
      (app)/          # Authenticated pages (chat, settings, etc.)
      (public)/       # Public pages (login, register)
  static/             # Static assets (icons, manifest.json for PWA)
  package.json
  svelte.config.js
  vite.config.ts
```

---

## Key Implementation Details

### Svelte 5 runes

The frontend uses Svelte 5's rune syntax exclusively — `$state`, `$derived`, `$effect`, `$props`. No legacy Svelte 4 patterns (`export let`, `$:` reactive statements, `createEventDispatcher`).

### WebSocket

A singleton WebSocket connection in `$lib/stores/websocket.svelte.ts` handles all real-time updates:

- Auto-reconnects with exponential backoff on disconnect.
- Queues outbound messages while disconnected.
- Maintains per-conversation subscriptions.
- Sends a ping every 30 seconds to keep the connection alive.

### API client

`$lib/utils/api.ts` provides a generic `api<T>(path, options)` function that:

- Prepends `/api/v1` to all paths.
- Unwraps the `{ "data": T }` success envelope.
- Throws `ApiError` (with HTTP status and error code) on non-2xx responses.

### Authentication

Auth guards live in SvelteKit layout `load` functions. The `(app)` layout redirects unauthenticated users to `/login`. The `(public)` layout redirects already-authenticated users to `/chat`.

### Styling

Tailwind CSS v4 with inline utility classes. Color palette: `zinc-*` for neutrals, `emerald` for success states, `amber` for warnings, `red` for errors. Dark mode via `dark:` classes respecting the system preference.
