import { EventEmitter } from 'node:events';
import { mkdir, open, readFile, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { randomBytes, randomUUID, createHash, timingSafeEqual } from 'node:crypto';
import { ensureHostTLS, privateEntry } from './trust-tls.js';
import { pairingEndpoint } from './endpoints.js';
const opaque = () => randomBytes(32).toString('base64url');
const digest = value => createHash('sha256').update(value).digest('hex');
const validToken = value => typeof value === 'string' && /^[A-Za-z0-9_-]{43}$/.test(value);
const uuid = value => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
function same(left, right) { const a = Buffer.from(left, 'hex'); const b = Buffer.from(right, 'hex'); return a.length === b.length && timingSafeEqual(a, b); }
async function publish(file, value) {
  const temp = file + '.' + randomUUID() + '.tmp'; let handle;
  try { handle = await open(temp, 'wx', 0o600); await handle.writeFile(JSON.stringify(value)); await handle.sync(); await handle.close(); handle = null; await rename(temp, file); const directory = await open(path.dirname(file), 'r'); try { await directory.sync(); } finally { await directory.close(); } }
  finally { await handle?.close(); await rm(temp, { force: true }); }
}
function validate(state, pin) {
  if (state?.schemaVersion !== 1 || !uuid(state.hostId) || state.certificateSHA256 !== pin || !Array.isArray(state.devices) || state.devices.length > 256)
    throw new Error('Point Guard trust schema or host identity mismatch. Restore original state; do not downgrade or silently reset.');
  const ids = new Set(); const hashes = new Set();
  for (const device of state.devices) {
    if (!uuid(device.deviceId) || device.clientId !== device.deviceId || ids.has(device.deviceId) || !/^[a-f0-9]{64}$/.test(device.credentialHash) || hashes.has(device.credentialHash) || typeof device.name !== 'string' || !device.name.length || device.name.length > 80 || !Number.isSafeInteger(device.createdAt) || (device.revokedAt != null && !Number.isSafeInteger(device.revokedAt)))
      throw new Error('Point Guard device trust is malformed. Restore original private state.');
    ids.add(device.deviceId); hashes.add(device.credentialHash);
  }
}
/** One private durable writer. Unknown/legacy state never becomes remote authorization. */
export async function openTrustStore({ directory, now = Date.now }) {
  await mkdir(directory, { recursive: true, mode: 0o700 }); await privateEntry(directory, true);
  const lockFile = path.join(directory, 'writer.lock'); let lock;
  try { lock = await open(lockFile, 'wx', 0o600); await lock.writeFile(String(process.pid)); }
  catch (e) { if (e.code === 'EEXIST') throw new Error('Point Guard trust writer is already locked. Close the owning app; stale locks require deliberate recovery.'); throw e; }
  try {
    const tls = await ensureHostTLS(directory); const file = path.join(directory, 'trust.json'); let state;
    try { const info = await privateEntry(file); if (info.size > 256 * 1024) throw new Error('Point Guard trust state exceeds its limit.'); state = JSON.parse(await readFile(file, 'utf8')); }
    catch (e) { if (e.code !== 'ENOENT') throw e; state = { schemaVersion: 1, hostId: randomUUID(), certificateSHA256: tls.certificateSHA256, devices: [] }; await publish(file, state); }
    validate(state, tls.certificateSHA256);
    /** @type {EventEmitter & Record<string, any>} */
    const trust = new EventEmitter(); const enrollments = new Map(); let tail = Promise.resolve(); let closed = false;
    const active = () => { if (closed) throw new Error('Point Guard trust store is closed.'); };
    const serialized = fn => { const work = tail.then(() => { active(); return fn(); }); tail = work.catch(() => {}); return work; };
    Object.assign(trust, {
      hostId: state.hostId, certificateSHA256: tls.certificateSHA256, tls: { cert: tls.cert, key: tls.key },
      createEnrollment({ endpoint, ttlSeconds = 120 }) {
        active(); const url = pairingEndpoint(endpoint);
        // Endpoint validation is shared with the pairing encoder; caller adapter binds actual configured endpoint.
        if (!Number.isInteger(ttlSeconds) || ttlSeconds < 1 || ttlSeconds > 300) throw new Error('Pairing requires WSS /v1 and an enrollment lifetime of 1–300 seconds.');
        for (const [id, entry] of enrollments) if (entry.expiresAt + 300000 < now()) enrollments.delete(id);
        if (enrollments.size >= 64) throw new Error('Too many pending enrollments. Wait for expiry.');
        const enrollmentId = randomUUID(); const token = opaque(); const expiresAt = now() + ttlSeconds * 1000;
        enrollments.set(enrollmentId, { hash: digest(token), expiresAt, status: 'pending' });
        return { enrollmentId, expiresAt, payload: JSON.stringify({ type: 'pi-mobile-pairing', version: 2, endpoint: url.href, hostId: state.hostId, certificateSHA256: state.certificateSHA256, enrollmentToken: token, expiresAt }) };
      },
      enrollmentStatus(enrollmentId) {
        active(); const entry = enrollments.get(enrollmentId);
        if (!entry) return { enrollmentId, status: 'unknown' };
        if (entry.status === 'pending' && entry.expiresAt <= now()) entry.status = 'expired';
        return { enrollmentId, status: entry.status, expiresAt: entry.expiresAt };
      },
      revokeEnrollment(enrollmentId) { active(); const entry = enrollments.get(enrollmentId); if (entry?.status === 'pending') entry.status = 'revoked'; },
      authorize(credential) {
        if (closed || !validToken(credential)) return null; const hash = digest(credential);
        const device = state.devices.find(d => same(hash, d.credentialHash) && d.revokedAt == null);
        return device ? { deviceId: device.deviceId, clientId: device.clientId } : null;
      },
      enroll({ enrollmentToken, name }) {
        return serialized(async () => {
          if (!validToken(enrollmentToken) || typeof name !== 'string' || !name.trim() || name.length > 80 || /[\x00-\x1f\x7f]/.test(name)) throw new Error('Invalid device enrollment.');
          const hash = digest(enrollmentToken); const entry = [...enrollments.values()].find(e => same(hash, e.hash));
          if (!entry || entry.status !== 'pending' || entry.expiresAt <= now()) throw new Error('Enrollment expired, revoked or already used. Scan a new Mac QR code.');
          if (state.devices.length >= 256) throw new Error('Device trust is full. Deliberate trust maintenance required.');
          // Consume before awaiting disk: failed/uncertain enrollment must obtain a fresh QR, never replay.
          entry.status = 'consumed'; const credential = opaque(); const deviceId = randomUUID();
          const next = { ...state, devices: [...state.devices, { deviceId, clientId: deviceId, name: name.trim(), createdAt: now(), credentialHash: digest(credential) }] };
          await publish(file, next); state = next;
          return { version: 1, hostId: state.hostId, deviceId, clientId: deviceId, credential };
        });
      },
      listDevices() { active(); return state.devices.map(({ credentialHash, ...device }) => ({ ...device })); },
      revokeDevice(deviceId) {
        return serialized(async () => {
          if (!state.devices.some(d => d.deviceId === deviceId)) throw new Error('Unknown paired device.');
          const next = { ...state, devices: state.devices.map(d => d.deviceId === deviceId ? { ...d, revokedAt: now() } : d) };
          await publish(file, next); state = next; trust.emit('revoked', deviceId);
        });
      },
      rotateDevice(deviceId) { return trust.revokeDevice(deviceId); },
      async close() { if (closed) return; await tail; closed = true; enrollments.clear(); await lock.close(); await rm(lockFile); },
    });
    return trust;
  } catch (e) { await lock.close(); await rm(lockFile); throw e; }
}
