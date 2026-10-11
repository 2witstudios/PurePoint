# pointguard

Agent tooling. Starts with universal skills; other tools land here as they come.

The skills are workflow commands that work across any project, kept separate
from [paralleldrive/aidd](https://github.com/paralleldrive/aidd) so they can
diverge freely. PageSpace-aware where it helps, never required.

## Skills

| Skill | What it does |
|---|---|
| `review` | Full code review, including an explicit OWASP Top 10 pass → publishes a review record to the repo's PageSpace drive, links it from tasks, posts the verdict on the PR. `/review` |
| `pr` | Open/update a PR whose description links the PageSpace task, plan and prompt pages. `/pr` |
| `handoff` | Hand a finished branch to review in one step: push, PR, handoff page, task moves, parent notify. `/handoff` |
| `task` | Carry an authorized objective through planning, tasking, provisional integration and independent reviews. Separate branch build inputs from main acceptance obligations; keep proof and delivery records current. `/task` |
| `triage` | Triage PR review comments, resolve already-addressed threads, delegate fix prompts to sub-agents. `/triage` |
| `rtc` | Reflective Thought Composition: restate → ideate → self-critique → expand orthogonally → score → respond. `--compact` for internal passes, `--depth N` for user-facing. `/rtc` |
| `geo-interview` | Interview AI models about GEO visibility for a product; share of voice + citation strategy. `/geo-interview` |
| `pagespace-cli` | Thin guide to the `pagespace` CLI: credentials (login vs keys), verbs, JSON/exit codes, plus an in-app-tool → CLI/SDK mapping. `/pagespace-cli` |
| `pagespace-sdk` | Thin guide to `@pagespace/sdk`: auth providers, namespaces, uploads, typed errors, custom operations. `/pagespace-sdk` |
| `pagespace-canvas-websites` · `pagespace-spreadsheets` · `pagespace-task-management` · `pagespace-writing-documents` | PageSpace's built-in page-type skills, copied verbatim from the app (`apps/web/src/lib/ai/skills/bodies`) so any agent can author CANVAS, SHEET, TASK_LIST and DOCUMENT pages. Re-copy when the app's bodies change. |

Dependencies on AIDD skills (`/aidd-fix`, `/aidd-churn`, `/aidd-javascript`,
`/aidd-tdd`, ...) are soft: each skill degrades gracefully when they are not
installed.

## Install

```sh
./install.sh          # link skills that aren't installed yet
./install.sh --force  # replace existing installs and legacy aidd-* names
```

Links each skill into `~/.claude/skills`, `~/.config/opencode/skills`, and
`~/.agents/skills`. This repo is the single source of truth — edit here, every
agent sees the change.

## Attribution

`review`, `pr` and `geo-interview` are derived from
[paralleldrive/aidd](https://github.com/paralleldrive/aidd) (MIT © 2025
Eric Elliott) and diverged from there. MIT licensed — see
[LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).

## History

`review`, `pr` and `handoff` started as forks of AIDD's `aidd-review` /
`aidd-pr` / handoff flow and have since diverged (PageSpace drive records,
review-record checks, gates). `triage` and `geo-interview` were renamed from
`aidd-triage` / `aidd-geo-interview` when they moved here.
