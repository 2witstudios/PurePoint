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
| `npm run e2e`                                | Placeholder until WIN-5.7 adds end-to-end tests.                                   |
| `npm run tauri build -- --debug --no-bundle` | Build the debug app at `src-tauri/target/debug/purepoint-windows.exe`, as CI does. |
| `npm run tauri build`                        | Build the release app and its NSIS and MSI installers.                             |

Backend checks run from `apps/purepoint-windows/src-tauri`: `cargo fmt -- --check`, `cargo clippy --all-targets` and `cargo test`.

CI runs all of these in `.github/workflows/windows-app.yml`.

## Layout

- `ui/src/{protocol,model,state,services,views,theme}`: front-end folders. State lives in Zustand stores (DEC-14).
- `src-tauri/`: its own Cargo workspace with its own `Cargo.lock`, excluded from the root workspace (DEC-7). It depends on `crates/pu-core` by path. Each backend module names the leaf that fills it in.
- The window has no native decorations (DEC-10). `ui/src/views/TitleBar.tsx` is a placeholder drag strip with window controls until WIN-7.1 builds the real chrome.

## Toolchain notes

- Tauri 2.12 needs Rust 1.90, so the app pins `tauri ~2.11`, `tauri-build ~2.6`, `@tauri-apps/api ~2.11` and `@tauri-apps/cli ~2.11` to stay on the repository toolchain (1.89). `rust-version = "1.89"` in `src-tauri/Cargo.toml` makes Cargo's MSRV-aware resolver keep the rest of `Cargo.lock` on versions that build with it. Lift the pins when `rust-toolchain.toml` moves to 1.90.
- `webview2-com` and `windows-core` match the versions Tauri 2.11 links, so the COM types `src-tauri/src/webview.rs` uses are the ones Tauri hands out.
- typescript-eslint supports TypeScript below 6.1, so TypeScript stays on 6.0.
- Release builds turn off WebView2's browser accelerator keys (reload, print, find, DevTools), its default context menu and DevTools; debug builds keep them.
