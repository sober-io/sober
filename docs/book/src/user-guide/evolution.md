# Self-Evolution

Sober continuously improves itself by observing conversation patterns and proposing new capabilities. It can generate WASM tools, create prompt-based skills, refine its own instructions, and set up scheduled automations -- all without manual intervention when configured to do so.

Every evolution is auditable, configurable, and revertible. You control what the agent is allowed to do autonomously and what requires your approval.

---

## How It Works

The agent runs a **self-evolution check** on a configurable interval (default: every 2 hours). Each cycle follows four phases:

1. **Execute pending approvals** -- any evolutions you approved since the last cycle are executed first.
2. **Gather data** -- the agent queries recent conversations and reviews what evolutions are already active. This phase uses no LLM tokens.
3. **Detect patterns** -- the agent analyzes the gathered data and proposes new evolutions by calling `propose_*` tools (internal-only, visible to Scheduler and Admin triggers only). Depending on your autonomy configuration, proposals are either auto-approved or queued for your review.
4. **Execute auto-approved** -- any proposals that were auto-approved in phase 3 are executed immediately.

The only evolutions that wait between cycles are proposals that require your approval.

---

## Evolution Types

### Plugins (WASM tools)

When the agent notices users repeatedly performing a multi-step task that could be automated with a dedicated tool, it proposes a WASM plugin. The plugin is generated via the code generation pipeline, audited for security, and registered in the plugin system.

**Example:** Users frequently fetch URLs and extract structured data. The agent proposes a `url-data-extractor` tool that does this in a single step.

### Skills (prompt-based)

Skills are prompt templates that encode a specific expertise. They are lightweight -- no compilation required -- and take effect immediately.

**Example:** Users often ask for simplified explanations. The agent proposes an `eli5-explainer` skill with a prompt template tuned for clear, simple explanations.

### Instructions

The agent can refine its own behavior by proposing changes to instruction files. These are written as overlay files that take precedence over the compiled-in base instructions, without modifying the binary.

**Example:** The agent's debugging approach is consistently suboptimal. It proposes an improved `reasoning.md` instruction with a more structured multi-step debugging methodology.

### Automations

When the agent detects recurring time-based patterns, it proposes scheduled jobs. These target specific users and deliver results to a designated conversation.

**Example:** A user requests a project status summary every Monday. The agent proposes a weekly scheduled job that generates and delivers the summary automatically.

---

## Autonomy Configuration

Each evolution type has an independent autonomy level that controls whether proposals require your approval:

| Level | Behavior |
|-------|----------|
| **Auto** | Proposals are approved and executed automatically. You are notified after the fact. |
| **Approval Required** | Proposals are queued for your review. Nothing happens until you approve. |
| **Disabled** | The agent will not propose this type of evolution at all. |

**Defaults:**

| Type | Default Autonomy |
|------|-----------------|
| Plugins | Approval Required |
| Skills | Auto |
| Instructions | Approval Required |
| Automations | Auto |

Plugins and instructions default to requiring approval because they have broader impact -- plugins execute arbitrary code and instructions change agent behavior for all users.

### Changing Autonomy Levels

**Via Settings UI:** Navigate to **Settings > Evolution**. The Autonomy Configuration section at the top has a dropdown for each type. Select the level you want and click **Save**.

**Via CLI:**

```bash
sober evolution config
```

This displays the current configuration. To change levels, use the Settings UI or the evolution config API endpoint.

---

## Managing Evolutions

### Settings UI

The **Settings > Evolution** page provides three sections:

**Pending Proposals** -- Evolutions awaiting your approval. Each card shows the type, title, description, confidence score, and the evidence that triggered the proposal. You can **Approve** or **Reject** each one. Approving triggers immediate execution.

**Active Evolutions** -- Currently live evolutions with usage metrics (how many times invoked, when last used). You can filter by type and **Revert** any active evolution to undo it.

**Timeline** -- A chronological feed of all evolution activity. Click **View all** to see the full timeline with status transition history for each evolution.

### CLI

```bash
# List evolutions (default: proposed + active)
sober evolution list

# Filter by type or status
sober evolution list --type plugin
sober evolution list --status proposed

# Approve a pending proposal (triggers immediate execution)
sober evolution approve <id>

# Reject a proposal
sober evolution reject <id>

# Revert an active evolution
sober evolution revert <id>

# View current autonomy configuration
sober evolution config
```

**Plugin management:**

```bash
# List all registered plugins (MCP, Skill, WASM)
sober plugin list
sober plugin list --kind wasm --status enabled

# Enable, disable, or remove a plugin
sober plugin enable <id>
sober plugin disable <id>
sober plugin remove <id>
```

**Skill management:**

```bash
# List skills from the catalog
sober skill list

# Trigger a catalog reload
sober skill reload
```

---

## Safety Guardrails

### What Cannot Be Modified

Instruction evolutions are blocked from modifying safety-critical files:

- Files with `category: guardrail` in their YAML frontmatter (e.g., `safety.md`)
- Files on a hardcoded blocklist maintained in the codebase

Both checks are enforced at the tool level -- the agent cannot even propose changes to these files. The detection prompt also instructs the agent not to attempt guardrail modifications.

### Rate Limits

To prevent runaway evolution, three rate limits are enforced:

| Limit | Value |
|-------|-------|
| Max proposals per cycle | 5 |
| Max auto-approvals per day | 3 |
| Max concurrent executing evolutions | 2 |

When the auto-approval daily limit is reached, additional proposals that would normally be auto-approved are queued as `proposed` instead, requiring manual approval.

### Deduplication

The system prevents duplicate evolutions through three layers:

1. **Database constraint** -- a unique index prevents two active evolutions of the same type and title from coexisting.
2. **Tool-level validation** -- each `propose_*` tool checks for existing similar capabilities before creating a proposal.
3. **Detection-aware prompting** -- the detection prompt includes all active evolutions so the agent avoids proposing redundant capabilities.

### Audit Trail

Every evolution lifecycle event is logged: proposals, approvals, rejections, executions, failures, and reverts. Each evolution also maintains a `status_history` field that records the full chain of status transitions with timestamps, visible in the timeline view.

### Revert

Any active evolution can be reverted. The revert operation is type-specific:

| Type | What Happens on Revert |
|------|----------------------|
| Plugin | Plugin is deleted from the registry; tool becomes unavailable on the next turn. |
| Skill | Plugin and skill file are deleted; skill catalog is reloaded. |
| Instruction | Overlay file is removed (or previous content is restored); base instruction takes effect again. |
| Automation | Scheduled job is cancelled. |

Reverts are immediate when triggered via the Settings UI or CLI.
