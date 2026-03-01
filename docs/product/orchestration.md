# Orchestration (Swarms & Templates)

**Maturity: SEED** | ID Prefix: ORCH | Dependencies: none

## Purpose

Higher-order agent workflows: swarm definitions that spawn multiple agents with predefined configurations, agent templates for reusable spawn configs, and prompt templates for common tasks.

## Conceptual Model

```
Template: reusable agent spawn configuration (type, default prompt, flags)
Swarm: named multi-agent workflow (which templates, what prompts, what variables)
Prompt: reusable prompt content with variable substitution
```

## Open Questions

? [ORCH-001] How should swarm results be aggregated — should the conductor merge outputs automatically, or present them to the user for manual review?

? [ORCH-002] Should prompt templates support conditional sections (e.g., include architecture context only if the task involves a specific domain)?
