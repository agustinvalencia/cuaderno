# The daily loop

The core rhythm of Cuaderno is a daily loop: **orient → act → log → close**. None of it is
mandatory or guilt-inducing; it's a habit that keeps the vault current with almost no overhead.

## Morning: orient

Start the day by asking the tool what deserves attention:

```bash
cdno orient --energy deep
```

[`orient`](../reference/cli/orient.md) shows commitments due soon, your active projects with their
current state and top next action, and a single **suggested starting point** — biased toward the
[energy](../concepts/contexts-and-energy.md) you tell it you have. Drop `--energy` to get a neutral
suggestion; pass `--energy light` on a low day.

Want just the project snapshot without commitments? Use [`cdno status`](../reference/cli/status.md).

## Through the day: act and log

As you work, two verbs carry most of the weight.

**Log what happens** — append a timestamped line to today's journal:

```bash
cdno log "scaled the mesh to 2M cells; 4x runtime, still stable"
```

**Capture stray thoughts** without breaking flow — they land in the inbox for later
[triage](inbox-and-triage.md):

```bash
cdno capture "does Chen 2025 use the same preconditioner?"
```

Make progress on projects as you go:

```bash
# Add the next action you just thought of:
cdno action add --project surrogate-model --title "Profile the assembly step" --energy medium

# Record where a project now stands (auto-logs the previous state to today's journal):
cdno project state --slug surrogate-model --text "Mesh scaling works; assembly is the bottleneck"

# File a useful result into the right portfolio:
cdno file --portfolio sparse-vs-dense-ood --source "ablation run B" --origin projects/surrogate-model

# Tick off a finished action (substring match on the bullet):
cdno action complete --project surrogate-model --query "feature set B"
```

## Evening: close

A light wind-down — note anything for your stewardships and reflect:

```bash
# Log a tracked activity under a stewardship:
cdno track gym --stewardship health --content "Upper body; good energy"

# A closing reflection is just another log line:
cdno log "good focus day; pick up assembly profiling tomorrow"
```

That's it. The journal now holds a faithful record of the day, your projects reflect reality, and
tomorrow's `orient` will pick up where you left off.

## Letting Claude run the loop

Each step has an MCP equivalent (`get_orientation`, `append_to_log`, `add_action`,
`update_project_state`, …), so an assistant can drive the same loop conversationally. See
[Connect to Claude](../getting-started/connect-to-claude.md) and the example
[skills](../reference/mcp/with-claude-skills.md).

Next: [Managing projects](projects.md).
