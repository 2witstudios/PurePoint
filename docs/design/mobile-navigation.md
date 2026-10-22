# Mobile navigation — Pinned agents + Projects

Status: selected design; visual direction approved by Jono, 2026-10-16. Implementation specification with backend work identified. Supersedes the earlier machine-tree sidebar design on this page.

[Chosen interactive canvas](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/f9g9k0jist5w75imhl302dqg) · [Earlier tree study, retained for reference](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/mgd2c1469rs66agu74bjxsqe)

Code audit baseline: c589432c65b246aab5eeb2229097c966225847a3. The canvas is a prototype: its sample spawn/rename/delete interactions are not connected to the app or daemon.

## Decision and purpose

Make individual agent conversations quick to find without losing PurePoint's project/workspace structure. Swarms can create many agents; stable pinned shortcuts protect the agents the user directs most. Projects remains a full destination for understanding and starting work, rather than a filter on the recent list.

The left navigator contains, in order:
1. Point Guard.
2. Projects.
3. Pinned agents.
4. Recent agents.
5. Connections… as a scrolling utility action.

Point Guard and Projects are matching navigation rows: same typography, alignment and at least 44 pt tap targets. Point Guard uses the existing PurePointLogo light/dark assets, with no green dot or navigation arrow. Projects uses the same outlined folder icon family as the main project list. The close control is separate. No global branding block, drawer search, Agents/Projects tabs, history row or fixed machine footer.

## Destinations and journeys

### Point Guard

Point Guard opens the selected paired Mac's existing Pi chat in the main content area. Keep new-conversation and busy-chat controls. A labeled/accessibility-named history control in this screen's toolbar opens history from the right. History does not occupy the left agent navigator.

The right history sidebar includes its host name, search, current conversation marker, dates, recent previews, New conversation and Close. On narrow phones it overlays content with a scrim and leaves a visible content edge. Busy-history browsing stays read-only until explicit Stop-and-resume confirmation. History, search and previews use native sessions; the prototype's sample history is illustrative.

### Projects → project workspace → agent

Projects opens a main-screen project list, grouped by execution machine with the bridge host identified. Each project shows its name, machine, actual agent counts and availability. Include registered projects with no agents; an empty workspace still needs its New action.

Selecting a project opens its workspace. Show project-root agents, then worktrees with name, branch, state and their agents. Preserve the desktop organizational model and make work visible within the selected project. This is a mobile presentation of the workspace domain, not a replica of desktop pane/tab geometry.

The workspace has New with New agent, New worktree and New terminal. Each worktree also has a scoped New agent action. A setup sheet gathers the necessary instructions/options before an explicit launch. The final confirmation names the machine and project/worktree. Never infer a target from a display name or last-used path.

Selecting an agent opens its existing terminal/native-composer screen. A visible **Open in project** action is present on every project-associated agent, whether it was reached through Projects, pinned agents or recents. It opens the owning workspace, expands the exact worktree/root group and highlights/focuses the agent. It is not a Back action and has no arrow. Projects provides its own normal Back to project-list navigation.

Preserve project-list/workspace scroll positions, expansion and context across destination changes. A missing project association gets an explanation and disabled Open in project; it must not navigate to a guessed project. Missing metadata gets a recoverable loading/error state, not a fabricated root.

### Pinned and recent agents

These are flat shortcuts across the selected bridge host's daemon fleet. Point Guard never appears in these groups. Each row shows agent name, type, machine/project context, state/phase and idle/last interaction information. Full paths/worktree identity are available in detail/accessibility for duplicate names.

Pinned agents retain user-chosen order and remain above recents regardless of swarm activity. New pins append to the pinned section; do not silently evict pins or reorder them on output. A pinned agent is excluded from recents.

Recency means the user's last open/send interaction, not terminal output, inventory refresh, idle tick or agent-generated activity. For agents the user has never interacted with, use startedAt as the fallback. Break ties by stable scoped identity. Background output never promotes an old worker above the user's active conversations.

Long press opens Pin/Unpin, Rename… and Delete…. No persistent pin action button occupies each row. Provide native accessibility actions and keyboard/context-menu equivalents. The current section label makes pinned state clear.

Pins, recent timestamps and local hiding are phone preferences, scoped to stable bridge identity + daemon machine ID + agent ID. Persist across relaunch; changing names/endpoints does not change identity. Cross-device pin sync is not part of this design.

An offline pinned agent remains visible with its last-known context and unavailable status while permission still allows it. Revocation/off clears restricted inventory, even for pinned agents. Never enable input from cached state.

### Rename and Delete semantics

Rename is the daemon agent name, reflected in inventory across clients. It requires management authorization, keeps identity/drafts intact, shows errors and does not optimistically claim a failed rename succeeded.

Delete in this navigator means deleting the phone's shortcut/conversation entry from recents, not killing a process, deleting its native transcript or removing a worktree. The confirmation says exactly that; final native action should be labeled **Remove from recents** for clarity. Unpin it if needed. The agent remains discoverable in Projects, and deliberate reopening restores its recent entry. Private drafts/recovery are retained. Do not hide it permanently across unrelated new agent identities.

Stopping an agent is a distinct project-workspace management action with an explicit confirmation and observed target. Worktree deletion, bulk kill and project removal are not implied by the navigator Delete item and are outside this change.

## Preserve existing mobile features

| Existing feature | Required home/behavior |
| --- | --- |
| Pi streaming transcript, Markdown/code, tools, notices, Latest | Point Guard main screen, unchanged model semantics |
| New chat; busy Stop-and-start-new | Point Guard toolbar and right history, same gating/confirmation |
| Session list/search/refresh/browse/resume | Right history; read-only browsing during work, explicit Stop-and-resume |
| Photos/files, preparation, limits, remove attachment | Point Guard composer; keep picker/selection fencing |
| Send/Steer/After reply, queues/receipts, Stop | Point Guard composer, unchanged; Stop never kills delegated workers |
| Offered editor draft and confirm/select/input/editor dialogs | Point Guard; preserve correlation, cancellation and presentation arbitration |
| Pi drafts, recovery/canceled text and uncertain submissions | Same durable scopes, explicit restore/dismiss, never automatic resend |
| Pair/select/rename saved Macs, QR/paste, cancellation/errors | Connections; retain scanner permission and unsupported-camera fallback |
| Disconnect/reconnect and detailed status | Connections and main context/status action, reachable in empty/offline states |
| Fleet inventory, complete/partial/error/empty states | Projects and agent list; explicit Refresh and visible retry |
| Agent composer outcomes and saved prompts | Existing agent screen; confirmed/sent/blocked/failed and reasons |
| Recovery of submitted A while newer draft B exists | Both survive navigation, host changes and relaunch; append/copy/dismiss, no overwrite/resend |
| All ten fixed quick keys | Esc, Ctrl-C, Tab, Enter, four arrows, y, n |
| Raw typing | Explicit toggle, off by default; existing permissions and limits |
| Read-only SwiftTerm | Actual PTY size, zoom/sideways scrolling, reset, legacy warning; with Control the phone claims its own size while viewing (`terminal.size`), never Resize |
| Foreground/network/re-pair lifecycle | Existing pinned transport, bounded queues and generation fencing |

Remove the separate Agents sheet toolbar entry only once this navigator fully replaces it. No feature is dropped because the visual prototype omits its control.

## Machine scope and availability

A paired Mac is an authenticated bridge with one Pi Point Guard session. Forwarded execution machines are daemons reached through that Mac's existing SSH forwards; they do not acquire Point Guard chat.

The Point Guard row always means the selected paired Mac. Connections changes paired host deliberately. Show that host in the main title/context and Projects screen. Fleet rows include execution-machine context. Do not connect to every saved host in the background or merge their fleets by hostname.

The phone never opens SSH. Remote availability depends on the Mac, bridge and forwards. A failed bridge marks its fleet unavailable; an individual daemon failure is machine-local. Chat may still work with terminal access off or a local daemon unavailable.

Unknown/incomplete inventory, no agents, unavailable project, missing daemon and offline bridge are different states. Keep cached data explicitly stale; do not render a current online/working badge from it. A disappeared selected agent remains an unavailable screen with recovery/navigation, never silently becomes a different agent.

## Backend work required

The daemon already has relevant operations. The phone bridge currently exposes only fixed permissions/agents/terminal/prompt operations. This design needs a management adapter and additional metadata, not arbitrary daemon IPC passthrough.

### Inventory and project navigation

Extend the bridge's read API with registered project/worktree metadata from daemon Inventory kinds Projects and Worktrees, alongside the existing agents projection. Preserve empty containers, project availability, full project identity, worktree id/name/branch/path/status and completeness/error information. No filesystem access from the phone.

Return partial results explicitly when one daemon or metadata query fails. Validate membership server-side; a client cannot turn an arbitrary submitted path into a registered project. New Swift decoding tolerates absent metadata on older bridges, retaining usable agents and honest fallback labels. Chat readiness and permissions discovery must remain independent of held/slow inventory.

Open in project uses this metadata and local navigation only. No separate mutation is needed.

### Proposed fixed management API

These are proposed wire additions, not existing endpoints. Final names and payloads belong in apps/purepoint-mobile/docs/contract.md before implementation.

| Semantic operation | Daemon mechanism already present | Adapter responsibility |
| --- | --- | --- |
| List projects/worktrees | Inventory | Bounded projection, permission checks, completeness and machine scope |
| Spawn agent | Spawn | Resolve registered project/root/worktree; validate allowed type/preset and bounded prompt/options |
| Create worktree | CreateWorktree | Validate name/base and target project; return authoritative identity |
| New terminal | SpawnShell | Resolve approved root/worktree cwd server-side; ensure spawned shell is associated and discoverable |
| Rename agent | Rename | Resolve exact agent/project; validate bounded name; refresh authoritative inventory |
| Stop agent | Kill with Agent target only | Explicit confirmation intent; no All/Worktree targets through this endpoint |

Spawn exposes supported safe fields, not arbitrary command strings, extra arguments, environment, executable paths or raw daemon request JSON. The New agent sheet needs an authenticated allowlisted agent/preset catalog so it cannot advertise unavailable choices. Choose project root, existing worktree or deliberate new worktree explicitly.

Shell discoverability is an acceptance requirement: SpawnShell takes cwd, so verify/adapt project/worktree association rather than assuming a successful PID automatically appears correctly in inventory.

### Permissions and mutation lifetime

Keep terminals off/view/control behavior as implemented. Do not silently expand an existing control grant into spawn/kill/worktree management authority.

Add a separate per-device workspace-management permission, default off, with a matching explicit Mac Connect Phone setting and bridge event/discovery field. Management requires both agent control and this permission. View remains read-only. Existing devices retain current grants without gaining new powers.

Check grants on every operation and again before issuing daemon mutations. Downgrade/revocation prevents further mutation dispatch, aborts undispatched tasks and fences late results. Operations already committed by a daemon cannot be recalled; report uncertainty honestly.

Mutations need bounded request IDs, in-flight deduplication and operation status/result reconciliation. Never auto-retry an uncertain Spawn/CreateWorktree/SpawnShell; that can duplicate work. Retain the submitted form and offer explicit status refresh before another launch. Define daemon correlation/idempotency support if required to resolve committed-but-unacknowledged launches. Socket cancellation used for prompt delivery is not a general rollback for management operations.

Pins, recency and Remove from recents need no daemon changes. They remain local and cannot affect another viewer.

## iOS structure and interaction

Reuse ChatModel's authenticated pinned WebSocket, existing host selection and AgentControlModel's permission/screen fencing. Introduce navigation destinations for Point Guard, Projects, Project workspace and Agent; history is a right-side overlay associated only with Point Guard.

Left navigator and right history are mutually exclusive. Each overlay owns focus and disables the covered content; support scrim, Close, VoiceOver escape, reduced motion and keyboard focus containment. A pending Pi dialog must remain host/session-bound and discoverable, without stale results targeting another destination.

Opening an agent changes the viewed destination only: no Stop/New/Resume/send. Close the prior attach through its originating transport and reject stale open/output/size events. Preserve existing pending-submission recovery.

Default agent rows about 76 pt for name/context/status; navigation rows and controls at least 44 pt. Drawer width min(available width − 44 pt, 328 pt). Dynamic Type grows rows; no tiny-text compression. Use original logo assets and proper stroke folder icons. Long labels truncate visually with complete accessibility text. Selection and Answer needed use text/shape as well as color.

## Scope boundary

Required: flat pinned/recent navigator, Point Guard right history, Projects list/workspace, Open in project, metadata, New agent/worktree/terminal, rename and narrowly scoped Stop, with explicit management permission.

Not implied: desktop layout document/pane-tab sync, file editing/git/review, arbitrary SSH setup, project filesystem deletion, bulk kill, deleting worktrees, schedules, or arbitrary execution/RPC API. Native agent transcripts are not a new phone-owned chat database. Readiness/confirmed prompt semantics remain daemon-owned.

## Acceptance criteria and tests

1. Two matching top navigation rows; no dot/chevrons/search/footer duplication. Folder icons are actual outlined assets, not Unicode glyphs.
2. Long-press Pin survives relaunch and a burst of 20+ new agents. Output does not reorder recent interactions; pin does not duplicate a row.
3. Projects lists empty and active registered projects. Workspace shows root agents, worktrees/branches and availability for local and forwarded machines.
4. From pinned, recents or workspace, Open in project locates the correct machine/project/worktree and highlights the agent. No navigation-history-dependent label.
5. Right history provides search/current/date/preview/new/resume and preserves busy confirmations.
6. Existing off/view/control and new management grant are enforced server-side. A prior control grant alone cannot spawn, rename or stop. Off leaks no restricted fleet metadata.
7. Spawn agent, worktree and terminal appear in the correct workspace; form errors preserve edits; uncertain receipts never trigger duplicate launches.
8. Remove from recents neither kills nor deletes files/transcripts/drafts; the agent remains reachable in Projects. Rename propagates via authoritative inventory.
9. Cross-host/machine ID/path/name collisions do not mix pins, forms, drafts or results. Late inventory, dialogs, picker results and terminal events cannot affect a newer selection.
10. Preserve every row of the existing-feature table. Test submitted-text recovery, permissions downgrade during operations, offline forwarded machines, legacy metadata/geometry and large-output resync.
11. Unit/domain tests cover sorting/pins/hidden-entry state, navigation mapping, decoding fallbacks, operation state and recovery. Bridge fake-Unix-daemon and real authenticated WS fixtures cover authorization, target validation, cancellation fences, partial inventory and duplicate/uncertain mutations.
12. Run relevant Node tests/checks and Swift source/typecheck harnesses. User runs Xcode builds/XCTest/device checks; agents do not run xcodebuild. Manual review includes 375/390/430 pt, large Dynamic Type, dark/light, VoiceOver, touch long-press and keyboard.
13. Prior tree-design requirements are superseded where navigation differs; trust, input safety, recovery and existing-feature preservation remain mandatory.

## Implementation sequence

1. Define the additive inventory/management contract, explicit management permission and mutation reconciliation behavior. Extend bridge/daemon only where existing operations are insufficient.
2. Add Foundation navigation/preferences/form domain and wire decoding, preserving existing persistence keys and transport.
3. Build chosen left navigation, Projects workspace and Point Guard right history; integrate existing content screens.
4. Wire management forms and authoritative refresh; verify end-to-end and remove redundant old sheet entry only after parity.

This is dependency order, not permission to ship fake management controls. No application/backend implementation has been made by editing the prototype.

## Code evidence

At the audit baseline:
- apps/purepoint-mobile/PurePoint/ChatView.swift and SetupViews.swift: existing shell/sidebar, composer, pairing, history and dialogs.
- apps/purepoint-mobile/PurePoint/ChatModel.swift and ChatDomain.swift: one selected paired host, native sessions, pinned transport, identities and recovery.
- apps/purepoint-mobile/PurePoint/AgentsView.swift, AgentControlModel.swift and AgentControlDomain.swift: inventory fields, readiness, controls, outcomes and scoped persistence.
- apps/purepoint-mobile/PurePoint/AgentTerminalView.swift: read-only SwiftTerm and geometry.
- apps/purepoint-mobile/bridge/daemon.js and terminals.js: fleet socket resolution and current fixed agents API.
- apps/purepoint-mobile/bridge/trust.js and admin.js: current device permissions.
- apps/purepoint-mobile/docs/contract.md: shared Pi and Agents wire contracts.
- crates/pu-core/src/protocol/inventory.rs: project/worktree metadata.
- crates/pu-core/src/protocol/mod.rs: Spawn, SpawnShell, Rename, CreateWorktree, Kill.
- crates/pu-core/src/protocol/targets.rs: explicit Agent versus bulk Kill targets.
- apps/purepoint-macos/purepoint-macos/Models/SidebarItem.swift and Views/Sidebar/: real desktop project/worktree/workspace hierarchy and actions.
