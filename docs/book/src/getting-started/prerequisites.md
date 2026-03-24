# Prerequisites

The software you need depends on how you plan to deploy Sõber. Choose the method that fits
your situation and confirm you have the listed dependencies before continuing to
[Installation](installation.md).

---

## Binary Install (Recommended for Production)

The `install.sh` script downloads a pre-built release, creates a system user, installs
services, and generates a `config.toml`. This is the fastest path to a production deployment.

**Platform requirements:**

- Linux x86_64 or aarch64
- systemd (for service management)
- `curl` or `wget`
- `sudo` / root access

**External services (you must provide these):**

| Service | Version | Purpose |
|---------|---------|---------|
| PostgreSQL | 17+ | Primary relational database |
| Qdrant | Latest stable | Vector store for memory and embeddings |
| SearXNG | Latest stable | Web search aggregation (required for `web_search` tool) |

Qdrant and SearXNG can be run alongside Sõber using Docker even when the Sõber binaries are
installed directly. PostgreSQL should be a managed instance or a dedicated server in production.
See [Search Setup](search-setup.md) for SearXNG installation options.

---

## Docker Compose

The Docker Compose setup includes PostgreSQL, Qdrant, SearXNG, and all Sõber services in a
single `compose.yml`. This is the easiest way to get a full working stack running locally or on
a single server.

| Dependency | Minimum Version |
|------------|----------------|
| Docker Engine | 24.0 |
| Docker Compose | v2 (the `docker compose` plugin, not `docker-compose`) |

No external services are required — the Compose file provisions everything.

---

## From Source

Building from source is recommended for contributors or when you need to modify Sõber itself.

**Build-time dependencies:**

| Dependency | Version | Notes |
|------------|---------|-------|
| Rust | Latest stable | Install via [rustup](https://rustup.rs) |
| Node.js | 24 | Required for the SvelteKit frontend |
| pnpm | Latest stable | Frontend package manager (`npm install -g pnpm`) |
| protobuf-compiler (`protoc`) | 3.x | Required to compile `.proto` files |
| Docker Engine | 24.0 | Required for integration tests and sqlx compile-time checks |
| Docker Compose | v2 | Required for the full dev stack |

**Installing `protoc` on common platforms:**

```bash
# Debian / Ubuntu
sudo apt install -y protobuf-compiler

# Fedora / RHEL
sudo dnf install -y protobuf-compiler

# macOS
brew install protobuf
```

**Note on sqlx:** Sõber uses `sqlx` with compile-time query verification. The `.sqlx/` cache
directory is committed to the repository so that CI and local builds can run without a live
database. If you add or modify queries you will need Docker running to regenerate the cache with
`cargo sqlx prepare`.

---

## Compiling WASM Plugins

Writing and compiling Sõber plugins (WASM modules) requires:

| Dependency | Notes |
|------------|-------|
| Rust | Install via [rustup](https://rustup.rs) |
| `wasm32-wasip1` Rust target | `rustup target add wasm32-wasip1` |

No other build tools are required for basic plugin development. See the
[Plugin Quickstart](../plugins/quickstart.md) for step-by-step instructions.

---

## LLM Provider

All deployment methods require access to an LLM API. Sõber supports any OpenAI-compatible
endpoint:

| Provider | Notes |
|----------|-------|
| [OpenRouter](https://openrouter.ai) | Recommended default; routes to many models |
| [OpenAI](https://platform.openai.com) | Direct API access |
| [Ollama](https://ollama.com) | Local models; set `SOBER_LLM_BASE_URL=http://localhost:11434/v1` |
| Any OpenAI-compatible API | Set base URL and model name accordingly |

You will need an API key (or a running local server) before completing the configuration step.

---

## Next Step

Once you have the required dependencies, continue to [Installation](installation.md).
