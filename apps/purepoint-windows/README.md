# PurePoint for Windows

The Windows desktop app: a Tauri 2 shell (`src-tauri/`, Rust) around a React + TypeScript front end (`ui/`). It is a client of the PurePoint daemon (`pu-engine`) and mirrors the macOS app in `apps/purepoint-macos`; `docs/reference/windows-parity-matrix.md` maps each macOS file to its place here.

## Prerequisites

- Windows 11 (DEC-12) with the WebView2 runtime (preinstalled on Windows 11).
- Node 22.13 or newer (CI uses 22.23.3).
- Rust from the repository's `rust-toolchain.toml` (1.89) with the MSVC toolchain and the Visual Studio C++ build tools.

## Commands

Run from `apps/purepoint-windows`:

| Command                                      | What it does                                                                       |
| -------------------------------------------- | ---------------------------------------------------------------------------------- |
| `npm ci`                                     | Install the front-end and Tauri CLI dependencies from `package-lock.json`.         |
| `npm run tauri dev`                          | Run the app with the Vite dev server (port 1420) and hot reload.                   |
| `npm run dev`                                | Run only the Vite dev server, for working on the UI in a browser.                  |
| `npm run build`                              | Type-check and build the front end into `dist/`.                                   |
| `npm run typecheck`                          | Type-check the front end (TypeScript strict).                                      |
| `npm run lint`                               | ESLint, then a Prettier check. `npm run format` rewrites files with Prettier.      |
| `npm test`                                   | Vitest with jsdom and Testing Library.                                             |
| `npm run gen:protocol`                       | Regenerate the daemon protocol types in `ui/src/protocol/generated`.               |
| `npm run e2e`                                | End-to-end tests against a real daemon (see below).                                |
| `npm run tauri build -- --debug --no-bundle` | Build the debug app at `src-tauri/target/debug/purepoint-windows.exe`, as CI does. |
| `npm run bundle`                             | Build the release NSIS installer with the bundled CLI and Point Guard runtime.     |

Backend checks run from `apps/purepoint-windows/src-tauri`: `cargo fmt -- --check`, `cargo clippy --all-targets` and `cargo test`.

CI runs all of these in `.github/workflows/windows-app.yml`.

The installer that workflow uploads as an artifact is a development build: it carries `tauri.conf.json`'s version (`0.1.0`), not a release version. Only `release.yml` stamps a release's version.

## Releases

A `v<version>` tag runs `.github/workflows/release.yml`. Its Windows jobs (WIN-10.4, DIST-009) publish to the same GitHub release as the Mac and Linux builds:

| Asset                                                                                 | What it is                                                                               |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `purepoint-<version>-<x86_64\|aarch64>-pc-windows-msvc.zip` (+ `.sha256`, `.minisig`) | `pu.exe` and `pu-engine.exe`, which `scripts/install.ps1` installs                       |
| `PurePoint_<version>_<x64\|arm64>-setup.exe` (+ `.sha256`, `.minisig`)                | the NSIS installer                                                                       |
| `latest-windows.json` on the fixed `appcast` release                                  | the updater feed for both architectures, the only Windows asset replaced on each release |

- **`windows`** (no secrets, read-only token) runs `node scripts/build-release.mjs --arch <x64|arm64> --version <tag>` for both architectures on `windows-latest`. arm64 is cross-compiled to `aarch64-pc-windows-msvc` with the runner's MSVC ARM64 tools, not built on an arm64 runner, so the arm64 binaries are never run in CI. `--version` stamps the tag into the app, into `pu` (`PU_RELEASE_VERSION`) and into the Point Guard manifest's `puVersion`. The build job zips the CLI and installs the x64 zip with `install.ps1 -Archive` into a temp directory as a smoke test. The updater public key comes from the repository variable `TAURI_UPDATER_PUBKEY`.
- **`sign-windows`** (the `release` environment) checks the builds' SHA-256 and then signs, in this order. Each signature is skipped with a warning until its secret exists:
  1. Authenticode with signtool when the `WINDOWS_CERTIFICATE_PFX` (+ `WINDOWS_CERTIFICATE_PASSWORD`) secret is set (WIN-10.6). It signs the zipped binaries and the setup exes. The app exe and the binaries inside the installer stay unsigned until signing moves into the bundle step.
  2. minisign, with `RELEASE_MINISIGN_KEY`, the Linux archives' key, through `.github/scripts/minisign.sh`. A key that doesn't match the one embedded in `install.sh` and `install.ps1` fails the job.
  3. The updater signature, with `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`), through `scripts/sign-release-feed.mjs`. It signs both installers, checks each signature against `TAURI_UPDATER_PUBKEY` and the version, and writes one `latest-windows.json`.
- **`attest`** records build provenance and the SBOM attestations for the zips and installers. **`publish`** uploads them and replaces `latest-windows.json` on the `appcast` release.

To try the Windows release build without a tag, run the workflow by hand on a branch (`gh workflow run release.yml --ref <branch>`). That dry run builds version `0.0.0` for both architectures and runs `sign-windows` with no environment and no secrets, so every signature is skipped and the unsigned zips and installers are left as workflow artifacts. The macOS and Linux jobs, `attest` and `publish` run only for a tag push, so a dry run publishes nothing.

`scripts/install.ps1` (REQ-WINAPP-038) is the Windows `install.sh`. Run it as `irm …/install.ps1 | iex`, or save it and run it with `-Version`, `-InstallDir`, `-Archive` or `-AddToPath`. It downloads the zip and its `.sha256` and `.minisig`, then verifies the release signature with `minisign` if it is on `PATH`. Otherwise it uses its built-in verifier: Ed25519 and BLAKE2b-512 compiled with `Add-Type`, because .NET has neither and OpenSSL isn't on a stock Windows `PATH`. When `gh` is signed in, it also runs `gh attestation verify`. At least one check must pass and no check that runs may fail. `PU_INSTALL_SKIP_VERIFY=1` only bypasses a missing check, never a failed one. Before the install, `Unblock-File` removes the Mark of the Web. A running `pu-engine.exe` is renamed aside. The script changes `PATH` only with `-AddToPath`. It runs on PowerShell 7 and Windows PowerShell 5.1. Its tests are in `.github/scripts/test_install_ps1.py` (Windows only): a local HTTP server and throwaway keys, signed both by an independent Python signer and by `minisign.sh`.

## End-to-end tests

`npm run e2e` (`ui/e2e/run.mjs`) drives the real app with WebdriverIO through [`tauri-driver`](https://v2.tauri.app/develop/tests/webdriver/) and `msedgedriver`:

1. `ui/e2e/setup-drivers.ps1` downloads the `msedgedriver` matching the installed WebView2 runtime and runs `cargo install tauri-driver --locked` into `.e2e-tools/` (gitignored; reused on later runs).
2. It builds `pu.exe` and `pu-engine.exe` at the repository root and the debug app (`tauri build --debug --no-bundle`) when they are missing (`PU_E2E_REBUILD=1` rebuilds the app), then stages a copy of the app with `pu.exe`, `pu-engine.exe` and the plugin beside it, as the installer lays them out, in a temp run folder.
3. Each spec gets a fixture (`ui/e2e/fixtures/daemon.ts`): a temp `HOME`/`USERPROFILE` with a git repository, `pu init`, a `pu-engine.exe` on that HOME's socket and a `terminal` agent from `pu spawn --root --agent terminal`. tauri-driver, and so the app, start with that environment, so the app talks to the fixture daemon's pipe and its startup installer installs into the temp HOME. `PU_INSTALLER_SANDBOX=1` (honored by debug builds only) keeps the installer off the registry `PATH` and away from `claude`, and `WEBVIEW2_USER_DATA_FOLDER` keeps the WebView2 profile out of `%LOCALAPPDATA%`.
4. Every test saves a screenshot, and each spec its `pu-engine.log` and `tauri-driver.log`, to `ui/e2e/artifacts/` (gitignored), which CI uploads. Afterwards the run kills anything it started that is still alive and deletes the temp folder (`PU_E2E_KEEP=1` keeps it).

Specs live in `ui/e2e/specs/*.e2e.ts`; `npm run e2e -- --spec ui/e2e/specs/smoke.e2e.ts` runs one.

## Protocol types

`ui/src/protocol/generated/` holds TypeScript types for the daemon wire protocol, generated from `crates/pu-core` with [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Import them from `ui/src/protocol` (`import type { Request, Response } from '../protocol'`), which also exports `PROTOCOL_VERSION`. Never edit the generated files: change the Rust types and run `npm run gen:protocol`, then commit the result. CI regenerates them and fails if they differ from what is committed.

How it works:

- pu-core's optional `ts` feature derives `ts_rs::TS` on `Request`, `Response` and every type they reach, plus the `types` file formats. Without the feature pu-core does not depend on ts-rs.
- `npm run gen:protocol` (`scripts/gen-protocol.mjs`) runs `cargo test -p pu-core --features ts --lib ts_export::export_typescript_bindings` from the repository root with `PU_TS_EXPORT_DIR` set to `ui/src/protocol/generated`. That test (`crates/pu-core/src/ts_export.rs`) empties the folder, writes one file per type, `protocolVersion.ts`, an `index.ts` barrel and `samples.ts`; the script then formats the folder with Prettier. ESLint skips the folder.
- `samples.ts` is JSON pu-core parses and writes (minimal requests, the same requests re-serialized, and responses as the daemon sends them), typed as `Request[]` and `Response[]`. `npm run typecheck` checks it, so a type that drifts from the real JSON fails the build. `ui/src/protocol/protocol.test.ts` pins the cases the samples cannot show.

What the types mean:

- A field is optional (`?`) where the JSON may leave it out: in requests, every field with a serde default; in responses, fields the daemon skips when empty. Shared types (`GridCommand`, `ScheduleTriggerPayload`, `SwarmRosterEntryPayload`, `ChannelReference`) take the looser request form.
- 64-bit integers are `number`, since `JSON.parse` returns numbers; protocol values stay below 2^53.
- PTY bytes (`input.data`, `output.data`) are lowercase hex strings. Dates are RFC 3339 strings. `grid_layout.layout` is `JsonValue`.
- `update_agent_config.launch_args` takes a shell-style string or a list of args; `AgentConfig.launchArgs` is written as the string form.

## Layout

- `ui/src/{protocol,model,state,services,views,theme}`: front-end folders. State lives in Zustand stores (DEC-14).
- `ui/src/theme/`: design tokens ported from the macOS `Theme/` folder (`tokens.css`, named in `tokens.ts`), the light/dark/system theme store (`themeStore.ts`, sets `<html data-theme>`) and the SF Symbol to Lucide icon map (`icons.ts`, `<Icon name="gear" />`). `node ../../scripts/extract-swift-tokens.mjs` lists the Swift tokens; `tokens.test.ts` fails when one has no CSS counterpart.
- `src-tauri/`: its own Cargo workspace with its own `Cargo.lock`, excluded from the root workspace (DEC-7). It depends on `crates/pu-core` by path. Each backend module names the leaf that fills it in.
- The window has no native decorations (DEC-10). `ui/src/views/shell/` draws the shell (WIN-7.1): a transparent title bar band with an in-app menu bar (File, View, Navigation, Panes, Tabs, Help — the Mac's `AppCommands`, with accelerators from the key binding store) and Windows 11 caption buttons. The band reports its layout through `chrome_set_layout`; `src-tauri/src/chrome.rs` puts an invisible layered "caption sink" child window over the webview, cut to the band's empty space plus the maximize button, which answers `WM_NCHITTEST` with `HTCAPTION` / `HTMAXBUTTON` / the top resize edges (Snap Layouts, drag, double-click maximize, the window menu). Win32 refuses layered child windows to apps without the Windows 10 compatibility section, so `src-tauri/windows-app-manifest.xml` adds it to Tauri's default manifest.
- The window starts hidden and `src-tauri/src/window_state.rs` shows it at its last placement (`window-state.json` in the app data directory, saved on close; maximized state included).

## Toolchain notes

- Tauri 2.12 needs Rust 1.90, so the app pins `tauri ~2.11`, `tauri-build ~2.6`, `@tauri-apps/api ~2.11` and `@tauri-apps/cli ~2.11` to stay on the repository toolchain (1.89). `rust-version = "1.89"` in `src-tauri/Cargo.toml` makes Cargo's MSRV-aware resolver keep the rest of `Cargo.lock` on versions that build with it. Lift the pins when `rust-toolchain.toml` moves to 1.90.
- `webview2-com` and `windows-core` match the versions Tauri 2.11 links, so the COM types `src-tauri/src/webview.rs` uses are the ones Tauri hands out.
- typescript-eslint supports TypeScript below 6.1, so TypeScript stays on 6.0.
- Every build turns off WebView2's browser accelerator keys (F5/Ctrl+R reload, Ctrl+P print, Ctrl+F find, F12/Ctrl+Shift+I DevTools, Ctrl+N, Ctrl+W) and its zoom (Ctrl+Plus/Minus, Ctrl+wheel), so those keys reach the page and a terminal (REQ-WINAPP-065); the debug e2e build therefore checks the release key behaviour (`hotkeys.e2e.ts`). Under msedgedriver, Ctrl+Shift+I/J/C still open DevTools (its keys arrive through the debugger), so that spec checks those three for no reload, print, find or zoom only. Release builds also turn off the default context menu and DevTools; debug builds keep both (right-click › Inspect).

## Syntax highlighting

Code is highlighted with CodeMirror 6's highlighters, not Shiki (WIN-8.3): `ui/src/services/codeHighlight.ts` parses with the Lezer grammars from `@codemirror/lang-*` (and `@codemirror/legacy-modes` stream parsers for Swift, TOML, shell, Ruby, C#, Scala, Haskell and Lua), loading each grammar as its own chunk on first use, and maps the syntax tags to EditorTheme's categories (`PP_HIGHLIGHTER`, classes `pp-tok-*` styled from the `--pp-editor-*` tokens in `ui/src/views/diff/highlight.css`). Parsing is synchronous and takes a few milliseconds per 200-line review page, which keeps the virtualized diff free of long tasks. The editor (WIN-8.5) reuses the same pieces: `loadLanguage(language)` gives the CodeMirror `Language` for a `LanguageSupport`, and `syntaxHighlighting(PP_HIGHLIGHTER)` colours it, so diffs, inline code and the editor share one grammar and one palette. File names map to languages with `detectEditorLanguage` (`ui/src/model/editorLanguage.ts`, Swift `EditorLanguage.detect(from:)`).
