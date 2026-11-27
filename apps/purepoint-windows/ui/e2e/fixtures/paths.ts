// Where the end-to-end harness finds the workspace builds (WIN-5.7).

import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const appDir = fileURLToPath(new URL('../../../', import.meta.url));
const repoRoot = join(appDir, '..', '..');

export const workspace = {
  appDir,
  repoRoot,
  /** `cargo build -p pu-cli` at the repository root. */
  puExe: process.env.PU_E2E_PU ?? join(repoRoot, 'target', 'debug', 'pu.exe'),
  /** `cargo build -p pu-engine` at the repository root. */
  engineExe: process.env.PU_E2E_ENGINE ?? join(repoRoot, 'target', 'debug', 'pu-engine.exe'),
  /** The staged app ui/e2e/run.mjs prepares (a copy of the debug build plus its sidecars). */
  appExe:
    process.env.PU_E2E_APP ?? join(appDir, 'src-tauri', 'target', 'debug', 'purepoint-windows.exe'),
  artifactsDir: join(appDir, 'ui', 'e2e', 'artifacts'),
  toolsDir: join(appDir, '.e2e-tools'),
} as const;
