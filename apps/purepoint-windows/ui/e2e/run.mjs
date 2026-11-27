// `npm run e2e` (WIN-5.7): fetch the drivers, build what is missing, stage the app, run the
// WebdriverIO specs, then make sure nothing the run started is left behind.
//
//   PU_E2E_REBUILD=1  rebuild the debug app even when it exists
//   PU_E2E_KEEP=1     keep the run's temp folder (temp HOMEs, staged app) for debugging

import { execFileSync, spawnSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = fileURLToPath(new URL('../../', import.meta.url));
const repoRoot = join(appDir, '..', '..');
const e2eDir = join(appDir, 'ui', 'e2e');
const toolsDir = join(appDir, '.e2e-tools');
const artifactsDir = join(e2eDir, 'artifacts');
const debugApp = join(appDir, 'src-tauri', 'target', 'debug', 'purepoint-windows.exe');
const puExe = join(repoRoot, 'target', 'debug', 'pu.exe');
const engineExe = join(repoRoot, 'target', 'debug', 'pu-engine.exe');
// The Point Guard spec's development runtime uses the repo's lock helper (a pu-cli binary).
const lockHelperExe = join(repoRoot, 'target', 'debug', 'pu-point-guard-lock.exe');
const plugin = join(repoRoot, 'crates', 'pu-cli', 'assets', 'plugin');

if (process.platform !== 'win32') {
  console.error('The end-to-end tests drive the Windows app and run on Windows only.');
  process.exit(1);
}

function step(title) {
  console.log(`\n== ${title}`);
}

function sh(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', shell: false, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} exited with ${result.status}`);
  }
}

/** Kill whatever this run started that is still alive: anything running from the run folder
 * or the tools folder, and any pu-engine serving a socket in the run folder. */
function killLeftovers(runRoot) {
  const script = `
$ErrorActionPreference = 'SilentlyContinue'
$roots = @($env:PU_E2E_KILL_ROOTS -split ';' | Where-Object { $_ })
$names = 'purepoint-windows.exe','pu-engine.exe','pu.exe','tauri-driver.exe','msedgedriver.exe'
$hits = Get-CimInstance Win32_Process | Where-Object {
  $p = $_
  ($names -contains $p.Name) -and ($roots | Where-Object {
    ($p.ExecutablePath -and $p.ExecutablePath.StartsWith($_, [StringComparison]::OrdinalIgnoreCase)) -or
    ($p.CommandLine -and $p.CommandLine.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0)
  })
}
foreach ($p in $hits) {
  Write-Output "killing leftover $($p.Name) ($($p.ProcessId))"
  & taskkill.exe /PID $p.ProcessId /T /F | Out-Null
}
`;
  try {
    const out = execFileSync(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script],
      {
        encoding: 'utf8',
        windowsHide: true,
        env: { ...process.env, PU_E2E_KILL_ROOTS: [runRoot, toolsDir].join(';') },
      },
    );
    if (out.trim()) console.log(out.trim());
  } catch (error) {
    console.warn('Could not check for leftover processes:', error.message);
  }
}

function removeTree(path) {
  for (let attempt = 0; attempt < 10 && existsSync(path); attempt++) {
    try {
      rmSync(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
    } catch {
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
    }
  }
  if (existsSync(path)) console.warn(`Could not remove ${path}`);
}

const started = Date.now();
// Long-name path: the daemon pipe name hashes the socket path (CI's temp is an 8.3 name).
const runRoot = realpathSync.native(mkdtempSync(join(tmpdir(), 'pu-e2e-')));
let exitCode = 1;
try {
  rmSync(artifactsDir, { recursive: true, force: true });
  mkdirSync(artifactsDir, { recursive: true });

  step('Drivers');
  sh('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-ExecutionPolicy',
    'Bypass',
    '-File',
    join(e2eDir, 'setup-drivers.ps1'),
    '-ToolsDir',
    toolsDir,
  ]);

  if (!existsSync(puExe) || !existsSync(engineExe) || !existsSync(lockHelperExe)) {
    step('Build pu and pu-engine');
    sh('cargo', ['build', '--locked', '-p', 'pu-cli', '-p', 'pu-engine'], { cwd: repoRoot });
  }

  // The debug app embeds the UI at build time, so a binary older than any UI or backend source
  // would test stale code: rebuild when anything under ui/src, src-tauri/src or the configs is newer.
  // `cargo build`/`test`/`clippy` in src-tauri write the same exe without the bundled UI (it then
  // loads the dev server at localhost:1420), so only a binary this script built — recorded in a
  // stamp beside it — is trusted.
  if (
    !existsSync(debugApp) ||
    process.env.PU_E2E_REBUILD === '1' ||
    !builtByUs(debugApp) ||
    appIsStale(debugApp)
  ) {
    step('Build the debug app');
    sh('npx', ['tauri', 'build', '--debug', '--no-bundle'], { cwd: appDir, shell: true });
    writeFileSync(stampFor(debugApp), String(statSync(debugApp).mtimeMs));
  }

  // Lay the app out as the installer bundles it (pu.exe and pu-engine.exe beside the exe, the
  // Claude plugin under pu-plugin\), in the run folder: the startup installer then has something
  // to install into the temp HOME, and the build folder never gets sidecars that a developer's
  // own `tauri dev` would install into their real profile.
  step('Stage the app');
  const staged = join(runRoot, 'app');
  mkdirSync(staged, { recursive: true });
  const appExe = join(staged, 'purepoint-windows.exe');
  copyFileSync(debugApp, appExe);
  copyFileSync(puExe, join(staged, 'pu.exe'));
  copyFileSync(engineExe, join(staged, 'pu-engine.exe'));
  if (existsSync(plugin)) cpSync(plugin, join(staged, 'pu-plugin'), { recursive: true });

  step('Run the specs');
  const wdio = spawnSync(
    'npx',
    ['wdio', 'run', join('ui', 'e2e', 'wdio.conf.ts'), ...process.argv.slice(2)],
    {
      cwd: appDir,
      stdio: 'inherit',
      shell: true,
      env: {
        ...process.env,
        PU_E2E_RUN_ROOT: runRoot,
        PU_E2E_APP: appExe,
        PU_E2E_PU: puExe,
        PU_E2E_ENGINE: engineExe,
      },
    },
  );
  if (wdio.error) throw wdio.error;
  exitCode = wdio.status ?? 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  exitCode = 1;
} finally {
  killLeftovers(runRoot);
  if (process.env.PU_E2E_KEEP === '1') console.log(`Kept ${runRoot}`);
  else removeTree(runRoot);
  console.log(
    `\ne2e finished in ${((Date.now() - started) / 1000).toFixed(1)} s (exit ${exitCode})`,
  );
}
process.exit(exitCode);

function stampFor(binary) {
  return `${binary}.e2e-stamp`;
}

/** True when `binary` is the one `tauri build --debug` produced here (not a later cargo build). */
function builtByUs(binary) {
  const stamp = stampFor(binary);
  return (
    existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === String(statSync(binary).mtimeMs)
  );
}

/** True when any app source is newer than the built debug app (mtime comparison). */
function appIsStale(binary) {
  const built = statSync(binary).mtimeMs;
  const roots = [join(appDir, 'ui', 'src'), join(appDir, 'src-tauri', 'src')];
  const files = [
    join(appDir, 'src-tauri', 'tauri.conf.json'),
    join(appDir, 'src-tauri', 'Cargo.toml'),
    join(appDir, 'package.json'),
  ];
  const newer = (path) => existsSync(path) && statSync(path).mtimeMs > built;
  if (files.some(newer)) return true;
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory() ? walk(path) : newer(path)) return true;
    }
    return false;
  };
  return roots.some((root) => existsSync(root) && walk(root));
}
