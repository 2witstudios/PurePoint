import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import WebSocket from "ws";
import { serve, allowedHost } from "./network.js";
const token = "test-only-token-not-a-real-credential-0000";
function connect(url, auth = token) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, {
      headers: { Authorization: `Bearer ${auth}` },
    });
    ws.once("open", () => resolve(ws));
    ws.once("error", reject);
  });
}
test("tailnet bind policy rejects wildcard and public hosts", () => {
  assert.equal(allowedHost("0.0.0.0"), false);
  assert.equal(allowedHost("192.168.1.2"), false);
  assert.equal(allowedHost("100.100.1.1"), true);
  assert.equal(allowedHost("127.0.0.1"), true);
});
test("authenticated socket, single controller and reconnect", async () => {
  const c = new EventEmitter();
  c.request = async (r) => ({ received: r.op });
  const server = await serve(c, { host: "127.0.0.1", port: 0, token });
  try {
    const url = `ws://127.0.0.1:${server.address().port}/v1`;
    await assert.rejects(connect(url, "wrong"));
    const ws = await connect(url);
    await assert.rejects(connect(url));
    const record = new Promise((resolve) =>
      ws.once("message", (x) => resolve(JSON.parse(x.toString()))),
    );
    ws.send(JSON.stringify({ version: 1, id: "x", op: "sync" }));
    assert.equal((await record).data.received, "sync");
    await new Promise((resolve) => {
      ws.once("close", resolve);
      ws.close();
    });
    const second = await connect(url);
    second.close();
  } finally {
    await server.shutdown();
  }
});
