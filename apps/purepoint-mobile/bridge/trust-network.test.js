import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter, once } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import https from 'node:https';
import WebSocket from 'ws';
import { serve } from './network.js';
import { openTrustStore } from './trust.js';
function connect(url, credential, clientId) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, { rejectUnauthorized: false, headers: { Authorization: `Bearer ${credential}`, 'X-PointGuard-Client-ID': clientId } });
    ws.once('open', () => resolve(ws)); ws.once('error', reject);
  });
}
function request(base, route, body, credential) {
  return new Promise((resolve, reject) => {
    const req = https.request(base + route, { method: body ? 'POST' : 'GET', rejectUnauthorized: false, headers: { ...(credential ? { Authorization: `Bearer ${credential}` } : {}), 'Content-Type': 'application/json' } }, res => {
      let data = ''; res.on('data', x => { data += x; }); res.on('end', () => resolve({ status: res.statusCode, body: data ? JSON.parse(data) : null }));
    }); req.on('error', reject); req.end(body ? JSON.stringify(body) : undefined);
  });
}
test('remote credentials bind ownership, reject admin rights and close revoked sockets without disturbing others', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'pg-network-')); const trust = await openTrustStore({ directory });
  const c = new EventEmitter(); let dispatched = 0;
  c.request = async r => { dispatched++; return { clientId: r.clientId }; };
  const server = await serve(c, { host: '127.0.0.1', port: 0, trust, tls: trust.tls });
  try {
    const base = `https://127.0.0.1:${server.address().port}`;
    const url = base.replace('https:', 'wss:') + '/v1';
    const code = JSON.parse(trust.createEnrollment({ endpoint: url }).payload);
    const response = await request(base, '/pair/enroll', { version: 1, enrollmentToken: code.enrollmentToken, name: 'Phone' });
    assert.equal(response.status, 200); const phone = response.body;
    const other = await trust.enroll({ enrollmentToken: JSON.parse(trust.createEnrollment({ endpoint: url }).payload).enrollmentToken, name: 'Other' });
    assert.equal((await request(base, '/pair/verify', null, phone.credential)).body.clientId, phone.deviceId);
    assert.equal((await request(base, '/admin/v1', { operation: 'providers' }, phone.credential)).status, 404);
    await assert.rejects(connect(url, 'local-admin-bootstrap-token-0000000000', phone.clientId));
    await assert.rejects(connect(url, phone.credential, other.clientId));
    const ws = await connect(url, phone.credential, phone.clientId); const second = await connect(url, other.credential, other.clientId);
    const receipt = once(ws, 'message'); ws.send(JSON.stringify({ version: 1, op: 'sync', id: 'spoof', clientId: other.clientId }));
    assert.equal(JSON.parse((await receipt)[0]).ok, false); assert.equal(dispatched, 0);
    const closed = once(ws, 'close'); await trust.revokeDevice(phone.deviceId); assert.equal((await closed)[0], 4001);
    assert.equal(second.readyState, WebSocket.OPEN);
    assert.equal((await request(base, '/pair/verify', null, phone.credential)).status, 401);
    await assert.rejects(connect(url, phone.credential, phone.clientId));
    const valid = once(second, 'message'); second.send(JSON.stringify({ version: 1, op: 'sync', id: 'valid', clientId: other.clientId }));
    assert.equal(JSON.parse((await valid)[0]).data.clientId, other.clientId);
    second.close();
  } finally { await server.shutdown(); await trust.close(); await rm(directory, { recursive: true, force: true }); }
});
test('legacy token option and plaintext remote trust cannot silently restore shared-token authorization', async () => {
  const c = new EventEmitter(); c.request = async () => ({});
  await assert.rejects(serve(c, { host: '127.0.0.1', port: 0, token: 'old-shared-token-00000000000000' }));
  await assert.rejects(serve(c, { host: '100.100.1.2', port: 0, localAdmin: { token: 'admin-token-00000000000000000000', clientId: 'desktop' } }));
});
