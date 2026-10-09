# Shared channel system · v3

Snapshot saved 2026-10-09T17:19:34.237562+00:00. User accepted the quieter v2 layouts and requested Slack/Discord-level channel aesthetics and usability. v3 proposes a common channel component across project overview, optional worktree pane, and full channel. Existing v1/v2 snapshots and Canvases are preserved. Production code was not changed.

## Review links and exact source

- [Project](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/wdabqps5xv7o8s1326q4f1uf) — page `wdabqps5xv7o8s1326q4f1uf`, observed revision 1, SHA-256 `1819793fe7396b1538fefc9755750fa8a3b2ce208482661a275ed134430b7dba`.
- [Worktree](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/toovv5ea7b8ckgpknr4n7mqm) — page `toovv5ea7b8ckgpknr4n7mqm`, observed revision 1, SHA-256 `db4cdee28349f1a741456714d0fdd56a59a54558d5a854eeef0b4982071fd88e`.
- [Channel](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/titpzeae2wj2msf00asnof68) — page `titpzeae2wj2msf00asnof68`, observed revision 1, SHA-256 `48985e330ece535536149d3098f4d80ced5df346a82a1b9bd593799690e79b0a`.

Exact inline HTML sources are saved at `designs/agent-watch/v3/project.html`, `worktree.html`, and `channel.html`, with source hashes, timestamps and readback verification in `manifest.json`. All saved Canvas bodies matched local files byte for byte. Component authoring source is `channel.js` and `channel.css`; each is embedded in the HTML snapshots. Revision metadata is observed state, not a claim of immutable revision URLs. Preserve this named snapshot before another direction.

## Shared visual system

Preserve the app framing, native sidebar and v2 hierarchy. Channel title is #project, with search, member disclosure and full-channel expansion as small icon actions. System fonts and repository surfaces remain; message body increases to 13 px with 1.55 line height. Authors are semibold 12–13 px, timestamps 9–10 px. Avatars are rounded-square 30 px desktop / 26 px narrow, with subdued blue-gray for agents, plum for review and teal for the human. Avatars are identities, not process-status dots. AGENT label disambiguates humans and tools. Same-author adjacent messages group without repeating avatar/header. Worktree context is secondary and hides in tight panes. Commit/PR references use restrained bordered chips and omit duplicate worktree references.

Date separators use subtle rules. New-message separator uses restrained warm text/rule; an actionable blue unread bar and matching sidebar count provide navigation. Message actions appear on hover or keyboard focus, with inline SVG icons. On touch they remain accessible below the message. Composition is a rounded 9 px, outlined input with bottom actions and an explicit Send icon. No persistent thread pane or terminal was added.

Component recreation mapping and existing app tokens remain in [v1 notes](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/gbt5m64u6i3wf9cn6eimyp3y). These are HTML recreations of proposed native controls, not original SwiftUI/AppKit components. Actual PurePoint logo is embedded. New avatar/hover/mention colors are candidate semantic tokens in `channel.css`, with light and dark variants; production should adapt them to native appearance. Icons are inline SVG drawings, to map to SF Symbols in production. No external libraries, fonts or scripts are loaded.

## States and interaction identifiers

- C01: Default timeline and composer, two unread messages. C02: consecutive-author grouping. C03: date/new-message separators and unread navigation. C04: Search/results/no-results. C05: hovered/focused actions and reaction. C06: inline thread collapsed/expanded with replies. C07: own-message editing. C08: member/about disclosure. C09: mention autocomplete. C10: multiline/inline-code composition. Empty/loading/error states remain available from v2.
- P01/W01: same accepted quiet structure; W01 has no terminals and channel closed by default. New channel is reused when opened.
- Hooks: `.channelhead`, `.messages`, `[data-message]`, `.msgactions`, `[data-thread]`, `.threadblock`, `[data-react]`, `[data-more]`, `[data-edit]`, `.composer`, `[data-compose]`, `.mentionpicker`, `#search`, `#jumpunread`, `#markread`, `#jumplatest`.

## Usability behavior

Enter sends, Shift+Enter inserts newline; composition guards IME input. Send disables when empty. Draft survives navigation between project, full-channel and optional worktree views in memory. Composer grows to 180 px. @ autocomplete filters known participants; Up/Down selects, Enter inserts, Escape dismisses. Inline-code button wraps a selection; backtick text renders as code. Search filters author/text/context and provides count/no-results; Command/Ctrl+F focuses it, Escape closes. Unread bar jumps to first unread or marks read; sidebar count synchronizes. Jump-to-latest appears when reading older content. Search does not redirect worktree selection.

Hover/focus actions expose a lightweight thumbs-up reaction, inline reply, and More. Threads expand under their parent with a separate composer; main-channel draft stays intact. More includes Mark unread and Edit for your own messages; editing marks the message as edited. Member button reveals identities, not presence inferred from Git changes.

## Simulations and implementation work

All history, replies, reactions, editing, identity, timestamps, Git references, search and read state are sample/in-memory data. Reload resets them; there are no backend submissions. No real messages, mentions, terminal input or agent jobs were sent. Read/edit/reaction/thread APIs and durable draft/read state are additional proposed UI capabilities, not existing engine features.

Implement one shared native channel component in the three contexts. The first backend remains durable per-project messages with authenticated sender identity, worktree references, ordered cursor and CLI/UI send/read parity. If replies/reactions/editing are adopted, they need durable IDs, parent references, authorization and update events. Only the author may edit. Search must paginate/index real history. Read cursor and draft should survive actual view reconstruction; new messages must preserve readers' scroll position and only follow latest when already at bottom. Mentions visually identify participants; immediate agent interruption/routing remains a separate explicit design, not automatic behavior. CLI posting stays intentional. Production should add proper long code-block/link rendering and delivery/retry states; the prototype covers inline code and simple reference chips only. Attachments, global workspace search, rich-text editing, custom emoji and Slack integrations are not part of this revision.

## Observable acceptance

- Channel feels consistent across full and embedded views; text wraps without horizontal page overflow.
- Same-author messages group, identities remain clear, and timestamps/worktree context recede.
- Unread timeline and sidebar count agree. Mark-read and jump navigation are usable by keyboard.
- Search and message actions are accessible without permanent extra chrome.
- Enter/Shift+Enter and mention selection behave predictably; empty Send disables.
- Reply, reaction and own-message edit preserve main draft and reader position.
- Opening worktree still shows full-width branch review and no terminals until the user navigates to an existing agent workspace.
- Ordinary messages do not wake agents or inject terminal input.

## Verification and limits

Local Chromium screenshots/checks at 1280×800 light/dark and widths 1000/700/390 had no document horizontal overflow or JS errors. Visual inspection covered full-channel dark, project light/minimum, narrow full-channel, thread expansion, hovered actions, and embedded worktree channel. Interaction checks passed for original logo loading, consecutive grouping, shared draft, search/no-results, reaction toggle, thread reply, own-message edit, synchronized unread dismissal, Enter send, Shift-Enter multiline, keyboard mention autocomplete, inline code and member disclosure. Existing navigation/diff/base/PR/channel-toggle checks passed. `verification.json` and `channel-verification.json` record the results.

Native app screen access and PageSpace rendered preview remain unavailable. Saved source readback is verified; local screenshots do not verify actual PageSpace sanitizer/theme bridge/CSP behavior or running Swift controls. Formal screen-reader/contrast audits and real CLI/backend delivery are not verified. Reduced-motion CSS disables smooth scroll. This is a reviewable design candidate, not a claim to match every Slack/Discord feature.
