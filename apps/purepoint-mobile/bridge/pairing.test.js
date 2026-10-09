import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, stat, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pairingPayload, writePairingPage } from "./pairing.js";

test("given a tailnet endpoint and owner secret should create a portable pairing payload", () => {
  const secret = "fixture-pairing-secret-0123456789abcdef";
  assert.deepEqual(
    JSON.parse(pairingPayload("ws://100.94.14.74:8787/v1", secret)),
    {
      type: "pi-mobile-pairing",
      version: 1,
      endpoint: "ws://100.94.14.74:8787/v1",
      secret,
    },
  );
  assert.throws(() => pairingPayload("ws://example.com/v1", secret));
  assert.throws(() => pairingPayload("ws://100.94.14.74:8787/v1", "short"));
});

test("given a pairing page should save a private offline QR without plaintext credentials or remote resources", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-pairing-"));
  const file = path.join(dir, "pairing.html");
  const secret = "fixture-pairing-secret-0123456789abcdef";
  try {
    await writePairingPage(file, "ws://100.94.14.74:8787/v1", secret);
    const html = await readFile(file, "utf8");
    assert.match(html, /<svg/);
    assert.ok(!html.includes(secret));
    assert.ok(!html.includes("<script"));
    assert.ok(!html.includes('src="http'));
    assert.equal((await stat(file)).mode & 0o777, 0o600);
    await writePairingPage(file, "ws://100.94.14.74:8787/v1", secret);
    assert.equal((await stat(file)).mode & 0o777, 0o600);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
