import test from "node:test";
import assert from "node:assert/strict";
import WebSocket from "ws";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { serve } from "./network.js";
import { fileURLToPath } from "node:url";
const fixtureToken = "fixture-only-not-production-000000000000";
function phone(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, {
      headers: { Authorization: `Bearer ${fixtureToken}` },
    });
    const pending = new Map();
    let id = 0;
    ws.on("message", (x) => {
      const r = JSON.parse(x.toString());
      if (r.type === "receipt") {
        const p = pending.get(r.id);
        if (p) {
          pending.delete(r.id);
          r.ok ? p.resolve(r.data) : p.reject(new Error(r.error));
        }
      }
    });
    ws.once("error", reject);
    ws.once("open", () =>
      resolve({
        ws,
        call: (op, fields = {}) =>
          new Promise((resolve, reject) => {
            const key = String(++id) + crypto.randomUUID();
            pending.set(key, { resolve, reject });
            ws.send(JSON.stringify({ version: 1, id: key, op, ...fields }));
          }),
        close: () =>
          new Promise((resolve) => {
            ws.once("close", resolve);
            ws.close();
          }),
      }),
    );
  });
}
test("full fixture flow reconnects without replay, recovers queue on Stop and answers dialogs", async () => {
  const rpc = new Rpc(
    process.execPath,
    [fileURLToPath(new URL("./fixture.js", import.meta.url))],
    process.cwd(),
    process.env,
  );
  const c = new Controller(rpc, { list: async () => [] });
  await c.refresh();
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    token: fixtureToken,
  });
  let p;
  try {
    const url = `ws://127.0.0.1:${server.address().port}/v1`;
    p = await phone(url);
    const initial = await p.call("sync");
    assert.equal(
      (
        await p.call("send", {
          text: "/fixture-slow",
          mode: "send",
          epoch: initial.epoch,
        })
      ).disposition,
      "started",
    );
    let state = await p.call("sync");
    assert.equal(state.busy, true);
    assert.equal(
      (
        await p.call("send", {
          text: "Recover this follow-up",
          mode: "after",
          epoch: state.epoch,
        })
      ).disposition,
      "queued",
    );
    await p.close();
    p = await phone(url);
    state = await p.call("sync");
    assert.equal(state.messages.filter((m) => m.role === "user").length, 1);
    assert.equal(state.queue[0].text, "Recover this follow-up");
    const stopped = await p.call("stop", {
      epoch: state.epoch,
      runId: state.runId,
    });
    assert.deepEqual(stopped.followUp, ["Recover this follow-up"]);
    state = await p.call("sync");
    assert.equal(state.busy, false);
    assert.equal(state.queue.length, 0);
    assert.equal((await rpc.call("get_state")).isStreaming, false);
    await p.call("send", {
      text: "/fixture-confirm",
      mode: "send",
      epoch: state.epoch,
    });
    state = await p.call("sync");
    assert.equal(state.dialogs[0].method, "confirm");
    await p.close();
    p = await phone(url);
    state = await p.call("sync");
    await p.call("answer", { dialogId: state.dialogs[0].id, confirmed: true });
    state = await p.call("sync");
    assert.equal(state.dialogs.length, 0);
    assert.equal(state.busy, false);
    assert.ok(
      state.messages.some((m) => m.text === "Extension received: true"),
    );
  } finally {
    if (p?.ws.readyState === WebSocket.OPEN) await p.close();
    await server.shutdown();
    c.dispose();
    await rpc.close();
  }
});
