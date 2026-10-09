# Agent watch screens and project channel delivery

Owner instruction: implement approved v2 quiet hierarchy and v3 shared channel. Selected design snapshots are in designs/agent-watch/v3, saved 2026-10-09T17:19:34Z; see manifest for hashes and Canvas URLs. Current integration branch pu/staged-changes at ce66e62. Existing unrelated branches/worktrees are preserved.

Build manifest:
| Leaf | Outcome | Owner | Build inputs | Accepts after | Writer |
|---|---|---|---|---|---|
| Channel engine/CLI | durable shared messages, cursors/search/replies/edit/reaction and explicit CLI | native builder | project-channel.md wire contract | protocol+real persistence/dispatch/CLI checks | Rust only |
| Git review data | cumulative branch diff, unique commits, staged/unstaged/untracked and trustworthy refresh/PR errors | native builder | current GitService/DiffState | temp Git repo checks and safe Swift typecheck | GitService, DiffState, watcher, Git models/tests |
| Native screens/channel | approved native screens, shared channel states/composer/sidebar routing | conductor | actual channel and Git producers integrated at pinned commits | full source typecheck, independent review, owner Xcode checks | remaining Swift UI/state/protocol |
| Composed review | exact snapshot review and fixes | independent reviewer | composed pinned snapshot | no open findings + applicable checks | read-only |

No production activation, deployment, global install or main merge. Git builders use separate worktrees; consumers may implement against the specified contract but producer availability requires pinned actual code. Parent integrates producer commits then proves composition. No scope cuts or follow-up-only placeholders. Storage feature is scoped and researched in project-channel.md; broader storage/database questions remain open.

Check obligations: Rust tests/fmt/clippy; protocol serialization; persistence/cursor/ownership/concurrency; no channel-to-PTY/trigger coupling; real Git edge cases; safe Swift parsing/typechecking and state tests; independent review; Xcode app build/test and native light/dark keyboard/scroll inspection (owner, cannot run in this agent session).

Status: planning/independent review, execution authorized by owner. Main acceptance pending; Xcode evidence not yet run.
