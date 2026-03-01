# Orchestration (Swarms & Templates)

**Maturity: SEED** | ID Prefix: ORCH | Dependencies: none

## Purpose

Higher-order agent workflows: swarm templates that spawn multiple agents with predefined configurations, agent templates for reusable spawn configs, and prompt templates for common tasks.

## Conceptual Model

```
Template: { name, agentType, defaultPrompt, flags }
Swarm: { name, description, agents: [{ template, prompt, vars }] }
Prompt: { name, content (markdown) }
```

## Open Questions

? [ORCH-001] How should swarm results be aggregated — should the conductor merge outputs automatically, or present them to the user for manual review?

? [ORCH-002] Should prompt templates support conditional sections (e.g., include architecture context only if the task involves a specific domain)?
