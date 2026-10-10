# Point Guard setup and trusted phone access

**Maturity: SPECIFIED** | ID Prefix: PGSET | Dependencies: `architecture/distribution.md`, `architecture/desktop-app-integration.md`, `architecture/module-structure.md`

## Purpose

A fresh supported Mac installs PurePoint, configures Pi through native provider login, chats without external Node/npm/Pi/repository setup and optionally pairs an iPhone once. This scoped spec governs Pi bridge setup, not remote worker-daemon transport/federation. Existing shell and split panes remain.

## Conceptual Model

App owns a packaged Node bridge/Pi child for app lifetime. Pi owns native durable credentials/sessions. A private per-user managed directory owns runtime selection, local principals, readiness and stable remote TLS identity/device trust. Local admin, local desktop chat and remote device chat are separate listeners/authorities. Tailscale supplies reachability; TLS pin plus device credential supplies application trust.

## Research Notes

**Researched: 2026-10-09**, baseline c0d14120ef0f62d7a8f7790bf022e7ebe7b2d856. Existing bridge preserves native sessions and central queue/Stop ownership; rewriting loses established behavior. Existing setup requires developer skills/cwd and shared token QR lacks host or device identity.

Pinned installed Pi 1.1.0 declarations/implementation (runtime builder) expose CredentialStore/AuthStorage read/list/modify/delete and provider.auth.oauth/apiKey.login interactions, not AuthStorage.login. Provider capabilities must come from pinned catalog. Auth updates need cancellation/generation check inside credential lock, private canonical auth storage and sanitized errors. Source: [Pi v1.1.0](https://github.com/earendil-works/pi/tree/v1.1.0/packages/ai).

App Process supervision fits approved app lifetime; launchd would add continuous service scope. State outside bundle and exclusive startup ownership preserve updates and prevent collision adoption. Distribution research compares migration and supervision alternatives; ad-hoc artifact CI and protected Developer ID proof are separate.

Pinned remote HTTPS/WSS reuses [Node HTTPS](https://nodejs.org/api/https.html) and [URLSession trust challenge](https://developer.apple.com/documentation/foundation/urlsessiondelegate/urlsession(_:didreceive:completionhandler:)). An unsigned hostId or signed challenge over plaintext cannot prevent bearer theft/relay; full encrypted application transport would be a rewrite. Pin SHA256 certificate DER before credentials; authenticated hostId remains inside TLS. Stable private self-signed TLS is provisioned once via system openssl; identity corruption/expiry requires actionable deliberate recovery, never automatic replacement. Keychain device-only storage uses [Apple accessibility](https://developer.apple.com/documentation/security/ksecattraccessibleafterfirstunlockthisdeviceonly).

## Decisions

! [PGSET-001] Reuse Pi 1.1.0 bridge with supported packaged Node, all npm transitive/resources and versioned Point Guard/pu instructions — preserves Pi behavior and removes end-user development dependencies.
! [PGSET-002] App owns exclusive runtime lifetime and stops/awaits only its children before update replacement — no daemon rewrite or process adoption.
! [PGSET-003] Native provider auth uses catalog capabilities and guarded asynchronous SDK interaction — no unsupported OAuth promise or stale credential overwrite.
! [PGSET-004] Remote chat requires pinned WSS and per-device opaque credentials; QR v1 enrolls once — Tailscale/shared token cannot authorize devices or caller-selected identities.
! [PGSET-005] Local HTTP admin and local desktop chat are separate loopback listeners; separate private admin-token and desktop-chat-token are provisioned — paired credentials never gain setup/provider/device-admin rights. Desktop stable client identity is bound to its local credential, not an arbitrary incoming header.
! [PGSET-006] Preserve native session/auth and versioned trust outside the bundle, invalidate ephemeral enrollment on restart — update restores durable state without replaying uncertain prompts.

## Requirements

- Given empty HOME and no external Node/npm/Pi/skills, should launch the relocated complete artifact with native default home or selected valid cwd.
- Given missing credentials, should offer only supported provider OAuth/API-key flows with native prompts, cancel, expiry and provider-change handling; never log or QR credentials.
- Given stale/canceled/expired login, should reject its credential commit under the storage lock even when newer credentials exist.
- Given app/service restart or bundle replacement, should restore selected session/provider/cwd and host/device trust; owned children stop before replacement; uncertain messages remain manual recovery.
- Given startup lock/listener/identity collision, should fail closed and show actionable recovery without adopting/killing another process.
- Given Connect phone, should display exact short-lived QR enrollment for a reachable tailnet WSS endpoint with authenticated host pin.
- Given expired/reused/concurrent QR redemption, should reject all but one valid redemption.
- Given device credential, should bind client/queue identity to authenticated principal and reject header/message impersonation.
- Given explicit revoke/rotation, should durably revoke and close active sockets while preserving unrelated devices and desktop chat.
- Given remote-chat credential, should deny provider/config/device-admin access and never expose local owner credentials.
- Given saved reachable host, should reconnect on foreground/network change with five bounded retries; revoked trust or pin/identity mismatch should require deliberate re-pair before credential release.
- Given legacy shared-token remote pairing, should preserve local recovery/native data and require QR v1 without authorization downgrade.
- Given composed candidate, should pass independent complete exact-head review and applicable CI, both advertised package architectures, nested ad-hoc signing/entitlements/launch and state-preserving replacement fixture; human Developer ID/notarization/device acceptance stays pending.

### Existing native Pi directory permissions

Given an existing user-owned native Pi agent directory with group/other read or traverse permissions but no group/other write permissions, should reuse it without changing its mode or canonical auth/settings/session data. New native directories are created owner-only (0700). Existing native directories must be real directories, not symlinks, and owned by the current user; reject group/other-writable directories with sanitized actionable recovery. Native auth files remain strictly owner-only regular files with valid JSON; unsafe/corrupt files are never silently repaired. Managed Point Guard and trust directories remain strictly owner-only.

Pi 1.1.0 settings initialization can create the native directory using the process umask; typical 0755 permissions are compatible with a private 0600 auth file. Requiring managed-state directory permissions for this existing Pi directory incorrectly rejects normal terminal installations.

## Sum Sheet

packaged ∧ nativeAuth ∧ durableState ∧ appOwned ∧ pinnedHost ∧ individualTrust ∧ boundedReconnect ∧ noReplay ∧ independentReview ∧ exactHeadCI. ProductionActivation ⇒ ownerAcceptance ∧ protectedSigning.

## Interfaces — PRPG contract r2 / PRPG-TRUST-1

The following producer proposals are reconciled with these normative clarifications: admin-token never authorizes chat; desktop-chat-token authorizes only loopback native chat; remote device credential only remote chat. Descriptor includes nativeChatURL, desktopClientId, hostId and certificateSHA256 in addition to adminURL/remote chatURL. runtime.configure, runtime.restart and successful auth return restartRequired; native app performs idle-only owned stop/relaunch preserving state. Runtime persists desktopClientId bound to desktop-chat-token; optional POINT_GUARD_CLIENT_ID bootstraps first creation, cannot change existing identity silently. Remote listener absence while Tailscale unavailable must not prevent local chat/provider setup; pairing reports actionable missing reachability. Stop/relaunch must await exit before replacement. TLS helper/trust store belongs to trust producer; runtime alone wires main/setup.

# PRPG-1 contract proposal r1 (not reconciled)
Producer wt-qcjdczac baseline c0d14120ef0f62d7a8f7790bf022e7ebe7b2d856; integration SHA none yet.

## Finding: pinned Pi auth and persistence
- Agent: wt-qcjdczac
- Spec: docs/product/configuration.md; docs/architecture/storage.md
- Type: research-note / decision-proposal
- Content: npm lock pins @earendil-works/pi-coding-agent/pi-ai 1.1.0. Installed package declarations + implementation are source of truth. AuthStorage is CredentialStore (read/list/modify/delete), no legacy login method. ModelRuntime exposes provider.auth.apiKey.login(interaction), provider.auth.oauth.login(interaction, options); AuthInteraction prompt/notify/signal supports text/secret/select/manual_code and auth_url/device_code/info/progress events. Call provider-owned flow directly, then AuthStorage.modify only after app-owned generation/cancel/expiry checks INSIDE locked mutation: avoid late canceled login overwriting newer credentials. Preserve canonical ~/.pi/agent/auth.json, models/settings/sessions. Fail closed on insecure existing auth file; never log SDK error strings containing credentials.
- OAuth supported by pinned catalog: anthropic, github-copilot, kimi-coding, meta, openai, openai-codex, openrouter, radius, xai. Enumerate capabilities from installed catalog, not hardcoded list. Provider-owned api_key interaction also supports provider-specific env fields; ambient-only providers should not be advertised as login-supported.
- Sources: installed npm package dist/core/{auth-storage,model-runtime}.{js,d.ts}; pi-ai/dist/auth/types.d.ts; https://github.com/earendil-works/pi/tree/v1.1.0/packages/ai .

## Finding: supervision and packaging
- Agent: wt-qcjdczac
- Spec: docs/architecture/distribution.md; docs/product/recovery-resilience.md; docs/product/desktop-app.md
- Type: decision-proposal
- Content: app-lifetime Process ownership fits existing bridge and does not touch worker daemon. Separate per-user managed-state directory + exclusive startup lock prevents simultaneous owned runtimes. Never attach to/adopt/kill unknown listener or remove stale lock automatically; fail with actionable recovery. Graceful owned bridge SIGTERM closes RPC child first, then listener/admin/lock; app awaits owned exit before update replacement. No prompt replay. Alternative launchd costs privileged/long-lived lifecycle work outside approved app-lifetime boundary.
- Pin Node 22 supported patch using official SHASUMS; ship arch-specific arm64/x86_64 package containing entire production npm graph, bridge/docs + versioned support tree + explicitly passed app-bundled pu CLI. No references to developer home. Preserve all native Pi state outside bundle; replacements must not overwrite auth/sessions/trust. Default cwd native user's home, selected cwd validated directory.
- Sign nested Mach-O helpers/native modules inside-out (not --deep signing), Node hardened runtime allow-jit; preserve exact entitlements only needed; verify nested signatures and relocated launch on both architectures. Ad-hoc CI proves structure/execution, never Developer ID trust/notarization. Human production signing/notarization/fresh-device gate stays pending.
- Sources: https://nodejs.org/download/release/ ; https://developer.apple.com/library/archive/technotes/tn2206/_index.html ; https://developer.apple.com/documentation/Apple-Silicon/porting-just-in-time-compilers-to-apple-silicon .

## Exact proposed app contract
Bundle Contents/Resources/PointGuard/{runtime-manifest.json,bridge/,docs/,support/,node_modules/}; executable Contents/Helpers/{point-guard-node,pu}. Manifest schemaVersion=1, contractVersion=1, piVersion=1.1.0, nodeVersion pinned, architecture arm64|x64, sourceSHA, paths relative to manifest: node ../../Helpers/point-guard-node, pu ../../Helpers/pu, entry bridge/main.js, instructions docs/point-guard.md, skills support/{pu,pu-cli}/SKILL.md. All referenced support files vendored/versioned; manifest copied/rebuilt only before signing.
App launches explicit node path + main.js --managed. Env: POINT_GUARD_STATE_DIR (default ~/Library/Application Support/PurePoint/PointGuard), POINT_GUARD_PU_PATH absolute bundled pu, PI_MOBILE_CWD optional default home. Remove PU_AGENT_ID/PU_PROJECT_ROOT, put bundled pu directory first PATH; PI native agent dir unchanged. State private 0700, files 0600: runtime.json (schemaVersion,selectedSessionPath,provider,model,cwd); admin-token; admin.json (schemaVersion,contractVersion,pid,instanceId,adminURL,chatURL). No token in status/ready stdout. App reads private admin-token + admin.json directly. Trust builder owns separate persistent trust state.
Admin binds ONLY 127.0.0.1 ephemeral port, separate server from chat. POST /admin/v1 with Authorization Bearer admin-token and JSON {operation,...}. Reject Origin (browser CSRF), phone tokens never accepted, <=64KiB body, errors {ok:false,error:{code,message}}, success {ok:true,result}. GET forbidden.
Operations:
- status -> {phase:starting|ready|failed,instanceId,contractVersion,chatURL,sessionId,provider,model,cwd,recovery?}
- providers -> {providers:[{id,name,oauth,apiKey,configured,models:[{id,name}]}]}
- auth.start {provider,type:oauth|api_key} -> {attemptId,expiresAt}; starts async SDK flow (API-key and OAuth use same interaction)
- auth.status {attemptId} -> {attemptId,provider,type,status:pending|complete|canceled|expired|failed,expiresAt,events:[],prompt?:{id,type,message,placeholder?,options?},error?}; events contain SDK auth URL/device code only, never credential values; never persisted/logged.
- auth.respond {attemptId,promptId,value} -> {}; stale prompt rejected. Credentials input only on admin plane.
- auth.cancel {attemptId} -> {}; abort/cancel pending prompt. Provider change/new attempt supersedes active attempt, generation checked inside credential lock.
- model.select {provider,model} -> {}; serialized idle-only rpc set_model then durable state. Cancel active login on provider change. Credentials update causes explicit idle-only runtime refresh/restart with native session restored; never replays prompts.
- runtime.configure {cwd} -> {}; validated idle-only, requires controlled child restart preserving selected session.
- runtime.restart -> {}; idle-only owned RPC replacement preserving native session/provider selection, no replay. runtime.stop -> {}; graceful owned bridge exit (app waits Process).
- pairing.create / pairing.status / devices.list / devices.revoke delegated to trust builder adapter; enrollment payload returned only via local admin, not static secret page.
Desktop chat uses independently provisioned desktop credential from trust builder, NOT admin-token. Need trust adapter exact registration signatures and provisioning contract from ag-emcc2y4e.

## r1 clarification for native integration
POINT_GUARD_CLIENT_ID required native persistent UUID. Two local ephemeral listeners: admin HTTP /admin/v1 (same local bootstrap token), native chat ws /v1 via serve(...localAdmin:{token,clientId}); remote WSS only enabled explicitly when PI_MOBILE_HOST provided, otherwise desktop local setup fully works without Tailscale. Readiness admin.json additionally {hostId,certificateSHA256,nativeChatURL,chatURL:null|string}. token never in descriptor. startup failure file error.json {schemaVersion:1,code,message,recovery} contains sanitized actionable error, no SDK raw text/secrets. No readiness stdout required.
runtime.configure and runtime.restart return {restartRequired:true}; native owner gracefully stops current owned process then relaunches, no adoption of PID from prior descriptor. auth.status complete includes restartRequired:true. auth completion itself does not forcibly interrupt native Pi. App offers apply/restart once idle; coordinator preserves pending receipts. model.select uses RPC set_model {provider,modelId:model} and durable state idle-only. auth.start supports provider-owned api_key prompts, no separate raw key endpoint needed (auth.respond carries secret). Default cwd user home; runtime.json selected cwd takes precedence over default, explicit PI_MOBILE_CWD overrides stored.

## r2 reconciliation: distinct local chat capability
Trust adapter uses localClient (not localAdmin). Provision desktop-chat-token separately from admin-token, both private 0600; descriptor includes desktopClientId, persisted in runtime.json (POINT_GUARD_CLIENT_ID initial seed optional; stored value authoritative). Native reads desktop-chat-token solely for nativeChatURL. openTrustStore({directory:STATE_DIR/trust}) supplies hostId/certificateSHA256/tls/close. Remote serve({host,port,trust,tls:trust.tls}), local chat serve({host:'127.0.0.1',port:0,localClient:{token:desktop-chat-token,clientId}}). Separate admin server authenticates only admin-token.

## Finding: authenticated host/device enrollment proposal PRPG-TRUST-1
- Agent: ag-emcc2y4e / wt-c614sre7; baseline c0d14120ef0f62d7a8f7790bf022e7ebe7b2d856
- Spec: docs/architecture/ipc-api.md IPC-004; docs/architecture/storage.md; docs/product/recovery-resilience.md; mobile contract
- Type: research-note + decision-proposal
- The unreleased prototype authenticated one shared token and caller-selected client ID, and embedded that durable token in its QR. Neither authenticated host identity. The first-release contract uses pinned, one-time enrollment while keeping both chat wire and enrollment QR at version 1; the prototype payload is superseded without a compatibility layer.
- Research: Node HTTPS accepts TLS cert/key (https://nodejs.org/api/https.html); Apple Keychain AfterFirstUnlockThisDeviceOnly survives updates/restarts after unlock, does not migrate to fresh devices (https://developer.apple.com/documentation/security/ksecattraccessibleafterfirstunlockthisdeviceonly). URLSession server-trust delegate can pin SHA256 leaf DER before releasing credentials. Prefer pinned TLS to unsigned host IDs/challenge on plaintext ws, which would leave bearer theft/relay hazards.

Exact proposal (awaiting coordinator reconciliation):
1. trust.js exports openTrustStore({directory,tls,now}), returning durable hostId (UUID) and certificateSHA256 (64 lowercase hex), admin-only methods createEnrollment({endpoint,ttlSeconds=120}), listDevices(), revokeDevice(deviceId), rotateDevice(deviceId), revokeEnrollment(enrollmentId); device authorize(credential) => {deviceId,clientId} or null; enroll({enrollmentToken,name}) => credential response. Enrollment tokens 32 random bytes base64url, bounded 120s/default/max300; memory-only pending enrollment (restart invalidates), atomic single-use including concurrent requests. Device credentials random 32 bytes, only SHA256 hashes persisted. All revokes/rotations serialized durable write before success + emit revoke to terminate sockets immediately. Device ID equals authoritative chat client ID; client header/message must match it. Rotation invalidates only selected device, preserves ID and unrelated devices; deliberate re-pair required for rotated phone, replacement credential not returned to admin UI.
2. Durable store schema v1, private directory 0700, atomic file 0600; host ID + certificate fingerprint + devices hashes. Malformed/unknown version/identity missing or changed fail closed, never rewrite/regenerate existing trust. Old shared token never accepted as phone credential. No automatic migration of legacy phone trust; scan fresh QR. Existing token becomes local-admin only through separate listener config, never QR/mobile URL.
3. QR: {type:'pi-mobile-pairing',version:1,endpoint:'wss://100.x.x.x:8787/v1',hostId,certificateSHA256,enrollmentToken,expiresAt:<epoch milliseconds>}. HTTPS POST /pair/enroll same host/port, body {version:1,enrollmentToken,name}; response {version:1,hostId,deviceId,clientId,credential}. HTTPS GET /pair/verify with device bearer => {version:1,hostId,deviceId,clientId}; 401 means revoke/lost trust, stop retries; pin mismatch stops retries before credential transmitted. Reject Origin, redirect, excessive request body; no admin on remote listener. Existing /v1 requires per-device auth.
4. Native admin transport separate loopback server (or private UDS), configured local token + native client ID. Runtime owns API wrapper, trust store methods above supplied to it; phone listener cannot authorize token. Local chat preserves macOS existing persistent client ID; remote header cannot choose it. Native API should expose pairing.create/list/revoke/rotate and use returned exact QR payload, not compose shared-token QR.
5. TLS provisioning: stable self-signed cert/key generated once with macOS /usr/bin/openssl (no new external dependency) in private managed state; reused on restart/update. trust.js can own certificate helper if coordinator agrees. Certificate fingerprint pinned from QR, host UUID authenticated through TLS response; identity/certificate rotation requires deliberate phone re-pair. Remote plaintext listener refused. Local fixture-only legacy transport must be explicit, loopback-only, never production default.
6. iOS saved record Keychain contains version/endpoint/hostId/cert pin/deviceId/clientId/credential, atomic update not delete+add. Legacy saved strings ignored and trigger scan; preserve draft/recovery data. Pinned URLSession for both enrollment/verify/chat; never shared session. Launch/foreground/NWPathMonitor reachable changes reset bounded reconnect budget (5 retries at 1,2,4,8,16 seconds). Verify before chat, fatal auth/pin errors require scan; no prompt resubmission. Disconnect respects user choice; background detaches view only.

Requests to runtime: open trust store/TLS before serve; serve(controller,{host,port,trust,tls,localAdmin?:{token,clientId}}) [localAdmin accepted only loopback separate instance]; pass trust methods to runtime admin handler; remove automatic shared-token QR and emit QR only from enrollment creation; use managed-state paths. Need coordinator agreement on TLS helper and separate native chat listener placement. No consequential contract mutations until recorded reconciliation/spec advancement.
Proofs: expired/reused/concurrent enrollment, revoked pending QR, restart invalidation, durable device authorization, corrupt/legacy state fail-closed, header/message spoofing, socket revoke/rotate, unrelated desktop+phone queue/Stop preserved, TLS pin mismatch before auth, Keychain/reconnect/no replay focused standalone proofs + composed nonpublishing CI app builds.

## Reconciled integration signatures (PRPG-TRUST-1)
Coordinator adopts pinned TLS on remote WSS, separate loopback native chat/admin. Trust owns TLS provisioning helper. Implementation waits coordinator spec SHA.
- await openTrustStore({directory:<STATE_DIR>/trust,now?:()=>epochMs}) => EventEmitter trust; exposes .hostId, .certificateSHA256, .tls={cert,key}; owns trust.json + identity-cert.pem + identity-key.pem. Generates TLS only for entirely absent identity; existing incomplete/corrupt/expired cert/key fails closed with actionable error. Uses /usr/bin/openssl only. await trust.close() releases private writer lock.
- serve(controller,{host,port,trust,tls:trust.tls}) => remote listener mandatory TLS; /v1 device auth, /pair/enroll and /pair/verify HTTPS.
- serve(controller,{host:'127.0.0.1',port:0,localAdmin:{token:<desktop-chat-token>,clientId:<persisted desktop client ID>}}) => loopback native CHAT only. Option name retained for proposed compatibility but token is desktop-chat token, never admin-token. Prefer rename localClient in final code to avoid confusion: serve(...,{localAdmin:{token,clientId}}). No remote trust/no pairing routes in localClient mode. Runtime provisions private desktop-chat-token (ensureToken) + desktop clientId UUID into admin.json. Credential header and message must equal server configured clientId. Runtime admin port separately validates its admin-token.
- trust.createEnrollment({endpoint,ttlSeconds?}) => {enrollmentId,payload:<JSON QR string>,expiresAt}; endpoint explicit remote wss /v1.
- trust.enrollmentStatus(enrollmentId) => {enrollmentId,status:'pending'|'consumed'|'expired'|'revoked'|'unknown',expiresAt?}; restart yields unknown.
- trust.listDevices() => [{deviceId,clientId,name,createdAt,revokedAt?}]; no hash/credentials. trust.revokeDevice(deviceId) => Promise<void>. trust.rotateDevice(deviceId) => Promise<void> invalidates selected device, deliberate re-enrollment; unrelated devices survive.
- trust.revokeEnrollment(enrollmentId) => void. trust.authorize(credential) => {deviceId,clientId} | null.
- trust.enroll({enrollmentToken,name}) => Promise<{version:1,hostId,deviceId,clientId,credential}>. Event 'revoked', deviceId, synchronously after persisted mutation.
Admin adapter runtime handler: pairing.create {endpoint,ttlSeconds?} -> trust.createEnrollment; pairing.status {enrollmentId} -> enrollmentStatus; devices.list -> {devices:trust.listDevices()}; devices.revoke {deviceId} -> revokeDevice; devices.rotate {deviceId} -> rotateDevice; pairing.revoke {enrollmentId}->revokeEnrollment. No these operations exposed by remote listener. Native UI reads public QR string only from returned payload; devices have no admin capabilities.

## Edge Cases

Refuse insecure/malformed/unknown-schema existing state instead of resetting identity or credentials. No descriptor adoption unless it belongs to the exact owned child and startup instance. App update/restart cannot erase trust, auth, session selection or local recovery receipts. No credential in public readiness stdout. Local fixture shared-token transport is explicit loopback test-only; never production fallback. Reconnect never resends prompts. Certificate expiry or missing tailnet is recoverable UI failure, not silent pin rotation or public bind.

### Reconciled r2 producer integration signatures

- openTrustStore({directory:STATE_DIR/trust}) provisions/loads stable TLS and returns hostId, certificateSHA256, tls, close and trust methods.
- Remote serve(controller,{host,port,trust,tls:trust.tls}); local serve(controller,{host:'127.0.0.1',port:0,localAdmin:{token,clientId}}). Local token is desktop-chat-token; admin HTTP uses admin-token only.
- trust.createEnrollment({endpoint,ttlSeconds?}) returns {enrollmentId,payload:<exact JSON QR string>,expiresAt}. trust.enrollmentStatus(enrollmentId) returns status pending/consumed/expired/revoked/unknown.
- devices.list returns {devices:trust.listDevices()}; devices.revoke/rotate take deviceId. pairing.revoke takes enrollmentId. Rotation invalidates selected credential and requires deliberate re-enrollment; no replacement credential exposed to admin UI.
- admin.json includes desktopClientId, nativeChatURL, optional chatURL, hostId, certificateSHA256. runtime.json preserves desktopClientId. No Tailscale is required to start desktop chat/setup; remote must use explicit tailnet address.
- runtime.configure/restart and auth.status complete return restartRequired:true. Native process owner applies only idle stop/await/relaunch. Sanitized error.json exposes code/message/recovery, never SDK raw credential-bearing errors.

Earlier proposal wording about same bootstrap token/in-process replacement is superseded by these r2 signatures.

The network option name localAdmin is retained by producer agreement but grants local chat only; it provides no HTTP admin routes. pairing.create refuses an endpoint different from actual configured remote chatURL; native uses descriptor chatURL. certificateSHA256 is the sole pin wire field, with no certPin alias.

### Native setup-only persistence edge case

**Researched: 2026-10-09**, pinned Pi 1.1.0 producer artifact proof. The SDK intentionally defers creating an empty new session file until conversation and can report unknown/unknown as a no-credential placeholder model. Persisting that placeholder then restoring via set_model causes a clean-install second launch to fail. Only catalog-valid selections are durable provider/model choices; an invalid old selection shows a setup notice. The managed adapter explicitly persists the native empty session tree with a small version-pinned SessionManager adapter, without fake user/assistant messages or changing nonmanaged Pi behavior. Prove native empty-tree roundtrip and relocated restart before first prompt as well as after conversation; preserve queue ownership and never replay prompts.

Actionable startup errors are sanitized code/message/recovery with schemaVersion, exact pid and instanceId; the native app displays only the matching owned launch. No old error descriptor may identify a current recovery action.

### Enrollment commit and uncertain cleanup

Independent real TLS/model tests show that a UI generation check after a detached Keychain write does not prevent canceled pairing A from overwriting newer B. Cancellation/generation advancement and the Keychain commit share one ordered utility writer; UI IO stays asynchronous. Prove delayed A save, cancel/background, subsequent B enrollment and final stored identity equal to connected B.

Native setup retains pending auth/enrollment cleanup IDs until an explicit successful cancel/revoke receipt; transport failure reports uncertainty and exposes retry, without replacing newer active UI. A revoked QR may remain valid until expiry if revocation cannot be confirmed. New QR creation requires confirmed cleanup of a known old code. Missing saved cwd is recoverable via an explicit owner-selected folder before readiness; next owned launch validates durable schema/private state before accepting the override. Corrupt/unknown trust/runtime state is never edited by folder recovery.

### Remote reachability and owned cleanup lifetime

An explicitly selected tailnet address can disappear between discovery and bind. Only EADDRNOTAVAIL, ENETUNREACH or EHOSTUNREACH on the optional remote listener permit loopback admin/native chat startup with chatURL:null. Status may include remoteRecovery:{code,message,recovery}, using static sanitized text that tells the owner to reconnect Tailscale and restart the owned service. Native setup displays this recovery while local provider/chat remain usable. pairing.create cannot mint a code without an actual remote listener. EADDRINUSE, identity/certificate/private-state corruption and other errors remain fail-closed; no public bind or identity replacement fallback.

Pending OAuth/enrollment cleanup belongs to the exact app-owned launch. Only confirmed exit of that owned child invalidates the SDK attempts and memory-only enrollment IDs; foreign exit notifications never discard cleanup. Fence late start/create/cleanup responses by launch lifetime as well as UI generation, so an exited runtime cannot resurrect IDs or block a new runtime. A folder override selected before readiness is one-shot: clear it after owned readiness, and clear it after an accepted ready runtime.configure so durable cwd remains authoritative on the next launch.

### PR192 recovery and native termination reconciliation

Crash recovery uses kernel-managed advisory exclusive locks on permanent private regular-file inodes, held by an app-owned packaged helper until explicit close or owner EOF. Never unlink a marker because its PID is absent/dead; private-file validation and kernel acquisition, not metadata, establish ownership. Before durable mutations assert the holder is still owned/live; unexpected holder loss invalidates setup/trust writes and closes managed service/device sockets. Preserve existing host identity, device credentials and runtime state when reacquiring after a crash. Concurrent startup, symlink/public/foreign files and unknown schemas fail closed. Runtime owns the packaged helper/API/CI; trust consumes that API under exact coordinated signatures.

A rejected idle-only stop during Quit must offer visible native feedback with Cancel and an explicit Stop Pi and Quit choice. Only that deliberate choice may request SIGTERM of the exact app-owned child during busy work; await owned child/RPC/helper exit before replying termination may proceed. Timeout keeps termination/update paused with visible error; no unconditional terminate-true fallback. Pi-session/draft recovery persists; uncertain prompts never replay.

Tailnet discovery must reject CGNAT addresses on Wi-Fi/Ethernet and unrelated interfaces. Match a live numeric utun interface address to the installed Tailscale client's own `ip -4` output, with bounded read-only subprocess discovery off the UI actor; no arbitrary inherited PATH or installing CLI dependency. Missing/ambiguous/unresponsive Tailscale leaves phone unavailable and loopback setup/chat usable. [Official Tailscale CLI](https://tailscale.com/docs/reference/tailscale-cli) documents built-in CLI and environment selection; [macOS variants](https://tailscale.com/docs/concepts/macos-variants) documents bundled GUI/CLI variants. Reachability never grants application authorization.

Advisory helper contract: manifest.paths.lockHelper is ../../Helpers/point-guard-lock; native passes explicit POINT_GUARD_LOCK_HELPER_PATH. Runtime-only separate pu-cli binary pu-point-guard-lock avoids shared main.rs. bridge/lock-helper.js acquirePrivateLock({file,helperPath:env.POINT_GUARD_LOCK_HELPER_PATH,onLost}) returns held/assertHeld/release/holderPid/identity/lost. Trust openTrustStore accepts optional helperPath and emits lockLost after fencing mutations/auth.

The Node owner opens and RETAINS the private nofollow lock file descriptor; helper inherits that same open-file description at stdio fd3 and acquires flock on it. Helper never explicitly LOCK_UN. Kernel ownership therefore survives unexpected helper exit while Node still holds its descriptor: queued/submitted writes drain before parent descriptor closes, preventing another writer from reacquiring during a stale in-flight rename. Observed helper loss still fences new operations and closes sockets; uncertain IO is never replayed. Parent crash closes its descriptor and helper stdin; helper exits and last descriptor close releases ownership. Other child processes must not inherit the retained lock descriptor. Do not accept a helper-only descriptor or documented concurrent-commit window as an equivalent implementation. Prove helper crash plus delayed owner IO excludes a second writer until drained release, and prove bridge crash permits same-inode restart preserving trust/state.

Platform research: [Apple flock(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html) confirms dup/fork descriptors share one advisory lock and explicit unlock in the child removes the parent's protection. Retaining the parent descriptor and never sending LOCK_UN is therefore part of the contract, verified by actual macOS helper-loss/exclusion tests rather than inferred from PID metadata.

### Simulator Keychain proof and signing boundary

The exact4f50765 CI host built successfully but failed before XCTest launch: exported Apple crash diagnostics report SIGKILL, CODESIGNING/Taskgated Invalid Signature. Xcode's linker embedded its simulator application identifier in `__TEXT,__entitlements`; adding iOS access entitlements afterwards to the host macOS ad-hoc signature made that host unlaunchable. This is a CI signing defect, not proof that Keychain is working.

Provide the private Keychain access group through a simulator-only Xcode entitlement input, expanded with Xcode's application prefix and bundle identifier. Keep device/production signing separate. Verification is read-only: inspect every simulator executable slice's embedded identity/group and its normal ad-hoc signature, never repair by attaching restricted iOS rights to the host signature. The real save/read/delete/migration XCTest must still run and pass; parser/signature checks cannot replace it. Preserve failure diagnostics and exact candidate attribution. Apple documents entitlement expansion/diagnosis in [TN2415](https://developer.apple.com/library/archive/technotes/tn2415/_index.html); the simulator-section and failure evidence here comes from actual Apple Xcode build/crash output, not an assumption about Developer ID/notarization.

### Dispatched controller work is part of the ownership drain

Independent GitHub Codex review at4f50765 found that an unused `controller.accepting` assignment did not fence requests and shutdown did not await the controller serialization queue. The retained-FD barrier therefore must cover accepted controller requests and administrative exclusive actions as well as store writes. Fence new requests, queued actions and RPC writes immediately on shutdown/loss; close listeners and cancel the owned RPC transport to unblock pending work; await all accepted controller work before closing trust/runtime holders. Concurrent shutdown callers share the same drain promise. Already uncertain prompts are never retried. A disposable real-runtime delayed-action/helper-kill test must show the second writer remains excluded until that action drains. Bundled helper/CLI crate and workspace dependency changes must route through architecture/signing/relocated package proofs, not only macOS compilation.
