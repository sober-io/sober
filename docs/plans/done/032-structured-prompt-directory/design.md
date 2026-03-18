# #032: Split SOUL.md into Structured Prompt Directory — Design

## Problem

The monolithic `backend/soul/SOUL.md` (143 lines) mixes three distinct concerns:
identity/personality, behavioral system instructions, and internal reasoning logic.
The `<!-- INTERNAL:START/END -->` HTML comment markers are a crude binary access
control mechanism with no granularity.

## Solution

Split into a flat `backend/crates/sober-mind/instructions/` directory with individual
markdown files. Each file has YAML frontmatter for metadata (category + visibility),
and is compiled into the binary via `include_str!()`.

## Design Decisions

- **Location:** `backend/soul/SOUL.md` → `backend/crates/sober-mind/instructions/*.md`
  (compile-time embedded, no runtime file I/O for base instructions)
- **Old directory:** `backend/soul/` deleted entirely
- **Frontmatter:** every `.md` file has YAML frontmatter with `category`, `visibility`,
  `priority`, and optional `extends`
- **4 categories (assembly order):** `personality` → `guardrail` → `behavior` → `operation`
- **2 visibility levels:** `public` (all triggers) | `internal` (Scheduler + Admin only)
- **INTERNAL:START/END markers** removed — replaced by frontmatter visibility
- **MEMORY_EXTRACTION_INSTRUCTIONS const** removed — content moved to `extraction.md`
- **`@path` references** resolved single-level via `ReferenceResolver`
- **User path:** `.sõber` → `.sober` (filesystem portability)
- **Frontmatter parsing:** `serde_yml` crate (maintained fork of deprecated `serde_yaml`)

## File Structure

```
backend/crates/sober-mind/instructions/
├── soul.md              # personality, public,  pri:10
├── safety.md            # guardrail,  public,  pri:10
├── memory.md            # behavior,   public,  pri:10
├── reasoning.md         # behavior,   internal,pri:20
├── evolution.md         # behavior,   internal,pri:30
├── tools.md             # operation,  public,  pri:10
├── workspace.md         # operation,  public,  pri:20
├── artifacts.md         # operation,  public,  pri:30
├── extraction.md        # operation,  public,  pri:40
└── internal-tools.md    # operation,  internal,pri:50
```

## Caching Model

| Layer | When loaded | Cached where | Invalidation |
|-------|------------|--------------|--------------|
| Base | Compile time (`include_str!`) | Static in binary | Recompile |
| User (`~/.sober/`) | Process startup | `InstructionLoader.cached` | Process restart |
| Workspace (`.sober/`) | First access per workspace | `Mind.workspace_cache` | Process restart |

## Visibility Rules

| TriggerKind | public | internal |
|-------------|--------|----------|
| Human       | yes    | no       |
| Replica     | yes    | no       |
| Admin       | yes    | yes      |
| Scheduler   | yes    | yes      |

## Extension Rules

- User (`~/.sober/`) can extend any base instruction file
- Workspace (`.sober/`) can extend any file EXCEPT `safety.md` (protected)
- Files with `extends: <filename>` → content appended to named base file
- Files without `extends` → standalone new instruction sections
- soul.md layering: user full-replace, workspace additive (existing behavior preserved)
