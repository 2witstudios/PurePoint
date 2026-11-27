// node --test scripts/installer-hooks.test.mjs (part of `npm test`): the NSIS hooks that stop
// PurePoint before install and uninstall (src-tauri/windows/installer-hooks.nsh, WIN-10.2). The
// installer is never run here: the .nsh is read as text, and the PowerShell selection it hands
// to nsExec is run against a stubbed Get-Process, so it is proven to pick only our own images.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { test } from 'node:test';

const NSH = fs.readFileSync(
  new URL('../src-tauri/windows/installer-hooks.nsh', import.meta.url),
  'utf8',
);
const MAIN = 'purepoint-windows';

/** The `!define NAME `value`` values, with NSIS's `$$` unescaped and MAINBINARYNAME set. */
function defines() {
  const found = {};
  for (const match of NSH.matchAll(/^!define (\w+) `(.*)`$/gm)) {
    found[match[1]] = match[2].replaceAll('$$', '$').replaceAll('${MAINBINARYNAME}', MAIN);
  }
  return found;
}

/** The body of `!macro name` … `!macroend`, without comment lines. */
function macro(name) {
  const match = NSH.match(new RegExp(`^!macro ${name}\\s*$([\\s\\S]*?)^!macroend`, 'm'));
  assert.ok(match, `no macro ${name}`);
  return match[1]
    .split(/\r?\n/)
    .filter((line) => !/^\s*;/.test(line))
    .join('\n');
}

test('stops anything only when $INSTDIR holds our app', () => {
  const body = macro('PUREPOINT_STOP_RUNNING').trim().split('\n');
  assert.equal(body[0].trim(), '${If} ${FileExists} "$INSTDIR\\${MAINBINARYNAME}.exe"');
  assert.equal(body.at(-1).trim(), '${EndIf}');
  // Nothing before the guard, and every stop inside it.
  const guarded = body.slice(1, -1).join('\n');
  assert.match(guarded, /Stop-Process/);
  assert.match(guarded, /pu\.exe" daemon stop/);
  for (const hook of ['NSIS_HOOK_PREINSTALL', 'NSIS_HOOK_PREUNINSTALL']) {
    assert.doesNotMatch(macro(hook), /Stop-Process|nsExec/, hook);
  }
});

test('every Stop-Process is fed by the known-image selection', () => {
  const body = macro('PUREPOINT_STOP_RUNNING');
  const stops = body.split('\n').filter((line) => line.includes('Stop-Process'));
  assert.equal(stops.length, 2);
  for (const line of stops) {
    assert.match(
      line,
      /\$\{PUREPOINT_(APP|OWN)_IMAGES?\}; \$\{PUREPOINT_RUNNING_IMAGES\} \| Stop-Process/,
    );
  }
  // No by-folder or by-name selection survives anywhere.
  assert.doesNotMatch(NSH, /StartsWith/);
  assert.doesNotMatch(NSH, /\.Name -eq/);
});

test('lists exactly the images PurePoint ships', () => {
  const d = defines();
  assert.equal(d.PUREPOINT_APP_IMAGE, `$k = @('${MAIN}.exe')`);
  assert.equal(
    d.PUREPOINT_OWN_IMAGES,
    `$k = @('${MAIN}.exe', 'pu-engine.exe', 'pu.exe', 'PointGuard\\bin\\node.exe', 'PointGuard\\bin\\point-guard-lock.exe')`,
  );
});

test(
  'the selection picks our images at their own paths and nothing else',
  { skip: process.platform !== 'win32' && 'PowerShell runs on Windows only' },
  () => {
    const d = defines();
    const inst = 'C:\\Users\\me\\AppData\\Local\\PurePoint';
    const processes = [
      `${inst}\\${MAIN}.exe`,
      `${inst}\\PU-ENGINE.EXE`,
      `${inst}\\pu.exe`,
      `${inst}\\PointGuard\\bin\\node.exe`,
      `${inst}\\PointGuard\\bin\\point-guard-lock.exe`,
      // Not ours: a node.exe or tool elsewhere under the folder, a look-alike folder, others.
      `${inst}\\node.exe`,
      `${inst}\\PointGuard\\node_modules\\x\\node.exe`,
      `${inst}\\tools\\pu.exe`,
      `${inst}\\other.exe`,
      `${inst}-old\\pu.exe`,
      'C:\\Program Files\\nodejs\\node.exe',
      null,
    ];
    const stub = `function Get-Process { ${processes
      .map((p) =>
        p === null ? '[pscustomobject]@{ Path = $null }' : `[pscustomobject]@{ Path = '${p}' }`,
      )
      .join(', ')} }`;
    const select = (images) =>
      execFileSync(
        'powershell.exe',
        [
          '-NoLogo',
          '-NoProfile',
          '-NonInteractive',
          '-ExecutionPolicy',
          'Bypass',
          '-Command',
          `${stub}; ${images}; ${d.PUREPOINT_RUNNING_IMAGES} | ForEach-Object { $_.Path }`,
        ],
        {
          encoding: 'utf8',
          windowsHide: true,
          env: { ...process.env, PUREPOINT_INSTDIR: `${inst}\\` },
        },
      )
        .split(/\r?\n/)
        .filter(Boolean);
    assert.deepEqual(select(d.PUREPOINT_APP_IMAGE), [`${inst}\\${MAIN}.exe`]);
    assert.deepEqual(select(d.PUREPOINT_OWN_IMAGES), processes.slice(0, 5));
  },
);
