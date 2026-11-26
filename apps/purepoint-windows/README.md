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
| `npm run e2e`                                | Placeholder until WIN-5.7 adds end-to-end tests.                                   |
| `npm run tauri build -- --debug --no-bundle` | Build the debug app at `src-tauri/target/debug/purepoint-windows.exe`, as CI does. |
| `npm run tauri build`                        | Build the release app and its NSIS and MSI installers.                             |

Backend checks run from `apps/purepoint-windows/src-tauri`: `cargo fmt -- --check`, `cargo clippy --all-targets` and `cargo test`.

CI runs all of these in `.github/workflows/windows-app.yml`.

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
- `src-tauri/`: its own Cargo workspace with its own `Cargo.lock`, excluded from the root workspace (DEC-7). It depends on `crates/pu-core` by path. Each backend module names the leaf that fills it in.
- The window has no native decorations (DEC-10). `ui/src/views/TitleBar.tsx` is a placeholder drag strip with window controls until WIN-7.1 builds the real chrome.

## Toolchain notes

- Tauri 2.12 needs Rust 1.90, so the app pins `tauri ~2.11`, `tauri-build ~2.6`, `@tauri-apps/api ~2.11` and `@tauri-apps/cli ~2.11` to stay on the repository toolchain (1.89). `rust-version = "1.89"` in `src-tauri/Cargo.toml` makes Cargo's MSRV-aware resolver keep the rest of `Cargo.lock` on versions that build with it. Lift the pins when `rust-toolchain.toml` moves to 1.90.
- `webview2-com` and `windows-core` match the versions Tauri 2.11 links, so the COM types `src-tauri/src/webview.rs` uses are the ones Tauri hands out.
- typescript-eslint supports TypeScript below 6.1, so TypeScript stays on 6.0.
- Release builds turn off WebView2's browser accelerator keys (reload, print, find, DevTools), its default context menu and DevTools; debug builds keep them.
