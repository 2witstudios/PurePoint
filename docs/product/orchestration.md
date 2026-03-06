# Orchestration (Swarms & Templates)

**Maturity: EXPLORING** | ID Prefix: ORCH | Dependencies: none

## Purpose

Higher-order agent workflows: swarm definitions that spawn multiple agents with predefined configurations, agent templates for reusable spawn configs, and prompt templates for common tasks.

## Conceptual Model

```
Template: reusable agent spawn configuration (type, default prompt, flags)
Swarm: named multi-agent workflow (which templates, what prompts, what variables)
Prompt: reusable prompt content with variable substitution
```

## Research Notes

**Storage:** YAML files in `.pu/templates/`, `.pu/agents/`, `.pu/swarms/` (local scope) and `~/.pu/` equivalents (global scope).

**Prompt templates:** name, content (body), description, agent type, scope. Variable substitution via `--var KEY=VALUE`. CLI: `pu prompt list|show|create|delete`.

**Agent definitions:** name, agent_type, default prompt (template or inline), tags, scope. CLI: `pu agent list|show|create|delete`.

**Swarm definitions:** name, agent roster (`AGENT:ROLE:QTY`), worktree_count, worktree_template, include_terminal, scope. CLI: `pu swarm list|show|create|delete|run`.

**Engine:** Protocol handlers for ListTemplates, GetTemplate, SaveTemplate, DeleteTemplate (and equivalents for agent_def, swarm_def). Swarm run spawns agents according to roster.

**Desktop:** AgentsHubView with creation sheets for prompts, agent defs, and swarms. CommandPalettePanel offers quick spawning from built-in variants, agent defs, and swarms.

## Open Questions

? [ORCH-001] How should swarm results be aggregated — should the conductor merge outputs automatically, or present them to the user for manual review?

? [ORCH-002] Should prompt templates support conditional sections (e.g., include architecture context only if the task involves a specific domain)?
