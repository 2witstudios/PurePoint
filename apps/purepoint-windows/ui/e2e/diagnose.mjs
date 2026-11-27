// TEMPORARY CI diagnostics for "DevToolsActivePort file doesn't exist". Run after
// `PU_E2E_KEEP=1 npm run e2e -- --spec <one spec>` so a staged app exists in a pu-e2e-* folder.
import { spawn, execFileSync } from 'node:child_process';
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const appDir = fileURLToPath(new URL('../../', import.meta.url));
const toolsDir = join(appDir, '.e2e-tools');
const out = join(appDir, 'ui', 'e2e', 'artifacts', 'diag');
mkdirSync(out, { recursive: true });
const report = [];
const log = (...a) => {
  const line = a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x, null, 1))).join(' ');
  console.log(line);
  report.push(line);
};

const ps = (script) => {
  try {
    return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
      encoding: 'utf8',
      windowsHide: true,
    });
  } catch (e) {
    return `ps failed: ${e.message}\n${e.stdout ?? ''}${e.stderr ?? ''}`;
  }
};

const drivers = JSON.parse(readFileSync(join(toolsDir, 'drivers.json'), 'utf8').replace(/^﻿/, ''));
log('drivers', drivers);
const runs = readdirSync(tmpdir()).filter((n) => n.startsWith('pu-e2e-'));
log('run roots', runs);
const runRoot = join(tmpdir(), runs.at(-1));
const appExe = join(runRoot, 'app', 'purepoint-windows.exe');
log('app', appExe, existsSync(appExe));

log(
  'registry/session',
  ps(`
reg query "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" 2>&1
reg query "HKCU\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" 2>&1
reg query "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2" /s 2>&1
reg query "HKCU\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2" /s 2>&1
Get-ChildItem "\${env:ProgramFiles(x86)}\\Microsoft\\EdgeWebView\\Application" | Select-Object Name
Get-ChildItem "\${env:ProgramFiles(x86)}\\Microsoft\\Edge\\Application" -ErrorAction SilentlyContinue | Select-Object Name
whoami; query session 2>&1; [Environment]::UserInteractive
(Get-Process -Id $PID).SessionId
`),
);
log('env keys', Object.keys(process.env).sort().join(' '));
log('TEMP', process.env.TEMP, 'TMP', process.env.TMP, 'tmpdir', tmpdir());

function fixtureEnv(name, { userData = true } = {}) {
  const root = join(runRoot, `diag-${name}`);
  const home = join(root, 'home');
  mkdirSync(join(home, '.pu'), { recursive: true });
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^PU_/i.test(k)));
  Object.assign(env, {
    HOME: home,
    USERPROFILE: home,
    PU_INSTALLER_SANDBOX: '1',
    RUST_BACKTRACE: '1',
  });
  if (userData) env.WEBVIEW2_USER_DATA_FOLDER = join(root, 'webview2');
  return { root, env };
}

function procs() {
  return ps(
    `Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'msedgewebview2|purepoint|msedgedriver|WerFault' } | ForEach-Object { "$($_.ProcessId) $($_.ParentProcessId) $($_.Name) :: $($_.CommandLine)" }`,
  );
}

function listTree(dir, depth = 3, prefix = '') {
  if (!existsSync(dir)) return `${prefix}(missing ${dir})\n`;
  let s = '';
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    s += `${prefix}${e.name}${e.isDirectory() ? '/' : ` ${statSync(join(dir, e.name)).size}`}\n`;
    if (e.isDirectory() && depth > 0) s += listTree(join(dir, e.name), depth - 1, prefix + '  ');
  }
  return s;
}

function killTree(pid) {
  try {
    execFileSync('taskkill.exe', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
  } catch {}
}

// A: the app on its own, as msedgedriver would launch it.
{
  const { root, env } = fixtureEnv('direct');
  env.TAURI_WEBVIEW_AUTOMATION = 'true';
  env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333';
  const o = openSync(join(out, 'A-app-output.log'), 'w');
  const child = spawn(appExe, [], { env, stdio: ['ignore', o, o], windowsHide: false });
  closeSync(o);
  let exit = null;
  child.on('exit', (c, s) => (exit = `${c} ${s}`));
  await delay(20_000);
  log('A exit', exit);
  let ver = 'none';
  try {
    ver = await (await fetch('http://127.0.0.1:9333/json/version')).text();
  } catch (e) {
    ver = `fetch failed ${e.message}`;
  }
  log('A devtools', ver);
  log('A procs', procs());
  log('A userdata', listTree(join(root, 'webview2')));
  log('A localappdata', listTree(join(process.env.LOCALAPPDATA, 'dev.purepoint.app'), 2));
  log('A output', readFileSync(join(out, 'A-app-output.log'), 'utf8').slice(-6000));
  if (child.pid) killTree(child.pid);
  await delay(2000);
}

// B/C/D: msedgedriver directly, verbose, with tauri-driver's capabilities.
const variants = [
  ['B-fixture-env', fixtureEnv('b').env],
  ['C-no-userdata', fixtureEnv('c', { userData: false }).env],
  ['D-plain-env', { ...process.env }],
];
let port = 9600;
for (const [name, env] of variants) {
  port++;
  env.TAURI_AUTOMATION = 'true';
  env.TAURI_WEBVIEW_AUTOMATION = 'true';
  const logPath = join(out, `${name}-msedgedriver.log`);
  const o = openSync(join(out, `${name}-msedgedriver.stdout.log`), 'w');
  const driver = spawn(
    drivers.msedgedriver,
    [`--port=${port}`, '--host=127.0.0.1', '--verbose', `--log-path=${logPath}`],
    { env, stdio: ['ignore', o, o], windowsHide: true },
  );
  closeSync(o);
  await delay(2000);
  const body = {
    capabilities: {
      alwaysMatch: {
        browserName: 'webview2',
        'ms:edgeChromium': true,
        'ms:edgeOptions': { binary: appExe, args: [] },
      },
    },
  };
  const ac = new AbortController();
  const req = fetch(`http://127.0.0.1:${port}/session`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal: ac.signal,
  })
    .then((r) => r.text())
    .catch((e) => `error ${e.message}`);
  await delay(25_000);
  log(`${name} procs@25s`, procs());
  const timer = setTimeout(() => ac.abort(), 70_000);
  const res = await req;
  clearTimeout(timer);
  log(`${name} session`, res.slice(0, 3000));
  const id = /"sessionId"\s*:\s*"([^"]+)"/.exec(res)?.[1];
  if (id) {
    try {
      const t = await (await fetch(`http://127.0.0.1:${port}/session/${id}/title`)).text();
      log(`${name} title`, t);
      await fetch(`http://127.0.0.1:${port}/session/${id}`, { method: 'DELETE' });
    } catch (e) {
      log(`${name} title failed`, e.message);
    }
  }
  if (driver.pid) killTree(driver.pid);
  ps(
    `Get-Process purepoint-windows,msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force`,
  );
  await delay(2000);
}
writeFileSync(join(out, 'report.txt'), report.join('\n'));
