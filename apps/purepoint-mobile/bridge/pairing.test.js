import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, stat, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pairingPayload, writePairingPage } from "./pairing.js";

const code = () => ({ type: "pi-mobile-pairing", version: 2, endpoint: "wss://100.94.14.74:8787/v1", hostId: "550e8400-e29b-41d4-a716-446655440000", certificateSHA256: "a".repeat(64), enrollmentToken: "b".repeat(43), expiresAt: Date.now() + 120000 });
test("one-time pinned enrollment encodes without shared-token fallback", () => {
  const payload = code();
  assert.deepEqual(JSON.parse(pairingPayload(payload)), payload);
  assert.throws(() => pairingPayload({ ...payload, endpoint: "wss://example.com/v1" }));
  assert.throws(() => pairingPayload({ ...payload, endpoint: "ws://100.94.14.74:8787/v1" }));
  assert.throws(() => pairingPayload({ ...payload, expiresAt: Date.now() - 1 }));
  assert.throws(() => pairingPayload({ ...payload, version: 1, secret: "legacy-shared-token-000000000000" }));
  assert.throws(() => pairingPayload("ws://100.94.14.74:8787/v1", "legacy-shared-token-000000000000"));
});
test("pairing page is private and offline; enrollment token never appears as plaintext", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-pairing-")); const file = path.join(dir, "pairing.html"); const payload = code();
  try {
    await writePairingPage(file, JSON.stringify(payload)); const html = await readFile(file, "utf8");
    assert.match(html, /<svg/); assert.ok(!html.includes(payload.enrollmentToken)); assert.ok(!html.includes("<script")); assert.ok(!html.includes('src="http'));
    assert.equal((await stat(file)).mode & 0o777, 0o600);
    await writePairingPage(file, payload); assert.equal((await stat(file)).mode & 0o777, 0o600);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
