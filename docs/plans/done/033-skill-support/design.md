# #033 — Skill Support (Format + Loader)

## Problem

Sõber has no mechanism for users to extend the agent's capabilities with reusable,
structured instruction sets. Users cannot create, share, or install skill packages
that teach the agent specialized workflows. The existing BCF `Skill` chunk type
stores learned behaviors in vector memory, but there is no system for curated,
versioned, trigger-based skill activation.

## Goals

1. Define a skill format that is **100% compatible** with the
   [Agent Skills specification](https://agentskills.io/specification) — bidirectional.
   Sõber skills work in other compliant agents; external skills work in Sõber.
2. Implement filesystem-based skill discovery with progressive disclosure
   (catalog → instructions → resources).
3. Provide two activation paths: **model-driven** (automatic, context-based) and
   **user-driven** (slash commands).
4. Expose skill metadata via API for frontend slash command registration.
5. Protect activated skill content from context compaction.

## Non-Goals (Future Work)

- Bundled base skills shipped with Sõber.
- Per-user / per-conversation skill enable/disable (requires DB — planned for
  management phase).
- Tool-based executable skills (WASM sandboxed — Plan #019).
- Self-evolution skill proposal loop (Plan #030).
- Settings UI for skill management.
- Skill usage analytics.

## Prior Art

### Agent Skills Specification (agentskills.io)

Open format for extending agent capabilities. Key design:

- **Directory-per-skill** with a `SKILL.md` file (YAML frontmatter + markdown body).
- **Progressive disclosure**: metadata (~100 tokens) at startup → full body on
  activation → resources on demand.
- **Frontmatter fields**: `name` (required), `description` (required), `license`,
  `compatibility`, `metadata`, `allowed-tools`.
- **Supporting directories**: `scripts/`, `references/`, `assets/`.
- **Cross-client discovery**: `.agents/skills/` convention at project and user level.

### Plan #032 — Structured Instruction Directory

Recently completed. Established the `InstructionLoader` pattern in `sober-mind`:
filesystem scanning, YAML frontmatter parsing, three-layer resolution
(base → user → workspace), visibility filtering, priority sorting, and caching.
The skill loader follows the same architectural pattern.

### Existing BCF Skill Chunks

`ChunkType::Skill` in `sober-memory` stores learned behaviors as vector-indexed
memory chunks. These are complementary to SKILL.md files:

| Aspect | SKILL.md files | BCF Skill chunks |
|--------|---------------|-----------------|
| Created by | Human or approved proposal | Agent `remember` tool |
| Discovery | Trigger matching via catalog | Semantic search via `recall` |
| Activation | Automatic or slash command | On-demand via recall |
| Persistence | Filesystem | Qdrant + BCF containers |
| Example | "When reviewing code, follow these steps..." | "User prefers SQL with CTEs" |

A future evolution path: the agent notices repeated BCF skill recalls → proposes
promoting them to a SKILL.md for reliable, trigger-based activation.

---

## Design

### 1. Skill File Format

Each skill is a directory containing a `SKILL.md` file. The format follows the
Agent Skills specification exactly — no custom top-level frontmatter fields.

```markdown
---
name: code-review
description: >
  Reviews code changes for quality, security, and style issues.
  Use when the user asks to review code, check a PR, or audit changes.
metadata:
  author: user
  version: "1.0"
---

## Instructions

When reviewing code, follow these steps:
1. Check for security vulnerabilities (OWASP top 10)
2. Verify error handling patterns
...

## Available scripts

- **`scripts/analyze.py`** — Static analysis helper
```

**Sõber-specific extensions** use the `metadata` map (spec-compliant):

```yaml
metadata:
  author: user
  version: "1.0"
  sober.priority: "50"           # activation priority (0-100)
  sober.default_enabled: "true"  # whether active by default
```

Supporting directories follow the spec:

```
code-review/
├── SKILL.md
├── scripts/         # executable scripts the agent can run
├── references/      # additional docs loaded on demand
└── assets/          # templates, resources
```

### 2. Directory Structure & Discovery

Skills are discovered from these locations, scanned at session startup:

| Scope | Path | Purpose |
|-------|------|---------|
| Workspace | `.sober/skills/` | Project-specific skills |
| Workspace | `.agents/skills/` | Cross-client interop (project) |
| User | `~/.sober/skills/` | Personal skills |
| User | `~/.agents/skills/` | Cross-client interop (user) |

No base skills are shipped. Users create or install skills into these directories.

**Scan rules:**
- Look for subdirectories containing `SKILL.md`.
- Skip `.git/`, `node_modules/`, and similar.
- Max depth: 4 levels. Max directories: 2000.
- Respect `.gitignore` for workspace paths.

**Name validation** (per Agent Skills spec):
- 1-64 characters, lowercase alphanumeric + hyphens only.
- Must not start/end with hyphen, no consecutive hyphens.
- Should match parent directory name (warn if mismatch, load anyway).
- Empty directories or directories without `SKILL.md` are silently skipped.

**Name collision resolution:**
- Workspace skills override user skills (same name → workspace wins).
- Within the same scope, first-found wins. Log a warning on collision.
- Key is the frontmatter `name` field, not the directory name.

**Trust:** Workspace-level skills come from potentially untrusted repositories.
The agent's existing permission system gates file reads and bash execution for
scripts referenced by skills.

### 3. Progressive Disclosure

Three tiers, per the Agent Skills spec:

**Tier 1 — Catalog (every prompt, ~50-100 tokens/skill):**

```xml
<available_skills>
  <skill name="code-review" path="/home/user/.sober/skills/code-review/SKILL.md">
    Reviews code changes for quality, security, and style issues.
    Use when the user asks to review code, check a PR, or audit changes.
  </skill>
</available_skills>
```

Injected into the system prompt after all instruction files but before tool
definitions. In the `Mind::build_system_prompt` pipeline, this is a new step
between instruction concatenation and tool metadata appending. The LLM sees
what skills exist and can decide when to activate them.

Behavioral instruction prepended to the catalog:

```
The following skills provide specialized instructions for specific tasks.
When a task matches a skill's description, call the activate_skill tool
with the skill's name to load its full instructions.
```

**Tier 2 — Instructions (on activation, <5000 tokens recommended):**

Full `SKILL.md` body loaded when the LLM calls `activate_skill` or the user
types a slash command. Returned wrapped in identifying tags:

```xml
<skill_content name="code-review">
[skill body — frontmatter stripped]

Skill directory: /home/user/.sober/skills/code-review
Relative paths are relative to the skill directory.

<skill_resources>
  <file>scripts/analyze.py</file>
  <file>references/owasp-checklist.md</file>
</skill_resources>
</skill_content>
```

**Tier 3 — Resources (as needed):**

The LLM reads files from `scripts/`, `references/`, `assets/` using its
standard file-read tool. Skill directories are allowlisted for file access.

### 4. The `activate_skill` Tool

Registered in the `ToolRegistry` alongside built-in tools.

**Schema (presented to the LLM):**

```json
{
  "name": "activate_skill",
  "description": "Load specialized skill instructions into the conversation. Call when a task matches an available skill's description.",
  "input_schema": {
    "type": "object",
    "properties": {
      "name": {
        "type": "string",
        "enum": ["code-review", "sql-helper"],
        "description": "The skill to activate"
      }
    },
    "required": ["name"]
  }
}
```

The `enum` is dynamically populated from the catalog at session start.

**Execution flow:**

1. Look up `name` in `SkillCatalog`.
2. If not found → return error.
3. Check per-conversation activation set → if already activated, return
   "Skill already active."
4. Read `SKILL.md` from disk, strip frontmatter. If `compatibility` field
   is present, include it in the response header so the model knows runtime
   requirements.
5. Enumerate files in skill directory (scripts, references, assets).
6. Return `<skill_content>` wrapped response.
7. Mark as activated in per-conversation state.

**Properties:**
- `context_modifying: true` — changes what the LLM knows.
- `internal: true` — skill content is instructional; no need to forward over
  WebSocket to the frontend.

### 5. Slash Command Registration

Every skill in the catalog is automatically a slash command by its name.

**API endpoint:** `GET /api/v1/skills`

Proxied via the agent's gRPC service (`ListSkills` RPC) — the API does not scan
the filesystem itself. This keeps the API as a thin gateway and ensures a single
source of truth for the skill catalog (the agent process).

```json
{
  "data": [
    {
      "name": "code-review",
      "description": "Reviews code changes for quality, security, and style issues."
    }
  ]
}
```

**Frontend integration:**
1. Fetch `/api/v1/skills` on app load and on workspace change (workspace-level
   skills may differ per project).
2. Merge with built-in commands for the `/` autocomplete menu.
3. When user types `/code-review`, the frontend sends a regular user message
   with the user's text. The backend intercepts the `/skill-name` prefix,
   activates the skill (loads its content into the conversation context), and
   strips the prefix before passing the message to the LLM. The model receives
   the skill content as a system-injected block — it does not need to call
   `activate_skill` itself.
4. If the user types `/code-review review my latest changes`, the skill is
   activated and "review my latest changes" is the user message.

### 6. Context Protection

Activated skill content must survive context compaction:

- `<skill_content>` tags identify skill-injected content.
- When context compaction is implemented (future work), the compaction engine
  will recognize these tags and exempt them from pruning. Until then, skill
  content is treated as regular tool output in the conversation.
- Deduplication: if the LLM or user tries to activate an already-active skill,
  no duplicate injection occurs. Activation state is tracked per conversation.

### 7. Catalog Scoping & Lifecycle

The skill catalog is **scoped per user+workspace**, not global. Different
users have different home directories (`~/.sober/skills/`), and different
conversations may operate in different workspaces (`.sober/skills/`).

`SkillLoader` manages a cache keyed by `(user_home, workspace_path)`.
On each request it checks whether a cached catalog exists and is still
fresh. If so, it returns the cached version. Otherwise it scans and
rebuilds.

```rust
pub struct SkillLoader {
    cache: RwLock<HashMap<CacheKey, CachedCatalog>>,
}

struct CacheKey {
    user_home: PathBuf,
    workspace: PathBuf,
}

struct CachedCatalog {
    catalog: Arc<SkillCatalog>,
    loaded_at: Instant,
}
```

**Cache invalidation:** TTL-based (5 minutes). After the TTL expires,
the next request triggers a rescan. This is simple, predictable,
and sufficient for a small number of skill directories. Filesystem
watchers (inotify) can be added later if needed.

**Conversation lifecycle:**
- **Each turn:** Loader returns cached or freshly-built catalog for the
  user's home + active workspace. Catalog injected into system prompt.
  `activate_skill` tool enum constrained to catalog entries.
- **During conversation:** Activation state tracked per conversation (not
  global). Already-activated skills are not re-injected.
- **Conversation end:** Activation state discarded.

**`ListSkills` RPC:** Accepts user context (home directory, workspace path)
so the API can pass it through. The agent delegates to `SkillLoader` which
returns cached or fresh results.

Skill file changes take effect after the cache TTL expires.

### 8. Frontmatter Parsing

**Lenient validation** — warn on issues but load the skill when possible:

- Name doesn't match parent directory name → warn, load anyway (per spec
  implementation guide recommendation).
- Name exceeds 64 characters → warn, load anyway.
- Unquoted YAML values with colons → attempt recovery by wrapping in quotes
  before retrying parse (common cross-client compatibility issue).
- Description missing or empty → skip the skill (description is essential
  for the catalog), log error.
- YAML completely unparseable → skip the skill, log error.

The `allowed-tools` field is parsed as a space-delimited string but not
enforced in v1 (marked experimental in the spec). Stored for future use.

---

## Crate: `sober-skill`

New library crate with single responsibility: skill discovery, parsing, and
activation.

### Module Structure

```
backend/crates/sober-skill/
├── Cargo.toml
└── src/
    ├── lib.rs              # re-exports
    ├── loader.rs           # SkillLoader: scan dirs, parse SKILL.md
    ├── catalog.rs          # SkillCatalog: in-memory index, activation tracking
    ├── frontmatter.rs      # Agent Skills spec frontmatter parsing
    ├── tool.rs             # ActivateSkillTool: implements Tool trait
    └── types.rs            # SkillEntry, SkillFrontmatter, SkillSource
```

### Key Types

```rust
/// Agent Skills spec frontmatter
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<BTreeMap<String, String>>,
    pub allowed_tools: Option<Vec<String>>,  // parsed from space-delimited string
}

/// Discovered skill entry
pub struct SkillEntry {
    pub frontmatter: SkillFrontmatter,
    pub path: PathBuf,       // absolute path to SKILL.md
    pub base_dir: PathBuf,   // parent dir (skill directory root)
    pub source: SkillSource,
}

/// Where a skill was found
pub enum SkillSource {
    User,
    Workspace,
}

/// Skill index — built per request from user home + workspace path.
/// Keyed by frontmatter `name` field (not directory name).
/// When two skills have the same name at different scopes,
/// workspace overrides user (resolved during loading).
pub struct SkillCatalog {
    skills: HashMap<String, SkillEntry>,
}

/// Caching skill loader — keyed by (user_home, workspace_path),
/// returns cached catalog if within TTL, rescans otherwise.
pub struct SkillLoader {
    cache: RwLock<HashMap<(PathBuf, PathBuf), CachedCatalog>>,
    ttl: Duration,  // default: 5 minutes
}

impl SkillLoader {
    pub fn get(&self, user_home: &Path, workspace: &Path) -> Result<Arc<SkillCatalog>, SkillError>;
}

/// Per-conversation activation tracking — created fresh for each
/// conversation, discarded when the conversation ends.
pub struct SkillActivationState {
    activated: HashSet<String>,
}
```

### Dependencies

```toml
[dependencies]
sober-core = { path = "../sober-core" }
serde = { version = "...", features = ["derive"] }
serde_yml = "0.0.12"       # YAML frontmatter parsing (same as sober-mind)
tokio = { version = "...", features = ["fs"] }
tracing = "..."
```

### Dependency Graph

```
sober-agent ────→ sober-skill  (for tool registration + catalog)
sober-mind ─────→ sober-skill  (for catalog prompt injection)
sober-skill ────→ sober-core   (for Tool trait, error types)
```

`sober-api` does **not** depend on `sober-skill`. The API serves skill data
by proxying the agent's `ListSkills` gRPC RPC. This keeps the API as a thin
gateway and avoids duplicate filesystem scanning.

---

## API Changes

### New Endpoint

`GET /api/v1/skills` — returns available skills for frontend slash menu.

Proxied to the agent process via a `ListSkills` gRPC RPC. The API does not
hold or manage the skill catalog — the agent is the single source of truth.

**Response:**
```json
{
  "data": [
    { "name": "code-review", "description": "..." },
    { "name": "sql-helper", "description": "..." }
  ]
}
```

Requires authenticated session (skills may vary per user in the future).

### gRPC Service Changes

New RPC on the agent gRPC service:

```protobuf
rpc ListSkills(ListSkillsRequest) returns (ListSkillsResponse);

message ListSkillsRequest {
  string user_id = 1;
  optional string conversation_id = 2;  // scopes to conversation's workspace
}

message SkillInfo {
  string name = 1;
  string description = 2;
}

message ListSkillsResponse {
  repeated SkillInfo skills = 1;
}
```

The agent resolves filesystem paths internally from `user_id` (home
directory) and `conversation_id` (workspace association). Callers
don't need to know filesystem layout.

---

## Frontend Changes

### Slash Command Integration

1. New service: `$lib/services/skills.ts` — fetches `/api/v1/skills`.
2. Chat input component: merge skill commands into `/` autocomplete alongside
   built-in commands. Skill commands shown with a distinct visual indicator.
3. On skill command selection: send message with `/skill-name` prefix. The
   backend intercepts and activates the skill; remainder becomes user message.

### No Settings UI (v1)

Skill management (enable/disable, configure) is deferred. Users manage skills
by adding/removing directories from the filesystem.

---

## Testing Strategy

### Unit Tests (sober-skill)

- Frontmatter parsing: valid, malformed YAML, missing required fields.
- Name validation: per spec constraints (lowercase, no consecutive hyphens,
  directory name mismatch warning, etc.).
- Loader: scan test fixtures directory, verify catalog contents.
- Collision resolution: workspace overrides user, warning logged.
- Activation: first activation succeeds, duplicate returns "already active."

### Integration Tests

- `activate_skill` tool execution with real skill directories.
- Prompt assembly with skill catalog injection (sober-mind integration).
- API endpoint returns correct skill list.

### Test Fixtures

```
tests/fixtures/skills/
├── valid-skill/
│   ├── SKILL.md
│   └── scripts/test.sh
├── malformed-frontmatter/
│   └── SKILL.md
├── missing-description/
│   └── SKILL.md
└── name-collision/
    ├── user/skill-a/SKILL.md
    └── workspace/skill-a/SKILL.md
```

---

## Version Bumps

| Crate | Current | New | Reason |
|-------|---------|-----|--------|
| `sober-skill` | — | `0.1.0` | New crate |
| `sober-mind` | `0.5.0` | `0.6.0` | Skill catalog prompt injection |
| `sober-agent` | `0.11.0` | `0.12.0` | activate_skill tool + ListSkills RPC |
| `sober-api` | current | +minor | /api/v1/skills proxy endpoint |

`sober-skill` defines its own `SkillError` enum with `From<SkillError> for AppError`,
following the existing pattern (no changes to `sober-core`).

---

## Relationship to Other Plans

| Plan | Relationship |
|------|-------------|
| #019 (sober-plugin) | Skills are prompt-based; plugins are executable WASM. Complementary, not overlapping. |
| #030 (self-evolution) | Future: agent proposes new skills via the proposal workflow. Depends on this plan. |
| #031 (recall search) | Recall searches BCF skill chunks; this plan handles SKILL.md files. Different systems. |
| #032 (structured prompts) | Established the instruction loading pattern this plan follows. Direct foundation. |
