# Swarm Management — Swarm Management & Multi-Agent Orchestration

## User Story

As a user, I want to define and launch swarms of agents that work together on a coordinated task so that I can parallelize complex work like code reviews, migrations, or feature implementation across multiple agents.

## Feature Description

A swarm editor and launcher that lets users define multi-agent task groups. Includes swarm definition (which agents, what prompts, what strategy), variable extraction from prompt templates, execution monitoring, and result aggregation. Supports both project-scoped and global swarm definitions.

## How ppg-cli Did It

Two-column swarm editor with a list of saved swarms on the left and an editor on the right. YAML-based swarm file management with strategy selection (shared workspace vs isolated worktrees), automatic variable extraction from `{{VAR}}` patterns in prompts, project and global scope support. Swarms launched agents sequentially via tmux.

What worked well: The two-column editor made swarm management intuitive. Variable extraction from prompts automated the parameterization step. Strategy selection (shared vs isolated) covered the two main orchestration patterns.

## PurePoint Opportunity

- **Daemon-native swarm execution**: The Rust daemon can coordinate swarm agents directly — concurrent spawning, health monitoring, and result collection without tmux overhead.
- **Real-time swarm progress**: IPC event streams provide live updates on each agent's progress within a swarm, enabling a swarm dashboard view.
- **Advanced strategies**: Beyond shared/isolated, the daemon can support dependency graphs between agents (agent B starts after agent A completes), resource pooling, and automatic retry on failure.
- **CLI parity**: `pu swarm run <name>` launches the same swarm definition from terminal, sharing config with the GUI.

## Priority

**P1** — Orchestration is PurePoint's core differentiator. This is the highest-value feature gap.
