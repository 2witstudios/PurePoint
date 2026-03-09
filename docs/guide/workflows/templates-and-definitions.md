# Templates and Definitions

Reusable prompts, agent definitions, and swarm compositions.

## Templates

Templates are reusable prompts with variable substitution. Store them as markdown files with optional YAML frontmatter.

### File format

```markdown
---
name: code-review
description: Review code for quality
agent: claude
command: optional-shell-command
---
Review the code on branch {{BRANCH}} for {{SCOPE}}.
Focus on security and performance.
```

**Frontmatter fields** (all optional):

| Field | Default | Description |
|---|---|---|
| `name` | filename stem | Template name |
| `description` | `""` | Short description |
| `agent` | `""` | Agent type to use |
| `command` | (none) | Shell command (for terminal agents, supports `{{VAR}}`) |

### Variables

Variables use `{{VAR}}` syntax. Pass values at spawn time:

```sh
pu spawn --template code-review --var BRANCH=main --var SCOPE=security
```

Unsubstituted variables produce a warning but don't fail.

### Storage

| Scope | Location |
|---|---|
| Local | `.pu/templates/{name}.md` |
| Global | `~/.pu/templates/{name}.md` |

Local templates shadow global templates with the same name.

### CLI commands

```sh
pu prompt list                                          # list all templates
pu prompt show code-review                              # view a template
pu prompt create code-review --body "Review {{BRANCH}}" --description "Code review" --agent claude
pu prompt delete code-review --scope local
```

### Creating templates manually

Write a `.md` file directly to `.pu/templates/` or `~/.pu/templates/`:

```sh
cat > .pu/templates/security-audit.md << 'EOF'
---
name: security-audit
description: Security audit for a branch
agent: claude
---
Perform a security audit on branch {{BRANCH}}.
Check for OWASP Top 10 vulnerabilities.
EOF
```

## Agent defs

Agent defs are saved agent configurations. They pair an agent type with a template or inline prompt.

### File format

YAML files in `.pu/agents/` (local) or `~/.pu/agents/` (global):

```yaml
name: reviewer
agent_type: claude
template: code-review           # references a template by name
tags:
  - review
  - code
available_in_command_dialog: true
icon: magnifying-glass
```

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Definition name |
| `agent_type` | string | `"claude"` | Agent type |
| `template` | string | (none) | Template name to use |
| `inline_prompt` | string | (none) | Raw prompt text (alternative to template) |
| `command` | string | (none) | Shell command (for terminal agents) |
| `tags` | string[] | `[]` | Tags for categorization |
| `available_in_command_dialog` | bool | `true` | Show in macOS command palette |
| `icon` | string | (none) | Icon name for UI |

Use `template` to reference a saved template, or `inline_prompt` for a self-contained definition.

### CLI commands

```sh
pu agent list
pu agent show reviewer
pu agent create reviewer --agent-type claude --template code-review --tags "review,code"
pu agent create dev-server --agent-type terminal --command "npm run dev"
pu agent delete reviewer --scope local
```

### Spawning from an agent def

Agent defs integrate with templates and the command palette in the macOS app. To spawn from the CLI, reference the agent def's template:

```sh
pu spawn --template code-review --var BRANCH=main
```

## Swarm defs

Swarms are multi-agent compositions. They define a roster of agent defs to spawn across one or more worktrees.

### File format

YAML files in `.pu/swarms/` (local) or `~/.pu/swarms/` (global):

```yaml
name: pr-review
worktree_count: 1
worktree_template: ""
roster:
  - agent_def: reviewer
    role: code-review
    quantity: 3
include_terminal: true
```

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Swarm name |
| `worktree_count` | number | `1` | Number of worktrees to create |
| `worktree_template` | string | `""` | Worktree name template (`{index}` is substituted) |
| `roster` | entry[] | `[]` | List of agent defs to spawn |
| `include_terminal` | bool | `false` | Add a terminal agent per worktree |

### Roster entries

| Field | Type | Default | Description |
|---|---|---|---|
| `agent_def` | string | (required) | Agent def name |
| `role` | string | (required) | Role identifier |
| `quantity` | number | `1` | Number of instances per worktree |

### Worktree naming

- Empty template: `{swarm_name}-{index}` (e.g., `pr-review-0`)
- With template: `{worktree_template}` with `{index}` substituted (e.g., `feature-{index}` becomes `feature-0`)

### Agent naming

Agents are named `{swarm}-{agent_def}-{wt_index}-{qty_index}`. Example: `pr-review-reviewer-0-0`, `pr-review-reviewer-0-1`, `pr-review-reviewer-0-2`.

### CLI commands

```sh
pu swarm list
pu swarm show pr-review
pu swarm create pr-review --roster reviewer:code-review:3 --worktrees 1 --include-terminal
pu swarm delete pr-review --scope local
pu swarm run pr-review --var BRANCH=main
```

### Example: multi-worktree swarm

```yaml
name: full-stack
worktree_count: 3
worktree_template: "feature-{index}"
roster:
  - agent_def: builder
    role: implementation
    quantity: 1
  - agent_def: reviewer
    role: code-review
    quantity: 1
include_terminal: false
```

Run: `pu swarm run full-stack`

Creates 3 worktrees (`feature-0`, `feature-1`, `feature-2`), each with 1 builder and 1 reviewer agent.
