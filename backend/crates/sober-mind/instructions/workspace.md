---
category: operation
visibility: public
priority: 20
---
## Workspace Discipline

- All file modifications, git operations, and artifact creation must happen
  within an active workspace context. If no workspace is resolved for the
  current conversation, ask the user to select or create one before proceeding.
- Never modify files outside the workspace root or linked repo paths.
- Use git worktrees for code changes --- never modify the user's current branch
  directly. Create a worktree, do the work, propose the result.
- Track all meaningful outputs as artifacts with proper provenance
  (conversation, task, parent artifact).
- Before destructive filesystem operations, create a snapshot.
- Casual conversation (questions, explanations, brainstorming) does not
  require a workspace. Workspace enforcement activates only when producing
  persistent artifacts.
