import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, readFile, writeFile, stat } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { openTrustStore } from './trust.js';
const endpoint = 'wss://100.100.1.2:8787/v1';
async function fixture(fn) {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'pg-trust-'));
  let now = Date.now(); let store;
  try { store = await openTrustStore({ directory, now: () => now }); await fn(store, directory, (value) => { now += value; }); }
  finally { await store?.close(); await rm(directory, { recursive: true, force: true }); }
}
test('short-lived enrollment is single use under concurrent attempts; credential stays out of durable state', async () => fixture(async (store, directory) => {
  const enrollment = store.createEnrollment({ endpoint }); const code = JSON.parse(enrollment.payload);
  assert.equal(code.version, 2); assert.equal(code.certificateSHA256, store.certificateSHA256); assert.equal(code.secret, undefined);
  const outcomes = await Promise.allSettled([store.enroll({ enrollmentToken: code.enrollmentToken, name: 'Phone' }), store.enroll({ enrollmentToken: code.enrollmentToken, name: 'Imposter' })]);
  assert.equal(outcomes.filter(x => x.status === 'fulfilled').length, 1);
  const device = outcomes.find(x => x.status === 'fulfilled').value;
  assert.equal(store.authorize(device.credential).clientId, device.deviceId);
  assert.equal(store.enrollmentStatus(enrollment.enrollmentId).status, 'consumed');
  const disk = await readFile(path.join(directory, 'trust.json'), 'utf8');
  assert.ok(!disk.includes(device.credential) && !disk.includes(code.enrollmentToken));
  assert.equal((await stat(path.join(directory, 'trust.json'))).mode & 0o777, 0o600);
}));
test('expiry and explicit enrollment revocation reject without creating devices', async () => fixture(async (store, _directory, advance) => {
  const expired = store.createEnrollment({ endpoint, ttlSeconds: 1 }); advance(1001);
  await assert.rejects(store.enroll({ enrollmentToken: JSON.parse(expired.payload).enrollmentToken, name: 'Phone' }));
  assert.equal(store.enrollmentStatus(expired.enrollmentId).status, 'expired');
  const revoked = store.createEnrollment({ endpoint }); store.revokeEnrollment(revoked.enrollmentId);
  await assert.rejects(store.enroll({ enrollmentToken: JSON.parse(revoked.payload).enrollmentToken, name: 'Phone' }));
  assert.equal(store.listDevices().length, 0);
}));
test('trust survives restart while pending enrollments do not; rotating one device preserves another', async () => fixture(async (store, directory) => {
  const enroll = async name => store.enroll({ enrollmentToken: JSON.parse(store.createEnrollment({ endpoint }).payload).enrollmentToken, name });
  const phone = await enroll('Phone'); const other = await enroll('Other');
  const pending = JSON.parse(store.createEnrollment({ endpoint }).payload); const identity = [store.hostId, store.certificateSHA256];
  await store.close(); const restarted = await openTrustStore({ directory });
  try {
    assert.deepEqual([restarted.hostId, restarted.certificateSHA256], identity);
    assert.equal(restarted.authorize(phone.credential).deviceId, phone.deviceId);
    await assert.rejects(restarted.enroll({ enrollmentToken: pending.enrollmentToken, name: 'Phone' }));
    await restarted.rotateDevice(phone.deviceId);
    assert.equal(restarted.authorize(phone.credential), null); assert.equal(restarted.authorize(other.credential).deviceId, other.deviceId);
    await restarted.revokeDevice(other.deviceId); assert.equal(restarted.authorize(other.credential), null);
    assert.equal(restarted.authorize('legacy-shared-token-0000000000000000'), null);
  } finally { await restarted.close(); }
}));
test('unknown schema, identity mismatch and partial identity fail closed without overwriting state', async () => fixture(async (store, directory) => {
  await assert.rejects(openTrustStore({ directory }), /already|lock/i); await store.close();
  const file = path.join(directory, 'trust.json'); const original = JSON.parse(await readFile(file, 'utf8'));
  const unknown = JSON.stringify({ ...original, schemaVersion: 99 }); await writeFile(file, unknown, { mode: 0o600 });
  await assert.rejects(openTrustStore({ directory })); assert.equal(await readFile(file, 'utf8'), unknown);
  await writeFile(file, JSON.stringify({ ...original, certificateSHA256: '0'.repeat(64) })); await assert.rejects(openTrustStore({ directory }));
  await writeFile(file, JSON.stringify(original)); await rm(path.join(directory, 'identity-key.pem')); await assert.rejects(openTrustStore({ directory }));
}));
