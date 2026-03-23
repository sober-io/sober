# Documentation Design

## Overview

Add project documentation in two layers: a concise README.md as the GitHub landing page, and a full mdBook-powered docs site deployed to GitHub Pages. Target audience is both self-hosters and developers/contributors.

## Prerequisites

Before implementation, ensure:

- **LICENSE file** — Create `LICENSE` (MIT) in repo root. `Cargo.toml` declares MIT but no LICENSE file exists.
- **`install.sh`** — Exists at `scripts/install.sh`. Curl-pipe-bash install pattern that fetches latest GH release. Supports install, upgrade, and uninstall modes with systemd service management. **Bug:** `GITHUB_REPO` on line 13 is set to `harrisiirak/s-ber` — must be fixed to `sober-io/sober`.
- **Cargo.toml repository URL** — `backend/Cargo.toml` has `repository = "https://github.com/harrisiirak/s-ber"` but the actual repo is `https://github.com/sober-io/sober`. Fix to match.
- **`.gitignore`** — Add `docs/book/build/` to prevent committing mdBook build output.

## README.md

New file: `README.md` in repo root. ~150-200 lines. Structure:

1. **Header** — Project name (Sõber), tagline ("Self-evolving AI agent system"), badges (CI status, license, docs link)
2. **What is Sõber?** — 3-4 sentences: what it does, why it exists
3. **Features** — Bullet list: multi-agent orchestration, WASM plugin system, scoped memory/context, multi-provider LLM support, CLI admin tools, PWA frontend, scheduling
4. **Quick Start** — Prerequisites (Docker) + `docker compose up -d` for fastest path. Link to docs for alternatives.
5. **Installation** — One-liner curl-pipe-bash install (`curl -fsSL https://raw.githubusercontent.com/sober-io/sober/main/scripts/install.sh | sudo bash`), Docker, and from-source paths. Links to docs for details.
6. **Development** — `just --list`, key justfile commands, link to contributing docs
7. **Architecture** — One simplified Mermaid system diagram + link to full architecture docs
8. **Documentation** — Link to GitHub Pages docs site
9. **License** — MIT

## Docs Site (mdBook)

### Location

`docs/book/` — separate from internal `docs/plans/` and dev reference files.

### Tooling

- **mdBook** — Rust-native static site generator
- **mdbook-mermaid** — Preprocessor for Mermaid diagram rendering
- Deployed to GitHub Pages via CI

### Site Structure

```
docs/book/
├── book.toml
├── theme/                  # Optional CSS overrides
└── src/
    ├── SUMMARY.md          # Table of contents (drives sidebar)
    ├── introduction.md
    ├── getting-started/
    │   ├── prerequisites.md
    │   ├── installation.md     # scripts/install.sh (binary), Docker, from source
    │   ├── configuration.md    # config.toml, SOBER_* env vars, layered resolution
    │   └── first-run.md        # Walk through first conversation
    ├── user-guide/
    │   ├── cli.md              # sober + soberctl usage
    │   ├── workspaces.md       # Workspace setup, .sober/ dir
    │   ├── conversations.md    # Chat, groups, settings
    │   ├── memory.md           # How memory/context works (user-facing)
    │   ├── mcp.md              # MCP server connections
    │   ├── tools.md             # All 18 built-in agent tools
    │   ├── scheduling.md       # Jobs, cron, intervals
    │   └── frontend.md         # SvelteKit PWA: install, build, dev server
    ├── plugins/
    │   ├── overview.md         # What plugins are, lifecycle
    │   ├── quickstart.md       # Build your first plugin
    │   ├── manifest.md         # plugin.toml format
    │   ├── capabilities.md     # The 11 host functions
    │   └── examples.md         # Example plugins
    ├── architecture/
    │   ├── overview.md         # System diagram (Mermaid), process model
    │   ├── crates.md           # Crate map + dependency graph (Mermaid)
    │   ├── memory-system.md    # BCF, scoped memory, vector storage
    │   ├── security.md         # Auth stack, injection defense, sandboxing
    │   ├── agent-mind.md       # Identity, soul.md, prompt assembly
    │   └── event-delivery.md   # Subscription model, scheduler routing
    └── contributing.md         # Dev setup, PR workflow, coding standards
```

### Diagrams

All diagrams use Mermaid syntax, rendered by mdbook-mermaid. Diagram types:

- System architecture overview (flowchart)
- Crate dependency graph (flowchart)
- Memory scoping hierarchy (flowchart)
- BCF binary layout diagram
- Memory context loading pipeline (sequence diagram)
- Plugin lifecycle (flowchart)
- Event delivery / subscription model (sequence diagram)
- Prompt injection defense pipeline (sequence diagram)
- Self-evolution scope (flowchart)

## CI & Deployment

New workflow: `.github/workflows/docs.yml`

- **Trigger:** Push to `main` touching `docs/book/**` or `README.md`
- **Steps:** Install mdbook + mdbook-mermaid (via cargo-binstall), run `mdbook-mermaid install` to inject Mermaid JS assets, build book, deploy to GitHub Pages via `actions/deploy-pages`
- GitHub Pages configured to deploy from GitHub Actions (not branch-based). One-time manual setup: repo Settings > Pages > Source > "GitHub Actions".

## Content Strategy

### Written fresh

- Introduction, Quick Start, First Run
- CLI usage documentation with examples
- Configuration reference (config.toml structure, all `SOBER_*` env vars with defaults, layered resolution)
- Plugin quickstart tutorial
- Built-in tools reference (all 18 agent tools with descriptions and usage)
- Frontend page (prerequisites, pnpm install, dev server, production build, how sober-web embeds it)
- Contributing guide (external audience)

### Repurposed from existing sources

| Source | Target |
|--------|--------|
| ARCHITECTURE.md system diagram | `architecture/overview.md` (converted to Mermaid) |
| ARCHITECTURE.md crate map | `architecture/crates.md` (converted to Mermaid, inventory all 18 workspace crates — ARCHITECTURE.md only lists 16, missing `sober-plugin-gen` and `sober-skill`) |
| ARCHITECTURE.md memory section | `architecture/memory-system.md` (deep dive on BCF binary format and Qdrant vector retrieval) |
| ARCHITECTURE.md security model | `architecture/security.md` (document current auth state honestly — password only; mention planned methods separately) |
| ARCHITECTURE.md agent mind section | `architecture/agent-mind.md` |
| ARCHITECTURE.md event delivery | `architecture/event-delivery.md` |
| ARCHITECTURE.md plugin system | `plugins/overview.md` + `plugins/capabilities.md` |
| CLAUDE.md dev rules | `contributing.md` (rewritten for external audience) |
| `AppConfig` struct / `config.toml` / `.env` | `getting-started/configuration.md` |
| `sober --help` / `soberctl --help` / `sober config` subcommands | `user-guide/cli.md` |
| `backend/crates/sober-agent/src/tools/` | `user-guide/tools.md` (all 17 built-in tools) |
| `scripts/install.sh` | `getting-started/installation.md` (document flags: `--user`, `--version`, `--yes`, `--uninstall`, `--database-url`, `--llm-*`) |

### Stays internal (not in docs site)

- `docs/plans/` — internal planning documents
- `CLAUDE.md` — AI assistant instructions
- `docs/rust-patterns.md`, `docs/svelte-patterns.md` — linked from contributing page, not duplicated

## book.toml

```toml
[book]
title = "Sõber Documentation"
authors = ["Sõber Contributors"]
language = "en"
multilingual = false
src = "src"

[build]
build-dir = "build"

[preprocessor.mermaid]
command = "mdbook-mermaid"

[output.html]
git-repository-url = "https://github.com/sober-io/sober"
edit-url-template = "https://github.com/sober-io/sober/edit/main/docs/book/{path}"
```

## Justfile Addition

```just
# Build documentation site
docs-build:
    cd docs/book && mdbook build

# Serve documentation locally with hot reload
docs-serve:
    cd docs/book && mdbook serve --open
```
