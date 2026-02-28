# Orchestration (Swarms & Templates)

**Maturity: SEED** | ID Prefix: ORCH

## Purpose

Higher-order agent workflows: swarm templates that spawn multiple agents with predefined configurations, agent templates for reusable spawn configs, and prompt templates for common tasks.

## Conceptual Model

```
Template: { name, agentType, defaultPrompt, flags }
Swarm: { name, description, agents: [{ template, prompt, vars }] }
Prompt: { name, content (markdown) }
```
