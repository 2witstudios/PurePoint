// The update feed of the Windows app (WIN-10.3, REQ-WINAPP-076), written by build-release.mjs.
//
// The app checks https://github.com/2witstudios/PurePoint/releases/download/appcast/latest-windows.json:
// the fixed `appcast` release that also carries the Mac's Sparkle appcast.xml, so each platform
// replaces only its own asset. The installer itself is an asset of the versioned release
// (`v<version>`), which the feed's `url` points at.
//
// Keys (minisign, `tauri signer generate`; the release key is WIN-10.6's):
//   TAURI_UPDATER_PUBKEY          the public key built into the app (`plugins.updater.pubkey`).
//                                 Without it the app keeps the committed placeholder and shows
//                                 "Updates not configured".
//   TAURI_SIGNING_PRIVATE_KEY     the private key (or TAURI_SIGNING_PRIVATE_KEY_PATH), and
//                                 TAURI_SIGNING_PRIVATE_KEY_PASSWORD when it has one. Without it
//                                 no signature or feed is written (a warning says so).
// The signature binds the version (`--app-version`), which the app requires
// (`requireSignedVersion`), so a feed cannot pair a newer version with an older installer.

import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export const FEED_NAME = 'latest-windows.json';
export const FEED_URL = `https://github.com/2witstudios/PurePoint/releases/download/appcast/${FEED_NAME}`;
export const DOWNLOAD_BASE = 'https://github.com/2witstudios/PurePoint/releases/download';

const PLATFORM_ARCH = { x64: 'x86_64', arm64: 'aarch64' };

/** The keys the build was given, and what it can do with them. Throws on a key with no pair. */
export function updaterKeys(env = process.env) {
  const pubkey = env.TAURI_UPDATER_PUBKEY?.trim() || null;
  const canSign = Boolean(
    env.TAURI_SIGNING_PRIVATE_KEY?.trim() || env.TAURI_SIGNING_PRIVATE_KEY_PATH?.trim(),
  );
  if (canSign && pubkey === null)
    throw new Error(
      'TAURI_SIGNING_PRIVATE_KEY is set but TAURI_UPDATER_PUBKEY is not: the app could not verify the update it signs.',
    );
  if (pubkey !== null && pubkey.startsWith('PLACEHOLDER'))
    throw new Error('TAURI_UPDATER_PUBKEY is the placeholder, not a public key.');
  return { pubkey, canSign };
}

/** The config merged over tauri.conf.json for the release key, or nothing. */
export function updaterConfig(pubkey) {
  return pubkey === null ? {} : { plugins: { updater: { pubkey } } };
}

/** The `platforms` keys the updater looks up for an NSIS install on `arch`. */
export function platformKeys(arch) {
  const cpu = PLATFORM_ARCH[arch];
  if (cpu === undefined) throw new Error(`Unknown arch ${arch}`);
  return [`windows-${cpu}-nsis`, `windows-${cpu}`];
}

/**
 * The feed (Tauri's static `latest.json` format) for one release.
 * `installers`: [{ arch, file, signature }]; `base`: where the versioned assets live.
 */
export function feedManifest({ version, notes = '', pubDate, installers, base = DOWNLOAD_BASE }) {
  const platforms = {};
  for (const { arch, file, signature } of installers) {
    const url = `${base}/v${version}/${encodeURIComponent(file)}`;
    for (const key of platformKeys(arch)) platforms[key] = { signature, url };
  }
  return { version, notes, pub_date: pubDate.toISOString(), platforms };
}

/**
 * Sign `file` with the Tauri CLI (`<file>.sig`, bound to `version`) and return the signature.
 * The private key comes from the environment, never the command line.
 */
export function signInstaller({ cli, file, version, env = process.env }) {
  const args = [cli, 'signer', 'sign', '--app-version', version];
  // An unencrypted key: say so, or the CLI would ask for a password.
  if (env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD === undefined) args.push('--password', '');
  args.push(file);
  execFileSync(process.execPath, args, { env, stdio: ['ignore', 'ignore', 'inherit'] });
  return fs.readFileSync(`${file}.sig`, 'utf8').trim();
}

/** Write the feed beside the installers and return its path. */
export function writeFeed(dir, manifest) {
  const target = path.join(dir, FEED_NAME);
  fs.writeFileSync(target, JSON.stringify(manifest, null, 2) + '\n');
  return target;
}

/** The minisign public key inside TAURI_UPDATER_PUBKEY (base64 of the `.pub` file's text). */
function minisignPublicKey(pubkey) {
  const line = Buffer.from(pubkey, 'base64').toString('utf8').split('\n')[1]?.trim() ?? '';
  const raw = Buffer.from(line, 'base64');
  if (raw.length !== 42 || raw.subarray(0, 2).toString() !== 'Ed')
    throw new Error('TAURI_UPDATER_PUBKEY is not a minisign public key');
  return { keyId: raw.subarray(2, 10), key: raw.subarray(10) };
}

function ed25519Verify(key, message, signature) {
  // DER SubjectPublicKeyInfo prefix for an Ed25519 key.
  const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key]);
  return crypto.verify(null, message, { key: spki, format: 'der', type: 'spki' }, signature);
}

/**
 * Check an updater signature (`<file>.sig`, base64 of a minisign signature) the way the app
 * will: made by `pubkey` over `file`, with a trusted comment that names `version`. Throws on
 * anything else, so a signing key that doesn't match the app's public key fails the release.
 */
export function verifyUpdaterSignature({ pubkey, file, signature, version }) {
  const { keyId, key } = minisignPublicKey(pubkey);
  const lines = Buffer.from(signature, 'base64').toString('utf8').split('\n');
  const sigLine = Buffer.from(lines[1]?.trim() ?? '', 'base64');
  const trusted = lines[2] ?? '';
  const global = Buffer.from(lines[3]?.trim() ?? '', 'base64');
  if (sigLine.length !== 74 || !trusted.startsWith('trusted comment: ') || global.length !== 64)
    throw new Error(`malformed updater signature for ${path.basename(file)}`);
  if (!sigLine.subarray(2, 10).equals(keyId))
    throw new Error(`${path.basename(file)} is signed by another key than TAURI_UPDATER_PUBKEY`);
  const algorithm = sigLine.subarray(0, 2).toString();
  const content = fs.readFileSync(file);
  const message =
    algorithm === 'ED'
      ? crypto.createHash('blake2b512').update(content).digest()
      : algorithm === 'Ed'
        ? content
        : null;
  const sig = sigLine.subarray(10);
  const comment = trusted.slice('trusted comment: '.length);
  if (
    message === null ||
    !ed25519Verify(key, message, sig) ||
    !ed25519Verify(key, Buffer.concat([sig, Buffer.from(comment, 'utf8')]), global)
  )
    throw new Error(`the updater signature of ${path.basename(file)} does not verify`);
  if (!comment.split('\t').includes(`version:${version}`))
    throw new Error(
      `the updater signature of ${path.basename(file)} does not name version ${version}`,
    );
}
