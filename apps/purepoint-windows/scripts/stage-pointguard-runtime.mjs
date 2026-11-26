#!/usr/bin/env node
// Stages the Windows Point Guard runtime payload and writes its sealed manifest (WIN-9.3).
//
// Layout (<out>\PointGuard\, the app's Tauri resource `PointGuard\`, and the same layout
// `pu phone` unpacks into %USERPROFILE%\.pu\pointguard\<version>\):
//   runtime-manifest.json      layout "windows", paths, and seal.files: SHA-256 of every file
//   bin\node.exe               from the official Node zip pinned in
//                              apps/purepoint-mobile/runtime/node-release.json ("win-<arch>")
//   bin\point-guard-lock.exe   the freshly built pu-point-guard-lock.exe (--lock-helper)
//   bridge\                    apps/purepoint-mobile/bridge without *.test.js
//   node_modules\              production dependencies from the lock (npm ci --omit=dev
//                              --ignore-scripts), foreign native prebuilds removed
//   docs\, support\, package.json
//
// The app refuses to launch a runtime whose files differ from seal.files (src-tauri/src/
// pointguard/seal.rs). The manifest's own SHA-256 is printed last; a release build pins it
// with PUREPOINT_POINTGUARD_MANIFEST_SHA256 at compile time. scripts/build-release.mjs
// (WIN-10.2) imports `stage` and wires both into the installer.
//
// Usage:
//   node scripts/stage-pointguard-runtime.mjs --out <dir> --lock-helper <exe>
//        [--arch x64|arm64] [--cache <dir>] [--node-zip <zip>] [--replace]
//   node scripts/stage-pointguard-runtime.mjs --seal <dir>   rewrite seal.files for <dir>
//
// Builds on Windows only (it extracts the zip with the system tar.exe). Downloads the Node
// zip into --cache only when it is not there; the zip's SHA-256 must match the pin.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
export const REPO = path.resolve(here, '..', '..', '..');
export const MOBILE = path.join(REPO, 'apps', 'purepoint-mobile');
export const MANIFEST = 'runtime-manifest.json';
export const PATHS = {
  node: 'bin/node.exe',
  lockHelper: 'bin/point-guard-lock.exe',
  entry: 'bridge/main.js',
  instructions: 'docs/point-guard.md',
  workflowPackage: 'support/pointguard-workflows',
  skills: ['support/pu/SKILL.md', 'support/pu-cli/SKILL.md'],
};

/** Lowercase hex SHA-256 of a file. */
export function fileSha256(file) {
  return createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

/** Every regular file under `root` as sorted forward-slash paths, the manifest excluded. */
export function listFiles(root) {
  const out = [];
  const walk = (dir, prefix) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      const full = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`Payload must not contain links: ${relative}`);
      if (entry.isDirectory()) walk(full, relative);
      else if (entry.isFile()) {
        if (relative !== MANIFEST) out.push(relative);
      } else throw new Error(`Payload must contain only files and directories: ${relative}`);
    }
  };
  walk(root, '');
  return out.sort();
}

/** `seal` for a staged directory: { algorithm, files: { path: sha256 } }. */
export function sealFor(root) {
  const files = {};
  for (const relative of listFiles(root)) files[relative] = fileSha256(path.join(root, relative));
  return { algorithm: 'sha256', files };
}

/** The Windows manifest, without its seal. */
export function windowsManifest({ puVersion, piVersion, nodeVersion, architecture, sourceSHA }) {
  return {
    schemaVersion: 1,
    contractVersion: 1,
    layout: 'windows',
    puVersion,
    piVersion,
    nodeVersion,
    architecture,
    sourceSHA,
    paths: PATHS,
  };
}

/** Write `<root>/runtime-manifest.json` with a fresh seal; returns the manifest's SHA-256. */
export function writeManifest(root, fields) {
  const manifest = { ...fields, seal: sealFor(root) };
  const text = JSON.stringify(manifest, null, 2) + '\n';
  fs.writeFileSync(path.join(root, MANIFEST), text);
  return createHash('sha256').update(text).digest('hex');
}

/** The workspace version (`[workspace.package] version`), the `pu` this payload is for. */
export function workspaceVersion(cargoToml) {
  const section = cargoToml.split(/^\[workspace\.package\]\s*$/m)[1] ?? '';
  const match = /^version\s*=\s*"([^"]+)"/m.exec(section.split(/^\[/m)[0]);
  if (!match) throw new Error('No [workspace.package] version in Cargo.toml');
  return match[1];
}

/** The pinned Windows Node zip for `arch` from node-release.json. */
export function nodePin(release, arch) {
  const archive = release.archives[`win-${arch}`];
  if (!archive) throw new Error(`node-release.json has no win-${arch} archive`);
  if (archive.name !== `node-v${release.version}-win-${arch}.zip`)
    throw new Error(`Unexpected archive name ${archive.name}`);
  return { version: release.version, ...archive };
}

/** Remove other platforms' native prebuilds from a package's native/ directory. */
export function pruneForeignNatives(native, keep = 'win32') {
  if (!fs.existsSync(native)) return;
  for (const entry of fs.readdirSync(native, { withFileTypes: true }))
    if (entry.isDirectory() && entry.name !== keep)
      fs.rmSync(path.join(native, entry.name), { recursive: true, force: true });
}

function parseArgs(argv) {
  const args = { arch: 'x64', replace: false };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${flag} needs a value`);
      return next;
    };
    if (flag === '--out') args.out = value();
    else if (flag === '--lock-helper') args.lockHelper = value();
    else if (flag === '--arch') args.arch = value();
    else if (flag === '--cache') args.cache = value();
    else if (flag === '--node-zip') args.nodeZip = value();
    else if (flag === '--seal') args.seal = value();
    else if (flag === '--replace') args.replace = true;
    else throw new Error(`Unknown argument ${flag}`);
  }
  return args;
}

async function nodeZip(pin, args) {
  if (args.nodeZip) return path.resolve(args.nodeZip);
  const cache = path.resolve(args.cache ?? path.join(os.tmpdir(), 'purepoint-node-cache'));
  fs.mkdirSync(cache, { recursive: true });
  const zip = path.join(cache, pin.name);
  if (!fs.existsSync(zip)) {
    const url = `https://nodejs.org/dist/v${pin.version}/${pin.name}`;
    console.log(`Downloading ${url}`);
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Download failed: HTTP ${response.status}`);
    const partial = `${zip}.download`;
    fs.writeFileSync(partial, Buffer.from(await response.arrayBuffer()));
    if (fileSha256(partial) !== pin.sha256) {
      fs.rmSync(partial, { force: true });
      throw new Error('Pinned Node checksum mismatch');
    }
    fs.renameSync(partial, zip);
  }
  return zip;
}

function run(command, args, options = {}) {
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

/** Stage `<args.out>/PointGuard`; returns its path and the manifest's SHA-256. */
export async function stage(args) {
  if (process.platform !== 'win32') throw new Error('Stage the Windows payload on Windows.');
  if (!args.out || !args.lockHelper) throw new Error('--out and --lock-helper are required');
  if (!['x64', 'arm64'].includes(args.arch)) throw new Error(`Unknown --arch ${args.arch}`);
  const lockHelper = path.resolve(args.lockHelper);
  if (!fs.statSync(lockHelper).isFile())
    throw new Error('Provide the built pu-point-guard-lock.exe');
  const target = path.join(path.resolve(args.out), 'PointGuard');
  if (fs.existsSync(target)) {
    if (!args.replace) throw new Error(`Refusing to replace ${target} without --replace`);
    fs.rmSync(target, { recursive: true, force: true });
  }
  const release = JSON.parse(
    fs.readFileSync(path.join(MOBILE, 'runtime', 'node-release.json'), 'utf8'),
  );
  const pin = nodePin(release, args.arch);
  const zip = await nodeZip(pin, args);
  if (fileSha256(zip) !== pin.sha256) throw new Error('Node zip checksum mismatch');
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'pointguard-stage-'));
  try {
    fs.mkdirSync(path.join(target, 'bin'), { recursive: true });
    const member = `${pin.name.replace(/\.zip$/, '')}/node.exe`;
    const tar = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');
    run(tar, ['-xf', zip, '-C', temp, member]);
    fs.copyFileSync(path.join(temp, member), path.join(target, 'bin', 'node.exe'));
    fs.copyFileSync(lockHelper, path.join(target, 'bin', 'point-guard-lock.exe'));
    // All production transitive dependencies come from the lock, install scripts off.
    const install = path.join(temp, 'npm');
    fs.mkdirSync(install);
    for (const file of ['package.json', 'package-lock.json'])
      fs.copyFileSync(path.join(MOBILE, file), path.join(install, file));
    // npm is npm.cmd on Windows: run it through cmd.exe with fixed arguments.
    run(
      process.env.ComSpec ?? 'cmd.exe',
      ['/d', '/c', 'npm', 'ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'],
      { cwd: install },
    );
    fs.cpSync(path.join(install, 'node_modules'), path.join(target, 'node_modules'), {
      recursive: true,
      verbatimSymlinks: true,
    });
    pruneForeignNatives(path.join(target, 'node_modules', '@earendil-works', 'pi-tui', 'native'));
    fs.cpSync(path.join(MOBILE, 'bridge'), path.join(target, 'bridge'), {
      recursive: true,
      filter: (source) => !source.endsWith('.test.js'),
    });
    fs.cpSync(path.join(MOBILE, 'docs'), path.join(target, 'docs'), { recursive: true });
    fs.cpSync(path.join(MOBILE, 'support'), path.join(target, 'support'), { recursive: true });
    fs.copyFileSync(path.join(MOBILE, 'package.json'), path.join(target, 'package.json'));
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
  const mobilePackage = JSON.parse(fs.readFileSync(path.join(MOBILE, 'package.json'), 'utf8'));
  const sourceSHA = execFileSync('git', ['-C', REPO, 'rev-parse', 'HEAD'], {
    encoding: 'utf8',
  }).trim();
  const digest = writeManifest(
    target,
    windowsManifest({
      puVersion: workspaceVersion(fs.readFileSync(path.join(REPO, 'Cargo.toml'), 'utf8')),
      piVersion: mobilePackage.dependencies['@earendil-works/pi-coding-agent'],
      nodeVersion: pin.version,
      architecture: args.arch,
      sourceSHA,
    }),
  );
  return { target, digest };
}

function reseal(dir) {
  const root = path.resolve(dir);
  const file = path.join(root, MANIFEST);
  const existing = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
  delete existing.seal;
  console.log(writeManifest(root, existing));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    if (args.seal) reseal(args.seal);
    else {
      const { target, digest } = await stage(args);
      console.log(`Staged ${target}`);
      console.log(`PUREPOINT_POINTGUARD_MANIFEST_SHA256=${digest}`);
    }
  } catch (error) {
    console.error(
      `stage-pointguard-runtime: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exit(1);
  }
}
