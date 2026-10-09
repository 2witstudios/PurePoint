# Agent watch and project channel handoff

Owner-authorized outcome: implement the approved quiet project overview, committed/local worktree review without terminals, and intentional shared project conversation across native UI and CLI. Selected design: designs/agent-watch/v3/manifest.json and pinned HTML snapshots.

## Candidate and ownership

Branch: pu/staged-changes. Independently approved composed source candidate: b574dfc8069decc69996f855c5f549dc5a9a8792. Conductor/native writer: ag-fy2k4w7a. Independent composed reviewer: /root/implementation_plan_review. No outstanding source findings in reviewed scope; owner native acceptance remains outstanding. Subsequent delivery-record commits contain administration and reusable verification scripts only.

Channel Rust producer: ag-dp8rbros, approved exact c757e32a7f49a73f9463ac38210fc78f80519539 by independent channel_review. Integrated base15fc898 at036a7ea and delta atbf144fc. Git producer: ag-e9ihgkw7, approved exact8590b7fe27eb31a86a6eb839e4bc74a50464786a by ag-vkt12khw/wt-b26vpyxq. Integrated98b77f8,04a7341,d6dbc7c,9f944dc,8590b7f; final delta integration40764c8. Actual producer sources are present; no producer stubs.

## Outcome and contract

- Project overview: compact root/worktree rows with committed/local counts and shared conversation.
- Worktree: cumulative branch changes by default, explicit base selection, staged/unstaged/untracked groups, commit list/patches, PR diffs, editable files and optional channel; no terminals in monitor.
- Native channel: grouped authors/avatars, dates/times, explicit references opening real patches, search including matching replies, unread sidebar/timeline navigation, inline threads/reactions/own edits, member disclosure, hover/focus actions, typed native mention completion, inline code control, growing native composer, Enter/Shift Enter and IME preservation.
- Shared per-project state: drafts and out-of-order read observations persisted; actual viewport/active-scene read marking; FIFO serialized history/paging, complete older-thread navigation, old loaded edit refresh, captured reply destinations and durable unconfirmed-send errors.
- CLI: pu channel send/read/edit/react; explicit/env/Git common-dir routing, stable daemon-effective UID human identity and manifest-validated agent snapshots. Channel reads/messages/mentions never inject terminal input or trigger agents. Additive wire keeps protocol6, locked/fsynced/atomic version1 per-project JSON store. See docs/product/project-channel.md for exact fields, validation and cursor semantics.

## Criteria and evidence

| Criteria | Actual code/proof |
|---|---|
| CH001–005 shared ordered intentional persistence, ownership, replies/reactions | pu-core channel store, engine channel handler, CLI channel; real socket and four-process lock proofs; full Rust workspace tests |
| CH006–010 quiet overview/monitor, cumulative commits/local groups, explicit base | native detail views, GitService/DiffState; real temporary Git fixtures, quoted/binary/rename/unborn/invalid/unrelated bases; conflict/stage2/empty-baseline variants |
| CH011 refresh and retained state/errors | generation guards and FIFO channel read queue; Git state/PR refresh fixtures; old loaded channel edit, pagination, reply-switch and uncertain-delivery checks |
| CH012–014 channel usability/drafts/read state | actual native components typecheck; Rust fixture decoded in Swift; state harness; independent source reviews; isolated native light/dark channel renders inspected |
| CH015 visible unavailability/corruption | validation/store corruption and unsupported-version tests; Git and remote errors; native retained data/error state |

## Checks run

PASS on composed source: cargo test --workspace --all-targets in isolated /tmp/purepoint-agent-watch/channel-target (CLI117, core341 + ignored worker exercised by process test, fixture1, engine156, integrations6+13); cargo fmt --all -- --check; clippy --workspace --all-targets -- -D warnings; git diff --check.

PASS: tools/verification/check-channel-state.sh (real serializer fixture, wire keys, persistent drafts/read cursor, unread catch-up beyond latest page, context-safe cursors, loaded older edit refresh, search-free thread reads, captured reply destination/draft safety, uncertain send preservation/no replay). Independent composed reviewer reran it.

PASS: tools/verification/check-agent-watch-views.sh (strict Swift5/mainactor/nonisolated settings, actual channel/review/file-edit components and actual wire/Git/state/editor producers). App-wide ProjectState/routing, terminal axis, syntax facade and GH-unavailable view are explicitly isolated dependency fixtures: this is not a whole-app build.

PASS: tools/verification/check-git-review.sh (standalone actual real-repo and state/remote harness). Independent Git reviewer separately compiled/reran equivalent strict harness and original conflict probes. Builder separately ran real linked-worktree watcher callback and daemon mapping/default-base/equality harnesses.

PASS: independent channel producer review, independent Git producer review and independent composed source review, zero open confirmed source findings.

## Outstanding acceptance and limitations

NOT RUN: full native Xcode app build/XCTest, rendered full-app sidebar/overview/worktree inspection, keyboard/IME/accessibility/scroll position/read visibility checks, NSApp foreground 2-second Git polling and 30-second PR timer. Responsible: owner in Xcode, before main acceptance. AGENTS.md prohibits agent-run xcodebuild/swift build because the app build phase overwrites installed binaries and can kill the active daemon; no app build/install/daemon replacement was performed.

Standalone native component light/dark rendering is not full app interaction evidence. Crash fault injection was not performed; real thread/process locking and atomic persistence are tested. Existing Git process SIGTERM timeout is not a hard guarantee against an ignoring process or inherited child pipes. Git separate metadata without primary backlink requires explicit project root for linked-checkout routing; ordinary root/subdirectory/linked-worktree routing has actual tests.

No merge, deployment, install or cleanup of other sessions. Task remains In Review, not Done. Continue by running owner native checks, resolving any failures, and re-reviewing any source delta before main acceptance.
