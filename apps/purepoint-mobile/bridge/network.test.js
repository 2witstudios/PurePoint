import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import WebSocket from "ws";
import { serve, allowedHost } from "./network.js";
const token = "test-only-token-not-a-real-credential-0000";
function connect(url, auth = token, clientId = crypto.randomUUID()) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, {
      headers: {
        Authorization: `Bearer ${auth}`,
        "X-PointGuard-Client-ID": clientId,
      },
    });
    ws.once("open", () => {
      ws.clientId = clientId;
      resolve(ws);
    });
    ws.once("error", reject);
  });
}
test("tailnet bind policy rejects wildcard and public hosts", () => {
  assert.equal(allowedHost("0.0.0.0"), false);
  assert.equal(allowedHost("192.168.1.2"), false);
  assert.equal(allowedHost("100.100.1.1"), true);
  assert.equal(allowedHost("127.0.0.1"), true);
});
test("authenticated concurrent clients share broadcasts and disconnect independently", async () => {
  const c = new EventEmitter();
  c.request = async (r) => ({ received: r.op });
  const server = await serve(c, { host: "127.0.0.1", port: 0, token });
  try {
    const url = `ws://127.0.0.1:${server.address().port}/v1`;
    await assert.rejects(connect(url, "wrong"));
    const ws = await connect(url);
    const desktop = await connect(url);
    const phoneUpdate = new Promise((resolve) =>
      ws.once("message", (bytes) => resolve(JSON.parse(bytes.toString()))),
    );
    const desktopUpdate = new Promise((resolve) =>
      desktop.once("message", (bytes) => resolve(JSON.parse(bytes.toString()))),
    );
    c.emit("snapshot", { type: "snapshot", revision: 42 });
    assert.deepEqual(await phoneUpdate, await desktopUpdate);
    const record = new Promise((resolve) =>
      ws.once("message", (x) => resolve(JSON.parse(x.toString()))),
    );
    ws.send(
      JSON.stringify({
        version: 1,
        clientId: ws.clientId,
        id: "x",
        op: "sync",
      }),
    );
    assert.equal((await record).data.received, "sync");
    await new Promise((resolve) => {
      ws.once("close", resolve);
      ws.close();
    });
    assert.equal(desktop.readyState, WebSocket.OPEN);
    desktop.close();
    const second = await connect(url);
    second.close();
  } finally {
    await server.shutdown();
  }
});

test("v1 rejects unsupported paths, missing device headers and identity spoofing", async () => {
  const c = new EventEmitter();
  let dispatched = 0;
  c.request = async () => {
    dispatched++;
    return {};
  };
  const server = await serve(c, { host: "127.0.0.1", port: 0, token });
  try {
    const base = `ws://127.0.0.1:${server.address().port}`;
    await assert.rejects(connect(base + "/unsupported"));
    await assert.rejects(connect(base + "/v1", token, ""));
    const ws = await connect(base + "/v1", token, "phone");
    const receipt = new Promise((resolve) =>
      ws.once("message", (bytes) => resolve(JSON.parse(bytes.toString()))),
    );
    ws.send(
      JSON.stringify({
        version: 1,
        clientId: "desktop",
        id: "spoof",
        op: "sync",
      }),
    );
    assert.equal((await receipt).ok, false);
    assert.equal(dispatched, 0);
    ws.close();
  } finally {
    await server.shutdown();
  }
});

test("concurrent clients are bounded and share one broadcaster without listener leaks", async () => {
  const c = new EventEmitter();
  c.request = async () => ({});
  const server = await serve(c, { host: "127.0.0.1", port: 0, token });
  const clients = [];
  try {
    const url = `ws://127.0.0.1:${server.address().port}/v1`;
    for (let index = 0; index < 32; index++) clients.push(await connect(url));
    await assert.rejects(connect(url), /503/);
    assert.equal(c.listenerCount("snapshot"), 1);
    assert.equal(c.listenerCount("editor"), 1);
    await Promise.all(
      clients.map(
        (ws) =>
          new Promise((resolve) => {
            ws.once("close", resolve);
            ws.close();
          }),
      ),
    );
    const replacement = await connect(url);
    replacement.close();
  } finally {
    await server.shutdown();
    assert.equal(c.listenerCount("snapshot"), 0);
    assert.equal(c.listenerCount("editor"), 0);
  }
});
