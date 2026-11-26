// Regenerates ui/src/protocol/generated from the pu-core protocol types (WIN-5.2).
// The `ts` feature's export test writes the ts-rs bindings; Prettier then formats them.
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(appDir, '..', '..');
const outDir = resolve(appDir, 'ui', 'src', 'protocol', 'generated');

function run(command, args, options) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(
  'cargo',
  [
    'test',
    '--locked',
    '-p',
    'pu-core',
    '--features',
    'ts',
    '--lib',
    'ts_export::export_typescript_bindings',
    '--',
    '--exact',
  ],
  { cwd: repoRoot, env: { ...process.env, PU_TS_EXPORT_DIR: outDir } },
);
run(
  process.execPath,
  [
    resolve(appDir, 'node_modules', 'prettier', 'bin', 'prettier.cjs'),
    '--write',
    '--log-level',
    'warn',
    outDir,
  ],
  {
    cwd: appDir,
  },
);
