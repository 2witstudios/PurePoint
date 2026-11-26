#!/usr/bin/env node
// Builds the Windows release installer (WIN-10.2, REQ-WINAPP-077): `npm run bundle`.
//
//   1. cargo build --release of pu-cli (pu.exe, pu-point-guard-lock.exe) and pu-engine at the
//      repository root.
//   2. Stage and seal the Point Guard runtime (stage-pointguard-runtime.mjs) into
//      src-tauri/target/bundle-inputs/PointGuard, with the lock helper from step 1.
//   3. Write src-tauri/target/bundle-inputs/tauri.bundle.conf.json, merged over tauri.conf.json
//      with `tauri build --config`: pu.exe and pu-engine.exe at the resource root (= the install
//      dir, beside the app exe, where the installer and daemon lifecycle look first) and
//      PointGuard\. They are not in tauri.conf.json itself because tauri-build copies every
//      resource on each cargo build, so `cargo clippy`/`cargo test` would need them staged.
//   4. tauri build --bundles nsis, with PUREPOINT_POINTGUARD_MANIFEST_SHA256 set to the staged
//      manifest's digest, so the release exe accepts only that manifest.
//
// Output: src-tauri/target/release/bundle/nsis/PurePoint_<version>_<arch>-setup.exe (unsigned).
//
// Usage: node scripts/build-release.mjs [--jobs N] [--arch x64|arm64] [--cache <dir>]
//        [--node-zip <zip>]
// --jobs caps cargo's parallelism for both builds (CARGO_BUILD_JOBS); --cache and --node-zip
// go to the staging script.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { REPO, stage } from './stage-pointguard-runtime.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const APP = path.resolve(here, '..');
const TAURI = path.join(APP, 'src-tauri');
const INPUTS = path.join(TAURI, 'target', 'bundle-inputs');
const TARGETS = { x64: 'x86_64-pc-windows-msvc', arm64: 'aarch64-pc-windows-msvc' };

function parseArgs(argv) {
  const args = { arch: 'x64' };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${flag} needs a value`);
      return next;
    };
    if (flag === '--jobs') args.jobs = value();
    else if (flag === '--arch') args.arch = value();
    else if (flag === '--cache') args.cache = value();
    else if (flag === '--node-zip') args.nodeZip = value();
    else throw new Error(`Unknown argument ${flag}`);
  }
  if (!(args.arch in TARGETS)) throw new Error(`Unknown --arch ${args.arch}`);
  if (args.jobs !== undefined && !/^[1-9]\d*$/.test(args.jobs))
    throw new Error('--jobs must be a positive integer');
  return args;
}

function run(command, args, options = {}) {
  console.log(`> ${command} ${args.join(' ')}`);
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

/** The merged Tauri config: the bundled binaries and runtime as resources (absolute sources). */
export function bundleConfig({ binDir, pointGuard }) {
  const source = (p) => p.split(path.sep).join('/');
  return {
    bundle: {
      resources: {
        [source(path.join(binDir, 'pu.exe'))]: 'pu.exe',
        [source(path.join(binDir, 'pu-engine.exe'))]: 'pu-engine.exe',
        [`${source(pointGuard)}/`]: 'PointGuard/',
      },
    },
  };
}

async function main() {
  if (process.platform !== 'win32') throw new Error('Build the Windows installer on Windows.');
  const args = parseArgs(process.argv.slice(2));
  const triple = TARGETS[args.arch];
  const env = { ...process.env };
  if (args.jobs) env.CARGO_BUILD_JOBS = args.jobs;
  const cross = args.arch !== 'x64' || process.arch !== 'x64';

  const cargoTarget = path.resolve(REPO, process.env.CARGO_TARGET_DIR ?? 'target');
  const cargoArgs = ['build', '--release', '--locked', '-p', 'pu-cli', '-p', 'pu-engine'];
  if (cross) cargoArgs.push('--target', triple);
  run('cargo', cargoArgs, { cwd: REPO, env });
  const binDir = cross
    ? path.join(cargoTarget, triple, 'release')
    : path.join(cargoTarget, 'release');
  for (const exe of ['pu.exe', 'pu-engine.exe', 'pu-point-guard-lock.exe'])
    if (!fs.statSync(path.join(binDir, exe), { throwIfNoEntry: false })?.isFile())
      throw new Error(`cargo did not produce ${path.join(binDir, exe)}`);

  fs.mkdirSync(INPUTS, { recursive: true });
  const { target: pointGuard, digest } = await stage({
    out: INPUTS,
    lockHelper: path.join(binDir, 'pu-point-guard-lock.exe'),
    arch: args.arch,
    cache: args.cache,
    nodeZip: args.nodeZip,
    replace: true,
  });
  console.log(`Staged ${pointGuard} (manifest SHA-256 ${digest})`);

  const config = path.join(INPUTS, 'tauri.bundle.conf.json');
  fs.writeFileSync(config, JSON.stringify(bundleConfig({ binDir, pointGuard }), null, 2) + '\n');

  // npm's tauri shim is a .cmd: run the CLI's JS entry with this Node instead.
  const cli = path.join(APP, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
  const tauriArgs = [cli, 'build', '--bundles', 'nsis', '--config', config, '--ci'];
  if (cross) tauriArgs.push('--target', triple);
  run(process.execPath, tauriArgs, {
    cwd: APP,
    env: { ...env, PUREPOINT_POINTGUARD_MANIFEST_SHA256: digest },
  });

  const nsis = path.join(TAURI, 'target', ...(cross ? [triple] : []), 'release', 'bundle', 'nsis');
  const installers = fs.readdirSync(nsis).filter((f) => f.endsWith('-setup.exe'));
  if (installers.length === 0) throw new Error(`No installer in ${nsis}`);
  for (const file of installers) {
    const size = fs.statSync(path.join(nsis, file)).size;
    console.log(`Installer: ${path.join(nsis, file)} (${(size / 1048576).toFixed(1)} MiB)`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    await main();
  } catch (error) {
    console.error(`build-release: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
