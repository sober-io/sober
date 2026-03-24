# Search Setup (SearXNG)

Sober uses [SearXNG](https://docs.searxng.org/) — a privacy-respecting, open-source meta-search
engine — to power the agent's `web_search` tool. When a user asks the agent to look something up
on the web, it queries your SearXNG instance and returns results.

**SearXNG is required if you want the agent to search the web.** The rest of Sober works fine
without it; only the `web_search` tool depends on it.

> If you installed via **Docker Compose**, SearXNG is already included in the stack — you can skip
> this page.

---

## Option 1: Docker (Recommended)

Run SearXNG as a standalone container using the settings file shipped with Sober:

```bash
docker run -d \
  --name searxng \
  -p 8080:8080 \
  -v "$(pwd)/infra/searxng/settings.yml:/etc/searxng/settings.yml:ro" \
  searxng/searxng
```

This binds SearXNG to port `8080` and uses the minimal configuration from the repository, which
enables JSON output (required by Sober) and disables the rate limiter for local use.

---

## Option 2: Existing Instance

If you already run a SearXNG instance, point Sober at it by setting the URL in your
configuration:

```toml
# config.toml
[searxng]
url = "https://your-searxng-instance.example.com"
```

Or via environment variable:

```bash
export SOBER_SEARXNG_URL="https://your-searxng-instance.example.com"
```

**Important:** Your instance must have **JSON format enabled** in its settings. In the SearXNG
`settings.yml`:

```yaml
search:
  formats:
    - html
    - json
```

Without `json` in the formats list, Sober cannot parse search results.

---

## Verify

Confirm SearXNG is reachable and returning JSON:

```bash
curl -s "http://localhost:8080/search?q=test&format=json" | head -c 200
```

You should see a JSON response containing a `results` array. If you get a connection error,
check that the container is running (`docker ps`) and the port mapping is correct.

---

## Configuration

The default URL is `http://localhost:8080`, which works for local Docker setups. See
[Configuration](configuration.md) for the full `[searxng]` section and environment variable
reference.

| Method | Setting |
|--------|---------|
| TOML | `[searxng]` → `url` |
| Environment variable | `SOBER_SEARXNG_URL` |

---

## Customization

The provided `infra/searxng/settings.yml` is a minimal development configuration. For
production or personal use, you may want to:

- **Choose search engines** — enable or disable specific engines (Google, DuckDuckGo, Brave, etc.)
  in the `engines` section of `settings.yml`.
- **Set a secret key** — replace the development secret in `server.secret_key` with a strong
  random value.
- **Enable rate limiting** — set `server.limiter: true` if the instance is exposed to untrusted
  clients.

See the [SearXNG documentation](https://docs.searxng.org/admin/settings/index.html) for the
full settings reference.

---

## Next Step

Continue to [Installation](installation.md) if you haven't installed Sober yet, or
[Configuration](configuration.md) to review your settings.
