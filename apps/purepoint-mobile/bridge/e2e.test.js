import test from "node:test";
import assert from "node:assert/strict";
import WebSocket from "ws";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { serve } from "./network.js";
import { fileURLToPath } from "node:url";
const fixtureToken = "fixture-only-not-production-000000000000";
const fixtureImage = {
  type: "image",
  mimeType: "image/png",
  data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aTz0AAAAASUVORK5CYII=",
};
function phone(url, clientId = "phone") {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, {
      headers: {
        Authorization: `Bearer ${fixtureToken}`,
        "X-PointGuard-Client-ID": clientId,
      },
    });
    const pending = new Map();
    let id = 0;
    let epoch;
    let snapshot;
    ws.on("message", (x) => {
      const r = JSON.parse(x.toString());
      if (r.type === "snapshot") {
        epoch = r.epoch;
        snapshot = r;
      }
      if (r.data?.epoch) epoch = r.data.epoch;
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
        get snapshot() {
          return snapshot;
        },
        call: (op, fields = {}) =>
          new Promise((resolve, reject) => {
            const key = String(++id) + crypto.randomUUID();
            pending.set(key, { resolve, reject });
            ws.send(
              JSON.stringify({
                version: 1,
                clientId,
                epoch,
                id: key,
                op,
                ...fields,
              }),
            );
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
test("given an image upload should reach the RPC child and survive reconnect without replay", async () => {
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
    const first = await p.call("sync");
    await p.call("send", {
      epoch: first.epoch,
      text: "/fixture-slow",
      mode: "send",
      images: [fixtureImage],
    });
    const native = await rpc.call("get_entries");
    assert.deepEqual(
      native.entries.find((e) => e.message?.role === "user").message.content[1],
      fixtureImage,
    );
    await p.close();
    p = await phone(url);
    const state = await p.call("sync");
    assert.equal(state.messages.filter((m) => m.role === "user").length, 1);
    assert.match(
      state.messages.find((m) => m.role === "user").text,
      /\[Image\]/,
    );
    await p.call("stop", { epoch: state.epoch, runId: state.runId });
  } finally {
    if (p?.ws.readyState === WebSocket.OPEN) await p.close();
    await server.shutdown();
    c.dispose();
    await rpc.close();
  }
});

const waitFor = async (predicate) => {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.fail("Both clients did not receive the authoritative update");
};

test("phone and desktop share streaming, queue ownership, dialogs, session races and independent reconnect", async () => {
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
  let p, d;
  try {
    const url = `ws://127.0.0.1:${server.address().port}/v1`;
    p = await phone(url, "phone");
    d = await phone(url, "desktop");
    const initial = await p.call("sync");
    await d.call("sync");
    await p.call("send", {
      text: "/fixture-slow",
      mode: "send",
      epoch: initial.epoch,
    });
    const running = await p.call("sync");
    await waitFor(
      () =>
        p.snapshot?.revision >= running.revision &&
        d.snapshot?.revision >= running.revision,
    );
    assert.deepEqual(p.snapshot, d.snapshot);
    assert.ok(d.snapshot.tools.some((tool) => tool.state === "running"));
    await assert.rejects(
      d.call("send", {
        text: "Racing idle send",
        mode: "send",
        epoch: initial.epoch,
      }),
      /running/,
    );
    await Promise.all([
      p.call("send", {
        text: "Identical queued text",
        mode: "after",
        epoch: running.epoch,
      }),
      d.call("send", {
        text: "Identical queued text",
        mode: "after",
        epoch: running.epoch,
      }),
    ]);
    const queued = await d.call("sync");
    assert.deepEqual(
      queued.queue.map((item) => item.clientId),
      ["phone", "desktop"],
    );
    assert.notEqual(queued.queue[0].id, queued.queue[1].id);
    await d.call("stop", { epoch: running.epoch, runId: running.runId });
    const stopped = await p.call("sync");
    assert.deepEqual(
      stopped.canceled.map((item) => item.clientId),
      ["phone", "desktop"],
    );
    assert.deepEqual(
      stopped.canceled.map((item) => item.id),
      queued.queue.map((item) => item.id),
    );
    await waitFor(
      () =>
        p.snapshot?.revision >= stopped.revision &&
        d.snapshot?.revision >= stopped.revision,
    );
    assert.deepEqual(p.snapshot, d.snapshot);
    await p.call("send", {
      text: "/fixture-confirm",
      mode: "send",
      epoch: stopped.epoch,
    });
    const dialog = await d.call("sync");
    const answers = await Promise.allSettled([
      p.call("answer", {
        epoch: dialog.epoch,
        dialogId: dialog.dialogs[0].id,
        confirmed: true,
      }),
      d.call("answer", {
        epoch: dialog.epoch,
        dialogId: dialog.dialogs[0].id,
        confirmed: false,
      }),
    ]);
    assert.equal(
      answers.filter((answer) => answer.status === "fulfilled").length,
      1,
    );
    assert.match(
      answers.find((answer) => answer.status === "rejected").reason.message,
      /already answered/,
    );
    const settled = await d.call("sync");
    const switches = await Promise.allSettled([
      p.call("new", { epoch: settled.epoch }),
      d.call("new", { epoch: settled.epoch }),
    ]);
    assert.equal(
      switches.filter((result) => result.status === "fulfilled").length,
      1,
    );
    assert.match(
      switches.find((result) => result.status === "rejected").reason.message,
      /Conversation changed/,
    );
    const switched = await d.call("sync");
    await waitFor(
      () =>
        p.snapshot?.epoch === switched.epoch &&
        d.snapshot?.epoch === switched.epoch,
    );
    assert.deepEqual(p.snapshot, d.snapshot);
    await p.close();
    await d.call("send", {
      epoch: switched.epoch,
      text: "/fixture-slow",
      mode: "send",
    });
    p = await phone(url, "phone");
    const reconnected = await p.call("sync");
    assert.equal(reconnected.busy, true);
    assert.equal(
      reconnected.messages.filter((message) => message.role === "user").length,
      1,
    );
    assert.equal(d.ws.readyState, WebSocket.OPEN);
    await p.call("stop", {
      epoch: reconnected.epoch,
      runId: reconnected.runId,
    });
    await assert.rejects(
      d.call("stop", { epoch: reconnected.epoch, runId: reconnected.runId }),
      /changed/,
    );
  } finally {
    for (const client of [p, d])
      if (client?.ws.readyState === WebSocket.OPEN) await client.close();
    await server.shutdown();
    c.dispose();
    await rpc.close();
  }
});
