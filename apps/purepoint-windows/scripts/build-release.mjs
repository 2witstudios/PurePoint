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
//      With TAURI_UPDATER_PUBKEY it also sets the updater's public key (WIN-10.3), replacing
//      the committed placeholder; without it the app shows "Updates not configured".
//   4. tauri build --bundles nsis, with PUREPOINT_POINTGUARD_MANIFEST_SHA256 set to the staged
//      manifest's digest, so the release exe accepts only that manifest.
//   5. With TAURI_SIGNING_PRIVATE_KEY (or _PATH), sign each installer for the updater
//      (`<setup>.exe.sig`, bound to the version) and write the feed, latest-windows.json, beside
//      it; otherwise skip both with a warning. See updater-feed.mjs for the feed and keys.
//
// Output: src-tauri/target/release/bundle/nsis/PurePoint_<version>_<arch>-setup.exe (no
// Authenticode signature), and with a signing key its .sig and latest-windows.json.
//
// Usage: node scripts/build-release.mjs [--jobs N] [--arch x64|arm64] [--version X.Y.Z]
//        [--cache <dir>] [--node-zip <zip>]
// --jobs caps cargo's parallelism for both builds (CARGO_BUILD_JOBS); --version overrides
// tauri.conf.json's version (the release job passes the tag's, WIN-10.4); --cache and
// --node-zip go to the staging script.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { REPO, stage } from './stage-pointguard-runtime.mjs';
import {
  feedManifest,
  signInstaller,
  updaterConfig,
  updaterKeys,
  writeFeed,
} from './updater-feed.mjs';

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
    else if (flag === '--version') args.version = value();
    else if (flag === '--cache') args.cache = value();
    else if (flag === '--node-zip') args.nodeZip = value();
    else throw new Error(`Unknown argument ${flag}`);
  }
  if (!(args.arch in TARGETS)) throw new Error(`Unknown --arch ${args.arch}`);
  if (args.version !== undefined && !/^\d+\.\d+\.\d+$/.test(args.version))
    throw new Error('--version must be X.Y.Z');
  if (args.jobs !== undefined && !/^[1-9]\d*$/.test(args.jobs))
    throw new Error('--jobs must be a positive integer');
  return args;
}

function run(command, args, options = {}) {
  console.log(`> ${command} ${args.join(' ')}`);
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

/**
 * The merged Tauri config: the bundled binaries and runtime as resources (absolute sources), the
 * updater's release public key when there is one, and the release version when it is given.
 */
export function bundleConfig({ binDir, pointGuard, pubkey = null, version = null }) {
  const source = (p) => p.split(path.sep).join('/');
  return {
    ...(version === null ? {} : { version }),
    ...updaterConfig(pubkey),
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
  // Before any build, so a key without its pair fails fast.
  const keys = updaterKeys();
  if (keys.pubkey === null)
    console.warn(
      'build-release: warning: TAURI_UPDATER_PUBKEY is not set; the app keeps the placeholder key and shows "Updates not configured".',
    );
  // The signing key goes only to the signer, never to cargo or the bundler.
  const env = { ...process.env };
  for (const name of [
    'TAURI_SIGNING_PRIVATE_KEY',
    'TAURI_SIGNING_PRIVATE_KEY_PATH',
    'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  ])
    delete env[name];
  if (args.jobs) env.CARGO_BUILD_JOBS = args.jobs;
  // pu records the release version, which `pu phone` requires the runtime manifest to match.
  if (args.version) env.PU_RELEASE_VERSION = args.version;
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
    puVersion: args.version,
    replace: true,
  });
  console.log(`Staged ${pointGuard} (manifest SHA-256 ${digest})`);

  const config = path.join(INPUTS, 'tauri.bundle.conf.json');
  fs.writeFileSync(
    config,
    JSON.stringify(
      bundleConfig({ binDir, pointGuard, pubkey: keys.pubkey, version: args.version ?? null }),
      null,
      2,
    ) + '\n',
  );

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

  if (!keys.canSign) {
    console.warn(
      'build-release: warning: TAURI_SIGNING_PRIVATE_KEY is not set; no updater signature or latest-windows.json was written.',
    );
    return;
  }
  const version =
    args.version ??
    JSON.parse(fs.readFileSync(path.join(TAURI, 'tauri.conf.json'), 'utf8')).version;
  const signed = installers
    .filter((file) => file.includes(`_${version}_`))
    .map((file) => ({
      arch: args.arch,
      file,
      signature: signInstaller({ cli, file: path.join(nsis, file), version }),
    }));
  if (signed.length === 0) throw new Error(`No installer for version ${version} in ${nsis}`);
  const feed = writeFeed(
    nsis,
    feedManifest({
      version,
      notes: process.env.PUREPOINT_RELEASE_NOTES ?? '',
      pubDate: new Date(),
      installers: signed,
    }),
  );
  console.log(`Update feed: ${feed}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    await main();
  } catch (error) {
    console.error(`build-release: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
