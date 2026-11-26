// node --test scripts/updater-feed.test.mjs (part of `npm test`): the update feed build-release.mjs
// writes (WIN-10.3). Signing uses a throwaway key generated in a temp dir and deleted after.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { bundleConfig } from './build-release.mjs';
import { installerArch, parseArgs } from './sign-release-feed.mjs';
import {
  FEED_NAME,
  FEED_URL,
  feedManifest,
  platformKeys,
  signInstaller,
  updaterConfig,
  updaterKeys,
  verifyUpdaterSignature,
  writeFeed,
} from './updater-feed.mjs';

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CLI = path.join(APP, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

test('the feed is the appcast release asset the app config names', () => {
  const config = JSON.parse(
    fs.readFileSync(path.join(APP, 'src-tauri', 'tauri.conf.json'), 'utf8'),
  );
  assert.deepEqual(config.plugins.updater.endpoints, [FEED_URL]);
  assert.equal(
    FEED_URL,
    'https://github.com/2witstudios/purepoint/releases/download/appcast/latest-windows.json',
  );
  assert.match(config.plugins.updater.pubkey, /^PLACEHOLDER/);
});

test('keys: a private key needs its public key; none is fine', () => {
  assert.deepEqual(updaterKeys({}), { pubkey: null, canSign: false });
  assert.deepEqual(updaterKeys({ TAURI_UPDATER_PUBKEY: ' abc ' }), {
    pubkey: 'abc',
    canSign: false,
  });
  assert.deepEqual(
    updaterKeys({ TAURI_UPDATER_PUBKEY: 'abc', TAURI_SIGNING_PRIVATE_KEY_PATH: 'k.key' }),
    { pubkey: 'abc', canSign: true },
  );
  assert.throws(() => updaterKeys({ TAURI_SIGNING_PRIVATE_KEY: 'secret' }), /TAURI_UPDATER_PUBKEY/);
  assert.throws(() => updaterKeys({ TAURI_UPDATER_PUBKEY: 'PLACEHOLDER-x' }), /placeholder/);
});

test('the release public key is merged over the placeholder', () => {
  assert.deepEqual(updaterConfig(null), {});
  const merged = bundleConfig({ binDir: 'C:\\bin', pointGuard: 'C:\\pg', pubkey: 'KEY' });
  assert.equal(merged.plugins.updater.pubkey, 'KEY');
  assert.equal(bundleConfig({ binDir: 'C:\\bin', pointGuard: 'C:\\pg' }).plugins, undefined);
});

test('a release build carries the tag version; a local one keeps tauri.conf.json', () => {
  const dirs = { binDir: 'C:\\bin', pointGuard: 'C:\\pg' };
  assert.equal(bundleConfig({ ...dirs, version: '1.2.3' }).version, '1.2.3');
  assert.equal('version' in bundleConfig(dirs), false);
});

test('the release feed signer takes one installer of the release per architecture', () => {
  assert.equal(installerArch('dist/PurePoint_1.2.3_x64-setup.exe', '1.2.3'), 'x64');
  assert.equal(installerArch('PurePoint_1.2.3_arm64-setup.exe', '1.2.3'), 'arm64');
  assert.throws(() => installerArch('PurePoint_1.2.4_x64-setup.exe', '1.2.3'), /not version/);
  assert.throws(() => installerArch('PurePoint_1.2.3_x86-setup.exe', '1.2.3'), /Not a PurePoint/);
  const both = ['PurePoint_1.2.3_x64-setup.exe', 'PurePoint_1.2.3_arm64-setup.exe'];
  assert.equal(parseArgs(['--version', '1.2.3', '--out', 'feed', ...both]).installers.length, 2);
  const twice = ['PurePoint_1.2.3_x64-setup.exe', 'x/PurePoint_1.2.3_x64-setup.exe'];
  assert.throws(
    () => parseArgs(['--version', '1.2.3', '--out', 'f', ...twice]),
    /per architecture/,
  );
  assert.throws(() => parseArgs(['--out', 'f', both[0]]), /--version/);
});

test('the manifest lists the NSIS installer of the versioned release', () => {
  assert.deepEqual(platformKeys('x64'), ['windows-x86_64-nsis', 'windows-x86_64']);
  assert.deepEqual(platformKeys('arm64'), ['windows-aarch64-nsis', 'windows-aarch64']);
  const manifest = feedManifest({
    version: '0.2.0',
    notes: 'Faster agents.',
    pubDate: new Date('2026-11-20T12:00:00Z'),
    installers: [{ arch: 'x64', file: 'PurePoint_0.2.0_x64-setup.exe', signature: 'SIG' }],
  });
  const entry = {
    signature: 'SIG',
    url: 'https://github.com/2witstudios/purepoint/releases/download/v0.2.0/PurePoint_0.2.0_x64-setup.exe',
  };
  assert.deepEqual(manifest, {
    version: '0.2.0',
    notes: 'Faster agents.',
    pub_date: '2026-11-20T12:00:00.000Z',
    platforms: { 'windows-x86_64-nsis': entry, 'windows-x86_64': entry },
  });
});

test(
  'signs an installer bound to its version and writes the feed',
  { skip: !fs.existsSync(CLI) && 'the Tauri CLI is not installed' },
  () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pp-updater-feed-'));
    try {
      const key = path.join(dir, 'throwaway.key');
      execFileSync(process.execPath, [CLI, 'signer', 'generate', '--ci', '-p', '', '-w', key], {
        stdio: 'ignore',
      });
      const setup = path.join(dir, 'PurePoint_0.2.0_x64-setup.exe');
      fs.writeFileSync(setup, 'not an installer\n');
      const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(key, 'utf8') };
      delete env.TAURI_SIGNING_PRIVATE_KEY_PATH;
      delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
      const signature = signInstaller({ cli: CLI, file: setup, version: '0.2.0', env });
      const decoded = Buffer.from(signature, 'base64').toString('utf8');
      assert.match(decoded, /trusted comment: .*\tversion:0\.2\.0/);
      const publicKey = Buffer.from(fs.readFileSync(`${key}.pub`, 'utf8'), 'base64').toString();
      // The signature names the key that made it (minisign key id, bytes 2-10).
      const keyId = (b64) => Buffer.from(b64, 'base64').subarray(2, 10).toString('hex');
      assert.equal(keyId(decoded.split('\n')[1] ?? ''), keyId(publicKey.split('\n')[1] ?? ''));

      // The release job checks each signature against the app's public key (WIN-10.4).
      const pubkey = fs.readFileSync(`${key}.pub`, 'utf8').trim();
      const check = (overrides) =>
        verifyUpdaterSignature({ pubkey, file: setup, signature, version: '0.2.0', ...overrides });
      check({});
      assert.throws(() => check({ version: '0.2.1' }), /does not name version 0\.2\.1/);
      const other = path.join(dir, 'other.key');
      execFileSync(process.execPath, [CLI, 'signer', 'generate', '--ci', '-p', '', '-w', other], {
        stdio: 'ignore',
      });
      assert.throws(
        () => check({ pubkey: fs.readFileSync(`${other}.pub`, 'utf8') }),
        /another key/,
      );
      fs.writeFileSync(setup, 'tampered\n');
      assert.throws(() => check({}), /does not verify/);
      fs.writeFileSync(setup, 'not an installer\n');

      const feed = writeFeed(
        dir,
        feedManifest({
          version: '0.2.0',
          pubDate: new Date(0),
          installers: [{ arch: 'x64', file: path.basename(setup), signature }],
        }),
      );
      assert.equal(path.basename(feed), FEED_NAME);
      const written = JSON.parse(fs.readFileSync(feed, 'utf8'));
      assert.equal(written.platforms['windows-x86_64-nsis'].signature, signature);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
);
