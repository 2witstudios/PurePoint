import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import WebSocket from "ws";
import { serve, allowedHost, chatLevel } from "./network.js";
const token = "test-only-token-not-a-real-credential-0000";
function connect(url, auth = token, clientId = "desktop") {
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
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    localAdmin: { token, clientId: "desktop" },
  });
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
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    localAdmin: { token, clientId: "desktop" },
  });
  try {
    const base = `ws://127.0.0.1:${server.address().port}`;
    await assert.rejects(connect(base + "/unsupported"));
    await assert.rejects(connect(base + "/v1", token, ""));
    const ws = await connect(base + "/v1", token, "desktop");
    const receipt = new Promise((resolve) =>
      ws.once("message", (bytes) => resolve(JSON.parse(bytes.toString()))),
    );
    ws.send(
      JSON.stringify({
        version: 1,
        clientId: "phone",
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
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    localAdmin: { token, clientId: "desktop" },
  });
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

test("long-lived terminal discovery keeps polling beyond dedup window without reconnecting", async () => {
  const controller = new EventEmitter();
  controller.request = async () => ({});
  const server = await serve(controller, {
    host: "127.0.0.1",
    port: 0,
    localAdmin: { token, clientId: "desktop" },
  });
  const ws = await connect(`ws://127.0.0.1:${server.address().port}/v1`);
  const waiting = new Map();
  ws.on("message", (bytes) => {
    const record = JSON.parse(bytes.toString());
    if (record.type === "receipt") {
      const resolve = waiting.get(record.id);
      waiting.delete(record.id);
      resolve?.(record);
    }
  });
  const request = (id) =>
    new Promise((resolve) => {
      waiting.set(id, resolve);
      ws.send(
        JSON.stringify({
          version: 1,
          clientId: "desktop",
          id,
          op: "permissions",
        }),
      );
    });
  try {
    assert.equal((await request("oldest")).ok, true);
    for (let base = 0; base < 4200; base += 8) {
      const receipts = await Promise.all(
        Array.from({ length: 8 }, (_, offset) =>
          request(`poll-${base + offset}`),
        ),
      );
      assert.ok(receipts.every((record) => record.ok));
    }
    assert.equal(ws.readyState, WebSocket.OPEN);
    assert.equal(
      (await request("oldest")).ok,
      true,
      "expired ID leaves bounded recent window",
    );
    assert.equal(
      (await request("poll-4199")).ok,
      false,
      "recent duplicate is still fenced",
    );
  } finally {
    ws.terminate();
    await server.shutdown();
  }
});

test("chat level derives from the existing grants and fails closed on anything else", () => {
  const level = (terminals, workspaceManagement) =>
    chatLevel({ clientId: "phone", terminals, workspaceManagement });
  assert.equal(chatLevel(null), "off");
  assert.equal(level("off", false), "off");
  assert.equal(level("off", true), "off");
  assert.equal(level("view", false), "read");
  assert.equal(level("view", true), "read");
  assert.equal(level("control", false), "read");
  assert.equal(level("control", true), "send");
  assert.equal(level("control", "true"), "read");
  assert.equal(level("admin", true), "off");
  assert.equal(level(undefined, true), "off");
  assert.equal(chatLevel({ clientId: "desktop", owner: true }), "send");
  assert.equal(
    chatLevel({ clientId: "phone", owner: "true", terminals: "off" }),
    "off",
  );
});
