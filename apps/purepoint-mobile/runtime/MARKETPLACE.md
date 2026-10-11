# Published package qualification

Snapshot: 2026-10-10. Exact Pi 1.1.0 and checksum-verified Node 22.23.3 Darwin arm64, not repository main or ambient Node 26. Published tarballs were verified against recorded npm SHA512 integrity. Extracted artifacts and script-free, peer-omitted disposable dependency installs were used; no personal Pi settings, accounts or app permissions were changed. Nothing from these candidates is shipped or activated by this implementation.

| Published artifact | Exact runtime result | Remaining setup/proof |
| --- | --- | --- |
| [pi-web-access 0.38.0](https://www.npmjs.com/package/pi-web-access/v/0.38.0) | Native package resolution, RPC startup, registered/active web tools, command output and teardown PASS | Real fetch/search behavior, output/artifact/cancel handling and provider credentials/configuration NOT RUN. Strong first web candidate, not full acceptance. |
| [pi-playwright 0.1.2](https://www.npmjs.com/package/pi-playwright/v/0.1.2) | Native progressive playwright-browser skill discovery and RPC startup/teardown PASS | Package is a CLI skill, no browser tool extension. Restricted managed PATH lacks playwright CLI; reproducible browser binaries/CLI/helper wiring, browser execution and permissions NOT RUN. |
| [pi-agent-browser-native 0.9.4](https://www.npmjs.com/package/pi-agent-browser-native/v/0.9.4) | Published engine requires Node >=24.21.0; rejected for bundled Node22.23.3 | Not executed; changing runtime version is separate scope. |
| [@amaster.ai/pi-computer-use 0.1.24](https://www.npmjs.com/package/@amaster.ai/pi-computer-use/v/0.1.24) | Native host module mapping, package resolution, RPC startup, desktop tool registration, command output and teardown PASS | No CuaDriver launched. Signed driver/host packaging, macOS Accessibility/Screen Recording grants and real actions/output/cancel remain NOT RUN. Packed code gates launch/high-risk actions via ctx.hasUI + ctx.ui.confirm (RPC supports it); human denial/cancellation and host grants need integration validation. Its default confirmation policy was not installed. |

`marketplace-qualification.json` records exact versions/integrities and observed runtime catalogs. Native MCP, OAuth/resources, codemode, tool_search, skills and packages remain supplied by Pi. No third-party MCP replacement or parallel delegation system is needed. pi-memory and pi-persona are not included: automatic exit model calls/background extraction and emotional machinery are outside owner intent. pi-subagents duplicates PurePoint delegation.

Reproduce in disposable directories only:

1. `npm pack` the exact four versions above into an artifact directory and verify SHA512 against the JSON snapshot.
2. Safely extract each under `<artifacts>/<name with / replaced by ->/package`. Inspect before loading; local packages execute code with host permissions.
3. For web-access and computer-use, create local lockfiles and `npm ci --ignore-scripts --omit=dev --omit=peer --no-audit --no-fund`. Do not copy Pi peers into those package trees. This diagnostic snapshot does not promise future floating dependencies; bundling would require committing complete dependency locks, licenses and host proofs first.
4. `<checksum-verified Node22.23.3> runtime/qualify-marketplace.mjs <absolute mobile root> <artifacts> <output.json>`.

The qualifier registers a local deterministic provider and inspection command, loads candidate packages via Pi's native temporary `--extension` package resolution, and shuts down only its own RPC children. It makes no real computer/web actions. Core regression RPC tests separately prove dialogs, native shell/files, cancellation, compaction and prompt refresh. Genuine app executables, both architectures, signing and permission/device acceptance remain separate human/package acceptance obligations.
