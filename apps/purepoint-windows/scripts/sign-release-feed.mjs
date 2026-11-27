#!/usr/bin/env node
// Signs the release's Windows installers for the updater and writes the combined feed
// (WIN-10.4). The `sign-windows` job of .github/workflows/release.yml runs it once for both
// architectures, after any Authenticode signing, so the signatures cover the final bytes:
//
//   node scripts/sign-release-feed.mjs --version X.Y.Z --out <dir> <PurePoint_X.Y.Z_<arch>-setup.exe>...
//
// Keys come from the environment as for build-release.mjs (updater-feed.mjs):
// TAURI_SIGNING_PRIVATE_KEY (+ _PASSWORD) and TAURI_UPDATER_PUBKEY. Without a private key it
// writes nothing and warns (exit 0); with one, each signature is checked against the public key
// the installers were built with before <out>/latest-windows.json is written. Notes come from
// PUREPOINT_RELEASE_NOTES.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import {
  feedManifest,
  signInstaller,
  updaterKeys,
  verifyUpdaterSignature,
  writeFeed,
} from './updater-feed.mjs';

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CLI = path.join(APP, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

/** The architecture an installer is for, from Tauri's `PurePoint_<version>_<arch>-setup.exe`. */
export function installerArch(file, version) {
  const match = /^PurePoint_(\d+\.\d+\.\d+)_(x64|arm64)-setup\.exe$/.exec(path.basename(file));
  if (!match) throw new Error(`Not a PurePoint installer: ${path.basename(file)}`);
  if (match[1] !== version) throw new Error(`${path.basename(file)} is not version ${version}`);
  return match[2];
}

export function parseArgs(argv) {
  const args = { installers: [] };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--version' || flag === '--out') {
      const value = argv[++i];
      if (value === undefined) throw new Error(`${flag} needs a value`);
      args[flag.slice(2)] = value;
    } else if (flag.startsWith('--')) throw new Error(`Unknown argument ${flag}`);
    else args.installers.push(flag);
  }
  if (!args.version || !/^\d+\.\d+\.\d+$/.test(args.version))
    throw new Error('--version X.Y.Z is required');
  if (!args.out) throw new Error('--out is required');
  if (args.installers.length === 0) throw new Error('No installers given');
  const arches = args.installers.map((file) => installerArch(file, args.version));
  if (new Set(arches).size !== arches.length) throw new Error('One installer per architecture');
  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const keys = updaterKeys();
  if (!keys.canSign) {
    console.warn(
      'sign-release-feed: warning: TAURI_SIGNING_PRIVATE_KEY is not set; no updater signatures or latest-windows.json were written.',
    );
    return;
  }
  const installers = args.installers.map((file) => {
    const signature = signInstaller({ cli: CLI, file, version: args.version });
    verifyUpdaterSignature({ pubkey: keys.pubkey, file, signature, version: args.version });
    return { arch: installerArch(file, args.version), file: path.basename(file), signature };
  });
  fs.mkdirSync(args.out, { recursive: true });
  const feed = writeFeed(
    args.out,
    feedManifest({
      version: args.version,
      notes: process.env.PUREPOINT_RELEASE_NOTES ?? '',
      pubDate: new Date(),
      installers,
    }),
  );
  console.log(`Update feed: ${feed} (${installers.map((i) => i.arch).join(', ')})`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main();
  } catch (error) {
    console.error(`sign-release-feed: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
