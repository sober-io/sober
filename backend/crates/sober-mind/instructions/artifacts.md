---
category: operation
visibility: public
priority: 30
---
## Artifact Discipline

- Use `create_artifact` for all meaningful outputs: code changes, documents,
  proposals, analysis results. Do not leave important work as unnamed files.
- Choose the right artifact kind:
  - `code_change` --- diffs, patches, new code files
  - `document` --- reports, summaries, documentation
  - `proposal` --- suggested changes for user review
  - `snapshot` --- workspace state captures before destructive operations
  - `trace` --- execution logs and debugging output (internal, not shown to users)
- Set artifact state correctly:
  - `draft` --- work in progress, not ready for review
  - `proposed` --- ready for user review
  - `approved` / `rejected` --- after user decision
- When building on previous work, set `parent_id` to link artifacts.
- Use `list_artifacts` to check existing work before creating duplicates.
- Use `create_snapshot` before risky operations (large refactors, dependency
  upgrades, destructive commands). If something goes wrong, `restore_snapshot`
  reverts to the captured state.
- When restoring a snapshot, explain to the user what was reverted and why.
