// A stand-in for apps/purepoint-mobile/bridge/main.js --managed (WIN-9.3 service tests).
// It follows the real handshake: tokens in the state directory, the owner admin HTTP server
// on the owner pipe, then the private admin.json readiness descriptor naming this pid and
// POINT_GUARD_INSTANCE_ID. stdin EOF (the app lifetime pipe) drains and exits.
//
// FAKE_PLATFORM_URL  file URL of the real bridge's platform.js (ownerPipeName)
// FAKE_MODE          ready (default) | crash | hang
// FAKE_ENV_OUT       write {argv, env} here first
// FAKE_CRASH_ONCE    a marker path: the first launch exits 7 shortly after ready
// FAKE_BUSY          1: runtime.stop is refused as busy
import { randomBytes, randomUUID } from 'node:crypto';
import { existsSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import os from 'node:os';
import path from 'node:path';

const env = process.env;
const state = env.POINT_GUARD_STATE_DIR;
const instanceId = env.POINT_GUARD_INSTANCE_ID;
const descriptor = path.join(state, 'admin.json');

if (env.FAKE_ENV_OUT) {
  writeFileSync(env.FAKE_ENV_OUT, JSON.stringify({ argv: process.argv.slice(2), env }));
}

const exit = (code) => {
  rmSync(descriptor, { force: true });
  process.exit(code);
};
process.stdin.on('end', () => exit(0));
process.stdin.on('close', () => exit(0));
process.stdin.resume();

if (env.FAKE_MODE === 'crash') {
  writeFileSync(
    path.join(state, 'error.json'),
    JSON.stringify({
      schemaVersion: 1,
      pid: process.pid,
      instanceId,
      code: 'fake',
      message: 'Fake startup failure.',
      recovery: 'Fix the fake.',
    }),
  );
  process.exit(3);
}

if (env.FAKE_MODE !== 'hang') {
  const token = randomBytes(32).toString('base64url');
  writeFileSync(path.join(state, 'admin-token'), token);
  writeFileSync(path.join(state, 'desktop-chat-token'), randomBytes(32).toString('base64url'));
  const { ownerPipeName } = await import(env.FAKE_PLATFORM_URL);
  const pipe = ownerPipeName(state);
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const send = (value, after) => {
      res.writeHead(200, { 'content-type': 'application/json', connection: 'close' });
      res.end(JSON.stringify(value), after);
    };
    if (req.headers.authorization !== `Bearer ${token}`)
      return send({ ok: false, error: { code: 'authorization', message: 'Owner authentication required.' } });
    const request = JSON.parse(Buffer.concat(chunks).toString());
    switch (request.operation) {
      case 'status':
        return send({ ok: true, result: { phase: 'ready', cwd: env.PI_MOBILE_CWD ?? os.homedir() } });
      case 'echo':
        return send({ ok: true, result: request });
      case 'runtime.stop':
        if (env.FAKE_BUSY === '1')
          return send({ ok: false, error: { code: 'busy', message: 'Point Guard is busy.' } });
        return send({ ok: true, result: {} }, () => setImmediate(() => exit(0)));
      default:
        return send({ ok: false, error: { code: 'operation', message: 'Unknown administrative operation.' } });
    }
  });
  await new Promise((resolve) => server.listen(pipe, resolve));
  writeFileSync(
    descriptor,
    JSON.stringify({
      schemaVersion: 1,
      contractVersion: 1,
      phase: 'ready',
      pid: process.pid,
      instanceId,
      adminSocket: pipe,
      nativeChatURL: 'ws://127.0.0.1:49152/v1',
      chatURL: null,
      desktopClientId: randomUUID(),
      hostId: 'fake-host',
      certificateSHA256: 'fake',
    }),
  );
  if (env.FAKE_CRASH_ONCE && !existsSync(env.FAKE_CRASH_ONCE)) {
    writeFileSync(env.FAKE_CRASH_ONCE, 'crashed');
    setTimeout(() => process.exit(7), 300);
  }
}
