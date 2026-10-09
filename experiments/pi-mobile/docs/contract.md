# Pi Mobile standalone contract

Maturity: SPECIFIED. Scope: standalone iPhone chat → Mac bridge → vanilla Pi. No dependencies on PurePoint engine, desktop or IPC. Authorizing brief: Jono, 2026-10-08.

## Purpose and conceptual model

The Mac process owns one persistent Pi RPC child. The phone is one authenticated controller and a replaceable view. Pi owns session storage and execution. Phone drafts and submission receipts are recovery aids, never a second authoritative transcript.

## Research and decisions (2026-10-08)

Research progressed SEED (lifetime / session authority questions), EXPLORING (upstream and harness inspection), CONVERGING (SDK versus RPC, log replay versus snapshots), DECIDED, then SPECIFIED here before implementation.

! [MOB-001] Use a child RPC process rather than an in-process agent — isolates runtime and keeps stdout/stderr distinct. Upstream published `@earendil-works/pi-coding-agent` 1.1.0 verified with npm registry. Lockfile pins all dependencies. Upstream inspected at ce950d78f424dcaf9f5d6a03ce80ab141130eb1d; published package docs/source are final authority. Sources: [package](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/package.json), [RPC](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/rpc.md), rpc-mode.ts / rpc-types.ts, [commands](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/rpc-commands.md).

! [MOB-002] Use revisioned authoritative snapshots rather than a durable network event ledger — a single child and controller need reconnect state, not another transcript store. Walk get_entries parentId from leafId; do not flatten abandoned branches. Display raw branch history with compaction/branch-summary markers; this is reading history, not rebuilding model context. Pi alone applies firstKeptEntryId/context edits to model context. Read-only session browsing uses native SessionManager.inMemory with bounded file entries; native SessionManager.listAll enumerates metadata. Switching uses RPC switch_session. Sources: [session format](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/session-format.md), session-manager.ts.

! [MOB-003] Serialize mutations; use epoch/run target for Stop — clear_queue returns text, pending dialogs are canceled so abort can reach idle, abort waits for idle, and agent_end can precede automatic continuation. agent_settled is the idle boundary. Dialog responses bypass mutation serialization so extension commands cannot deadlock. No automatic prompt replay. Unknown protocol/operations reject visibly.

! [MOB-004] Bind only explicit tailnet IPv4/IPv6 or loopback, authenticate in WebSocket HTTP upgrade with a private bearer secret generated on first setup or supplied by the owner — Tailscale encrypts traffic even for ws inside the tailnet. Optional owner TLS supports wss. Reject public/wildcard binds. Second controller is rejected; disconnect never aborts Pi. No global installation. Credential generation is local and explicitly owner-authorized.

! [MOB-005] Preserve vanilla resources — no provider overrides or no-skills flag. Pi discovers ~/.agents/skills (including symlinks). Discover a pu skill in installed Codex skill/plugin roots and add its path explicitly. get_commands validates that skills loaded. Clear PU_AGENT_ID and PU_PROJECT_ROOT. Cross-project pu commands must route with a per-command PU_PROJECT_ROOT or explicit cwd (installed pu has no general --project-root flag). Source: skills.md, configuration.md, skills.ts. Read-only PageSpace harness precedent at 0cc42713700b5a779ba88feb62054c583c0f7eba: bin/pagespace.mjs uses argument arrays and extension preload; extensions/pagespace.ts replaces filesystem/provider behavior and its launcher disables native skills. None of those restrictions/replacements are adopted.

! [MOB-006] Native SwiftUI with system typography and PurePoint's original Point Guard light/dark logo and icon. Neutral user bubbles, adaptive system backgrounds, 20pt transcript margins, 24pt turn spacing, Dynamic Type, selectable Markdown and monospaced copyable code. Graphite accent #353632 in light appearance, soft stone #E6E4DD in dark appearance; white/black system canvas and adaptive gray surfaces. No blue, introductory paragraphs, setup footers or tagline. QR is the primary connection action; manual fields expand on demand. Use native multiline editing and explicit buttons so newline/IME never submits. Consecutive tool calls form one inline expandable activity group with consistently aligned rows, no filled container and no tool icons; native results and matching live activity appear once. Prose retains its chronological place, and blank tool-only assistant records create no spacing. Native errors and brief operational status remain visible. Reviewed against the revised owner brief: exact existing brand artwork is the visual anchor; typography stays native and the activity surface encodes grouping rather than decoration.

## Requirements

- REQ-MOB-001: Given phone disconnect/backgrounding, should keep Pi running and reconnect to authoritative branch history plus partial response/tools without replay or duplicates.
- REQ-MOB-002: Given fragmented stdout, should split only LF, preserve Unicode/UTF-8, correlate IDs, separate diagnostics, bound records/requests/client buffers and visibly fail on child exit.
- REQ-MOB-003: Given idle/busy composition, should submit complete text with explicit Send/Steer/After reply, show started/queued/handled receipts separately from completion and preserve later edits and uncertain submitted text.
- REQ-MOB-004: Given Stop, should clear queued text before abort, retain canceled text, target the observed epoch/run, reject stale commands and never kill pu workers.
- REQ-MOB-005: Given native sessions, should create/list/browse/resume; require idle for switching or offer explicit Stop then switch; respect tree and compaction semantics.
- REQ-MOB-006: Given extension dialogs, should present confirm/select/input/editor and correlate answers/cancellations; retain pending dialogs across reconnect, expire timed requests, and preserve composer drafts on set_editor_text.
- REQ-MOB-007: Given missing Pi/model/skills, should provide setup guidance without reading or printing secret files; preserve native provider/auth and use explicit cwd/argument arrays.
- REQ-MOB-008: Given invalid auth/version/second controller, should reject; bound inbound/outbound network data and never expose arbitrary RPC commands or filesystem paths as operations.

## Interface

WebSocket upgrade: Authorization: Bearer OWNER_SECRET, /v1. First record `{version:1,id,op:"sync"}`. All requests include version, id (unique UUID), op. Operations: sync, sessions, history(sessionId), send(text,mode:send|steer|after,epoch), stop(epoch,runId), new(epoch), resume(epoch,sessionId), answer(dialogId,value|confirmed|cancelled).

Replies: `{type:"receipt",id,ok,data?,error?}`. Command timeouts are uncertain, never retried automatically. Canceled queue text is retained in bounded bridge-lifetime recovery snapshots. Cached request IDs are bound to one bridge lifetime; repeated IDs reject rather than redispatch. Snapshots: `{type:"snapshot",version:1,epoch,revision,runId,busy,sessionId,title,messages,tools,queue,canceled,dialogs,notices,error?}`. Epoch rotates on session changes/runtime restart. IDs for rows derive from native role/timestamp or entry IDs; live assistant final replaces partial. Messages contain id,role,text,activity,error. Text is bounded and truncation marked. Snapshots replace the phone projection; revision suppresses old delivery. Read-only history returns a separate projection without selecting it.

## Edge cases and limits

One controller, one active session. No bridge crash durability for request receipts; Pi sessions survive, partial in-flight generation may not. Restart creates a fresh session unless PI_MOBILE_SESSION is supplied. Stop cannot cancel an extension handler before it starts a future independent run; stale epochs/runs reject. Dialogs may be canceled by Pi timeouts; bridge expires them. TUI-only custom widgets are unsupported by Pi RPC; string widgets/status/notices are surfaced. Images display a text placeholder in this text-first slice. At most 500 visible history rows, 64KiB per text, 100 tools, 32 dialogs, 8MiB RPC records, 16MiB history files, 1MiB network requests and 4MiB client backlog. Older complete history remains in Pi native files. Tailnet membership/access policy and Mac availability remain owner-managed.

## Sum sheet and module boundaries

| Situation                      | Required result                                                                             |
| ------------------------------ | ------------------------------------------------------------------------------------------- |
| Phone detaches                 | Pi child remains alive; no abort/replay                                                     |
| Reconnect                      | Native active branch + current partial/tools/queue/dialogs + bounded canceled-text recovery |
| Submit                         | Local draft captured once; distinct acceptance receipt; later edits remain local            |
| Stop observed run              | Clear queue → cancel blocking dialogs → guarded abort → idle refresh                        |
| Browse / switch                | Read-only native projection during work; explicit idle/Stop gate before native switch       |
| Child fails / wire unsupported | Visible setup/error state; no silent restart or uncertain resend                            |

`core.js` exposes independent framing and branch/live projection helpers. `rpc.js` owns process transport and correlated commands. `controller.js` owns semantic operations and bounded in-memory projection/recovery; it depends on injected RPC/session interfaces. `setup.js` adapts native resource/session discovery; `network.js` authenticates and transports semantic records; `main.js` composes them. Swift ChatDomain is Foundation-only; ChatModel owns phone transport/draft recovery, PairingSecret owns Keychain IO, and SwiftUI views own presentation. Neither core/client calls PurePoint APIs. Public wire operations and limits are specified above; tests are colocated with each bridge boundary and the Swift domain target.

# QR pairing addition — 2026-10-08

The standalone bridge emits an offline SVG QR in a private local HTML file, using a private credential generated on first setup or an existing owner-supplied credential. The phone uses Apple's VisionKit DataScanner, checks support/availability and requests camera permission. Research: [Apple scanner contract](https://developer.apple.com/documentation/visionkit/scanning-data-with-the-camera), [node-qrcode SVG API](https://github.com/soldair/node-qrcode). Local generator dependency is pinned to qrcode 1.5.4.

- Given a valid bridge address and private owner secret, startup should save a mode-600 offline QR page and print only its path. Failure should leave manual pairing and the running bridge available.
- Given a supported camera and permission, Scan Mac QR code should read a QR, validate it, store its secret through existing Keychain pairing, and connect once. Cancel should leave the existing connection and draft intact.
- Given an unsupported device, denied permission, malformed QR, oversized payload, public endpoint or unsupported version, the app should show useful guidance without sending credentials or changing its pairing.
- Given logs and local page contents, the plaintext secret should appear in neither. The page should contain no remote resources; its QR itself contains the secret and must remain private.

Payload: UTF-8 JSON with `type: "pi-mobile-pairing"`, `version: 1`, `endpoint` and `secret`. Maximum scan payload 8192 bytes; secrets 32–1024 bytes with no embedded CR/LF. The same tailnet/loopback endpoint policy applies. No custom URL scheme, relay, camera-photo storage, or network protocol change. First setup generates 32 random bytes, atomically publishes a mode-600 secret file, and reuses it thereafter. Existing invalid files are rejected without replacement. Jono explicitly requested automatic credential generation after the initial manual-setup implementation. Reusable QR validity follows the owner's existing credential; it is not an expiring one-time enrollment token.

## Mobile visual revision requirements

- Given native tool results plus matching live activity, the conversation should show each call once in one consecutive activity group, preserving running/failed state and output.
- Given assistant prose between tool calls, grouping should preserve that order; given an empty tool-only assistant record, it should create no extra gap. Empty error records must remain visible.
- Given light or dark appearance, controls and message bubbles should use neutral adaptive colors and the corresponding unmodified PurePoint logo.
- Given first launch or connection settings, the screen should show the Point Guard brand and concise actions without introductory or protocol explanations. Manual connection and actionable errors remain available.

## Composer, sidebar and attachment revision — 2026-10-09

Pinned upstream RPC types and implementation at `ce950d78f424dcaf9f5d6a03ce80ab141130eb1d` accept native `ImageContent[]` on prompt/steer/follow_up. They do not expose a general arbitrary-file upload API. Source: `packages/coding-agent/src/modes/rpc/rpc-types.ts`, `rpc-mode.ts`, and [published RPC documentation](https://pi.dev/docs/latest/rpc-commands). Apple's PhotosPicker, security-scoped fileImporter, ImageIO thumbnail decoding and PDFKit provide the native import boundary. No core fork or Mac file-upload storage is introduced.

- Given the keyboard is visible, the composer should remain one rounded surface with text, attachment chips and bottom controls, inset from the screen edges; no full-width contrasting background strip. Return/IME remains editing only.
- Given the sidebar button or left-edge swipe, a left conversation drawer should open with search, new conversation and connection settings. Backdrop tap, close action, accessibility escape and leftward swipe should dismiss it. Selecting a conversation resumes when idle; during work it opens read-only browsing with the existing explicit Stop/resume decision.
- Given light/dark appearance, primary resume text should explicitly contrast with its background; no pale-on-pale dependency on the system prominent style.
- Given an attachment selection, the draft should show a removable filename/thumbnail chip. Images should be downsampled/oriented and encoded as JPEG; UTF-8 files and selectable PDF text should enter the prompt as named text. PDFs are not sent as original binaries, scanned PDFs are rejected with actionable guidance.
- Given image send support, snapshot `capabilities` should include `images`; an older bridge should fail visibly on the phone before image submission rather than silently discarding it. `send.images` is optional and retains wire version 1.
- Given malformed, noncanonical base64, unsupported/mismatched image MIME/signature, non-image model, more than four images, or total images over 512 KiB, the bridge should reject before calling Pi. JSON requests stay under the existing 1 MiB limit.
- Given busy Pi, attachments should remain local for the next idle message. Pi's clear_queue returns text only; image queuing is deliberately not offered until recovery can retain native queued images reliably.
- Given submission/connection loss, original text and attachment bytes should remain in local recovery; newer typing/attachments should remain unchanged. Uncertain inputs must never auto-resend. Draft attachments persist per endpoint/session in a private app-support JSON file (10 cached drafts), recovery receipts persist separately (50 receipts). These are draft/recovery data, not a second conversation database. Older text-only receipts migrate from UserDefaults. Native image history remains Pi's authoritative storage and mobile history displays image placeholders.

Import bounds: four attachments, total prepared bytes 512 KiB; images source ≤20 MiB, thumbnail ≤1600px, prepared JPEG ≤256 KiB each; UTF-8 file/extracted PDF text ≤32 KiB, PDF ≤30 pages; combined prompt text ≤64 KiB. Files are read under a scoped URL grant and are not written into a project on the Mac.

### Interaction responsiveness

- Given cold launch, saved attachment/recovery JSON and the connection Keychain read should load off the UI actor. Editing while recovery loads should keep the newer local attachment draft. Connecting waits for recovery hydration without blocking the composer.
- Given rapid typing or attachment edits, persistence should coalesce for 250 ms on a serial utility queue; the latest pending value per storage key wins. Backgrounding flushes pending writes asynchronously. Sending flushes recovery before network transmission, while the composer remains editable.
- Given streamed snapshots, decoding and native/live transcript grouping should run off the UI actor, in receive order. A connection-generation check after background work should prevent a disconnected socket from applying stale results.
- Given unchanged message content while typing or receiving a status update, SwiftUI should skip the existing row body. Markdown parsing and attachment image preparation should run off the UI actor; newer Markdown tasks must cancel stale view updates.

Grounding: Apple's [Improving app responsiveness](https://developer.apple.com/documentation/xcode/improving-app-responsiveness) recommends keeping synchronous non-UI work off the main thread and verifying hangs with device profiling. These changes address observed code paths; their device latency benefit remains to be measured on the owner's phone.

- Given physical-device testing, a dedicated shared Device scheme should run an optimized app without LLDB, while the original debug/test scheme remains available. Both should target the same app and signing configuration.
- Given startup diagnosis, static launch logs should distinguish entering the SwiftUI App initializer from chat view appearance; they must not include credentials, endpoints, conversation data or device identifiers. These markers describe app lifecycle events, not guaranteed first rendered pixels.
- Given connection settings appearance, loading an existing Keychain secret should not block the UI thread or overwrite a newer manual secret/address edit.
