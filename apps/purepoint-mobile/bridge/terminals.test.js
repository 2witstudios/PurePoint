import test from "node:test";
import assert from "node:assert/strict";
import net from "node:net";
import { EventEmitter, once } from "node:events";
import { mkdtemp, mkdir, writeFile, rm, readFile } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";
import { createDaemons } from "./daemon.js";
import { terminalConnection } from "./terminals.js";
import { serve } from "./network.js";
import { openTrustStore } from "./trust.js";

async function until(fn) {
  for (let i = 0; i < 300; i++) {
    if (fn()) return;
    await delay(5);
  }
  throw new Error("Timed out waiting for requirement");
}
async function fixture(t) {
  const home = await mkdtemp(path.join(os.tmpdir(), "pt-"));
  const sockets = new Set();
  const attached = [];
  const inputs = [];
  const requests = [];
  let signalSnapshot = undefined;
  let phase = "input_ready",
    aware = true,
    fail = false,
    hold = false,
    version = 10;
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    socket.on("error", () => {});
    let pending = "";
    const send = (x) => socket.write(JSON.stringify(x) + "\n");
    socket.on("data", (data) => {
      pending += data.toString();
      let index;
      while ((index = pending.indexOf("\n")) >= 0) {
        const r = JSON.parse(pending.slice(0, index));
        pending = pending.slice(index + 1);
        requests.push(r);
        if (r.type === "health")
          send({ type: "health_report", protocol_version: version });
        if (r.type === "inventory")
          send({
            type: "inventory",
            complete: true,
            agents: [
              {
                id: "ag-one",
                name: "One",
                agent_type: "claude",
                state: "running",
                idle_seconds: 12,
                started_at: "2026-10-16T13:00:00Z",
                screen_aware: aware,
                phase,
                signal: signalSnapshot,
              },
            ],
          });
        if (r.type === "attach") {
          attached.push({ socket, request: r, send });
          send({ type: "attach_ready", buffered_bytes: 0 });
        }
        if (r.type === "input") {
          inputs.push({ r, socket });
          if (r.submit && !hold)
            send(
              fail
                ? { type: "error", message: "Echo mismatch" }
                : { type: "ok" },
            );
        }
      }
    });
  });
  await new Promise((resolve) =>
    server.listen(path.join(home, "daemon.sock"), resolve),
  );
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
    await rm(home, { recursive: true, force: true });
  });
  return {
    home,
    daemons: createDaemons({ puHome: home }),
    attached,
    inputs,
    requests,
    signal(value) { signalSnapshot = value; },
    phase(value) {
      phase = value;
    },
    aware(value) {
      aware = value;
    },
    fail(value) {
      fail = value;
    },
    hold(value) {
      hold = value;
    },
    version(value) {
      version = value;
    },
  };
}
function client(f, { queueLimit, slow = false, now = Date.now } = {}) {
  const records = [],
    callbacks = [];
  let permission = "control",
    management = false;
  const ws = {
    readyState: 1,
    bufferedAmount: 0,
    send(record, callback) {
      records.push(JSON.parse(record));
      if (slow) callbacks.push(callback);
      else queueMicrotask(() => callback());
    },
  };
  const connection = terminalConnection({
    ws,
    identity: { deviceId: "phone" },
    permission: () => permission,
    management: () => management,
    send: (r) => records.push(r),
    daemons: f.daemons,
    queueLimit,
    now,
  });
  return {
    connection,
    records,
    ws,
    callbacks,
    request: connection.request,
    changeWorkspace(value) {
      management = value;
      connection.changed();
    },
    change(value) {
      permission = value;
      connection.changed();
    },
  };
}

test("Unix daemon inventory includes offline registered machines, actual idle values and legacy replay warning", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  await writeFile(
    path.join(f.home, "machines.json"),
    JSON.stringify({
      version: 1,
      machines: [{ id: "offline", name: "Linux" }],
    }),
  );
  const inventory = await c.request({ op: "agents" });
  assert.equal(inventory.machines[0].agents[0].idleSeconds, 12);
  assert.equal(
    inventory.machines[0].agents[0].startedAt,
    "2026-10-16T13:00:00Z",
  );
  assert.equal(inventory.machines[1].online, false);
  assert.deepEqual(inventory.machines[1].agents, []);
  f.version(9);
  const opened = await c.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  assert.equal(opened.screenMayBeInexact, true);
  assert.deepEqual(f.attached[0].request, {
    type: "attach",
    agent_id: "ag-one",
  });
  c.change("off");
  const count = f.requests.length;
  await assert.rejects(c.request({ op: "agents" }), /permission/);
  assert.equal(
    f.requests.length,
    count,
    "off does not even inspect machine daemons",
  );
});

test("prompts report confirmed, sent, blocked and failed without waiting on unsafe screen phases", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  const prompt = () =>
    c.request({
      op: "agent.prompt",
      machine: "local",
      agentId: "ag-one",
      text: "Hello",
    });
  assert.deepEqual(await prompt(), { outcome: "confirmed", reason: null });
  assert.equal(f.inputs[0].r.cancel_on_disconnect, true);
  assert.equal(f.inputs[0].r.submit, true);
  f.aware(false);
  assert.deepEqual(await prompt(), { outcome: "sent", reason: null });
  f.aware(true);
  f.phase("dialog");
  assert.deepEqual(await prompt(), {
    outcome: "blocked",
    reason: "agent is waiting on a prompt, use the keys",
  });
  f.phase("has_draft");
  assert.equal((await prompt()).outcome, "blocked");
  f.phase("busy");
  assert.equal((await prompt()).outcome, "blocked");
  assert.equal(f.inputs.length, 2);
  f.phase("input_ready");
  f.fail(true);
  assert.deepEqual(await prompt(), {
    outcome: "failed",
    reason: "Echo mismatch",
  });
});

test("same-device attachments belong only to opening connection; keys cannot claim size", async (t) => {
  const f = await fixture(t),
    a = client(f),
    b = client(f);
  t.after(a.connection.dispose);
  t.after(b.connection.dispose);
  const opened = await a.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  f.attached[0].send({
    type: "pty_size",
    cols: 111,
    rows: 37,
    owner_client_id: "desktop",
  });
  await until(() => a.records.some((r) => r.type === "terminal.size"));
  assert.deepEqual(
    a.records.find((r) => r.type === "terminal.size"),
    { type: "terminal.size", streamId: opened.streamId, cols: 111, rows: 37 },
  );
  assert.equal(f.attached[0].request.replay, "snapshot");
  assert.equal(f.attached[0].request.client_id, `phone:${opened.streamId}`);
  for (const op of ["terminal.close", "terminal.keys", "terminal.raw"])
    await assert.rejects(
      b.request({ op, streamId: opened.streamId, key: "Enter", data: "x" }),
      /connection/,
    );
  const expectedKeys = {
    Esc: "1b",
    "Ctrl-C": "03",
    Tab: "09",
    Enter: "0d",
    ArrowUp: "1b5b41",
    ArrowDown: "1b5b42",
    ArrowLeft: "1b5b44",
    ArrowRight: "1b5b43",
    y: "79",
    n: "6e",
  };
  for (const key of Object.keys(expectedKeys))
    await a.request({ op: "terminal.keys", streamId: opened.streamId, key });
  await until(() => f.inputs.length === Object.keys(expectedKeys).length);
  assert.deepEqual(
    f.inputs.map(({ r }) => r.data),
    Object.values(expectedKeys),
  );
  assert.ok(!f.requests.some((r) => ["resize", "claim_size"].includes(r.type)));
  await assert.rejects(
    a.request({
      op: "terminal.keys",
      streamId: opened.streamId,
      key: "toString",
    }),
    /key/,
  );
  a.change("view");
  await assert.rejects(
    a.request({ op: "terminal.raw", streamId: opened.streamId, data: "no" }),
    /permission/,
  );
});

test("raw input preserves legacy mouse bytes and validates encoding and limits", async (t) => {
  const f = await fixture(t),
    a = client(f),
    b = client(f);
  t.after(a.connection.dispose);
  t.after(b.connection.dispose);
  const opened = await a.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  const input = {
    op: "terminal.raw",
    streamId: opened.streamId,
    encoding: "base64",
    data: Buffer.from([0x1b, 0x5b, 0x4d, 0x60, 0xa0, 0x80]).toString("base64"),
  };
  await a.request(input);
  await a.request({ ...input, data: "héllo", encoding: "utf8" });
  await until(() => f.inputs.length === 2);
  assert.equal(f.inputs[0].r.data, "1b5b4d60a080");
  assert.equal(f.inputs[1].r.data, Buffer.from("héllo").toString("hex"));
  for (const data of [
    "",
    "!!!",
    "YQ",
    "YQ==\n",
    Buffer.alloc(65537).toString("base64"),
  ])
    await assert.rejects(
      a.request({ ...input, data }),
      /Invalid terminal input/,
    );
  await assert.rejects(a.request({ ...input, encoding: "hex" }), /encoding/);
  await assert.rejects(b.request(input), /connection/);
  a.change("view");
  await assert.rejects(a.request(input), /permission/);
  assert.equal(f.inputs.length, 2);
});

test("explicit Control size claims go only to the stream's own attachment and survive reopen", async (t) => {
  const f = await fixture(t),
    a = client(f),
    b = client(f);
  t.after(a.connection.dispose);
  t.after(b.connection.dispose);
  const opened = await a.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  const claims = () => f.requests.filter((r) => r.type === "claim_size");
  for (const [cols, rows] of [
    [19, 30],
    [501, 30],
    [50, 7],
    [50, 301],
    [50.5, 30],
  ])
    await assert.rejects(
      a.request({ op: "terminal.size", streamId: opened.streamId, cols, rows }),
      /columns/,
    );
  await assert.rejects(
    b.request({
      op: "terminal.size",
      streamId: opened.streamId,
      cols: 50,
      rows: 30,
    }),
    /connection/,
  );
  assert.equal(claims().length, 0);
  await a.request({
    op: "terminal.size",
    streamId: opened.streamId,
    cols: 52,
    rows: 38,
  });
  await until(() => claims().length === 1);
  assert.deepEqual(claims()[0], { type: "claim_size", cols: 52, rows: 38 });
  // View-only reopen drops the claim (closing the attach releases ownership).
  a.change("view");
  await until(() => f.attached.length === 2);
  await assert.rejects(
    a.request({
      op: "terminal.size",
      streamId: opened.streamId,
      cols: 52,
      rows: 38,
    }),
    /permission/,
  );
  assert.equal(claims().length, 1);
  // Restored control re-asserts the remembered claim on the new attachment.
  a.change("control");
  await until(() => f.attached.length === 3 && claims().length === 2);
  assert.deepEqual(claims()[1], { type: "claim_size", cols: 52, rows: 38 });
});

test("legacy daemons reject phone size claims", async (t) => {
  const f = await fixture(t),
    a = client(f);
  t.after(a.connection.dispose);
  f.version(9);
  const opened = await a.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  await assert.rejects(
    a.request({
      op: "terminal.size",
      streamId: opened.streamId,
      cols: 52,
      rows: 38,
    }),
    /does not support/,
  );
  assert.ok(!f.requests.some((r) => r.type === "claim_size"));
});

test("downgrade immediately cancels in-flight submit socket and prevents subsequent input", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  f.hold(true);
  const pending = c.request({
    op: "agent.prompt",
    machine: "local",
    agentId: "ag-one",
    text: "Hello",
  });
  const rejected = assert.rejects(pending, /permission/);
  await until(() => f.inputs.length === 1);
  const socket = f.inputs[0].socket;
  c.change("view");
  await rejected;
  await until(() => socket.destroyed);
  await assert.rejects(
    c.request({
      op: "agent.prompt",
      machine: "local",
      agentId: "ag-one",
      text: "again",
    }),
    /permission/,
  );
  assert.equal(f.inputs.length, 1);
});

test("bounded stream backlog resets snapshot once writable, does not stall other viewers or loop forever", async (t) => {
  const f = await fixture(t),
    slow = client(f, { queueLimit: 20000, slow: true }),
    fast = client(f);
  t.after(slow.connection.dispose);
  t.after(fast.connection.dispose);
  await slow.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await fast.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 2);
  const burst = {
    type: "output",
    data: Buffer.alloc(100000, 120).toString("hex"),
  };
  f.attached[0].send(burst);
  f.attached[1].send({
    type: "output",
    data: Buffer.from("other viewer").toString("hex"),
  });
  await until(
    () =>
      slow.callbacks.length === 1 &&
      fast.records.some((r) => r.data === "other viewer"),
  );
  assert.equal(f.attached.length, 2, "recovery waits for real send callback");
  slow.callbacks.shift()();
  await until(() => f.attached.length === 3);
  assert.ok(slow.records.some((r) => r.type === "terminal.reset"));
  for (let i = 2; i < 4; i++) {
    f.attached[i].send(burst);
    await until(
      () =>
        slow.callbacks.length ||
        slow.records.some((r) => r.type === "terminal.closed"),
    );
    slow.callbacks.shift()?.();
    if (i === 2) await until(() => f.attached.length === 4);
  }
  await until(() => slow.records.some((r) => r.type === "terminal.closed"));
  assert.equal(f.attached.length, 4, "repeated overload is bounded");
});

test("incremental UTF-8 preserves split bytes and resync discards old partial sequence", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  await c.request({ op: "terminal.open", machine: "local", agentId: "ag-one" });
  await until(() => f.attached.length === 1);
  const send = f.attached[0].send;
  send({ type: "output", data: "e2" });
  send({ type: "output", data: "82ac" });
  send({ type: "output", data: "e2" });
  send({ type: "resync" });
  send({ type: "output", data: "4f4b" });
  await until(() => c.records.some((r) => r.data === "OK"));
  assert.deepEqual(
    c.records.filter((r) => r.type === "terminal.output").map((r) => r.data),
    ["€", "OK"],
  );
});

// Real TLS/WebSocket + Unix socket boundary, including durable trust changes.
test("authenticated WebSocket fences inventory/input on immediate trust downgrade and revocation", async (t) => {
  const f = await fixture(t);
  const trust = await openTrustStore({ directory: path.join(f.home, "trust") });
  t.after(() => trust.close());
  const controller = new EventEmitter();
  controller.request = async () => ({});
  const server = await serve(controller, {
    host: "127.0.0.1",
    port: 0,
    trust,
    tls: trust.tls,
    daemons: f.daemons,
    terminalQueueLimit: 20000,
  });
  t.after(server.shutdown);
  const url = `wss://127.0.0.1:${server.address().port}/v1`;
  const enrolled = await trust.enroll({
    name: "Phone",
    enrollmentToken: JSON.parse(
      trust.createEnrollment({ endpoint: url }).payload,
    ).enrollmentToken,
  });
  const open = async () => {
    const ws = new WebSocket(url, {
      rejectUnauthorized: false,
      headers: {
        Authorization: `Bearer ${enrolled.credential}`,
        "X-PointGuard-Client-ID": enrolled.clientId,
      },
    });
    const messages = [];
    ws.on("message", (b) => messages.push(JSON.parse(b.toString())));
    await once(ws, "open");
    let next = 0;
    return {
      ws,
      messages,
      async request(op, fields = {}) {
        const id = String(++next);
        ws.send(
          JSON.stringify({
            version: 1,
            id,
            op,
            clientId: enrolled.clientId,
            ...fields,
          }),
        );
        await until(() =>
          messages.some((r) => r.type === "receipt" && r.id === id),
        );
        return messages.find((r) => r.type === "receipt" && r.id === id);
      },
    };
  };
  await trust.setWorkspaceManagement(enrolled.deviceId, false);
  await trust.setTerminals(enrolled.deviceId, "off");
  const a = await open(),
    b = await open();
  t.after(() => {
    a.ws.terminate();
    b.ws.terminate();
  });
  assert.equal((await a.request("permissions")).data.terminals, "off");
  assert.equal(
    (await a.request("permissions")).data.workspaceManagement,
    false,
  );
  assert.equal((await a.request("agents")).ok, false);
  assert.equal(f.requests.length, 0);
  await trust.setTerminals(enrolled.deviceId, "control");
  const opened = (
    await a.request("terminal.open", { machine: "local", agentId: "ag-one" })
  ).data;
  assert.equal(
    (await b.request("terminal.raw", { streamId: opened.streamId, data: "x" }))
      .ok,
    false,
  );
  const otherStream = (
    await b.request("terminal.open", { machine: "local", agentId: "ag-one" })
  ).data;
  await until(() => f.attached.length === 2);
  f.attached[0].send({
    type: "output",
    data: Buffer.alloc(100000, 120).toString("hex"),
  });
  f.attached[1].send({
    type: "output",
    data: Buffer.from("independent viewer").toString("hex"),
  });
  await until(() =>
    a.messages.some(
      (r) => r.type === "terminal.reset" && r.streamId === opened.streamId,
    ),
  );
  await until(() => f.attached.length === 3);
  await until(() =>
    b.messages.some(
      (r) =>
        r.type === "terminal.output" &&
        r.streamId === otherStream.streamId &&
        r.data === "independent viewer",
    ),
  );
  assert.ok(!a.messages.some((r) => r.data === "independent viewer"));
  f.hold(true);
  const pending = a.request("agent.prompt", {
    machine: "local",
    agentId: "ag-one",
    text: "Hello",
  });
  await until(() => f.inputs.length === 1);
  const changed = trust.setTerminals(enrolled.deviceId, "view");
  assert.equal(
    trust.authorize(enrolled.credential).terminals,
    "view",
    "fence exists before disk completion",
  );
  await changed;
  assert.equal((await pending).ok, false);
  await until(() => f.inputs[0].socket.destroyed);
  assert.equal(
    (
      await a.request("terminal.keys", {
        streamId: opened.streamId,
        key: "Enter",
      })
    ).ok,
    false,
  );
  await trust.setTerminals(enrolled.deviceId, "off");
  await until(() =>
    a.messages.some(
      (r) => r.type === "terminal.closed" && r.streamId === opened.streamId,
    ),
  );
  const close = once(a.ws, "close");
  const revoke = trust.revokeDevice(enrolled.deviceId);
  assert.equal(trust.authorize(enrolled.credential), null);
  await revoke;
  assert.equal((await close)[0], 4001);
});

test("isolated output bursts recover repeatedly after stable drained progress and own close is idempotent", async (t) => {
  const f = await fixture(t);
  let clock = 0;
  const c = client(f, { queueLimit: 20000, slow: true, now: () => clock });
  t.after(c.connection.dispose);
  const opened = await c.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  for (let i = 0; i < 4; i++) {
    f.attached[i].send({
      type: "output",
      data: Buffer.alloc(100000, 120).toString("hex"),
    });
    await until(() => c.callbacks.length === 1);
    c.callbacks.shift()();
    await until(() => f.attached.length === i + 2);
    f.attached[i + 1].send({
      type: "output",
      data: Buffer.from("snapshot drained").toString("hex"),
    });
    await until(() => c.callbacks.length === 1);
    c.callbacks.shift()();
    clock += 6000;
  }
  assert.equal(c.records.filter((r) => r.type === "terminal.reset").length, 4);
  assert.ok(!c.records.some((r) => r.type === "terminal.closed"));
  const close = { op: "terminal.close", streamId: opened.streamId };
  assert.deepEqual(await c.request(close), {});
  assert.deepEqual(await c.request(close), {});
  assert.equal(c.records.filter((r) => r.type === "terminal.closed").length, 1);
});

test("maximum UTF8 prompt survives a delayed daemon receipt with full submitted bytes", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  f.hold(true);
  const text = "日本語\n".repeat(6553) + "abcdef";
  assert.equal(Buffer.byteLength(text), 65536);
  const pending = c.request({
    op: "agent.prompt",
    machine: "local",
    agentId: "ag-one",
    text,
  });
  await until(() => f.inputs.length === 1);
  assert.equal(Buffer.from(f.inputs[0].r.data, "hex").toString(), text);
  await delay(30);
  f.inputs[0].socket.write(JSON.stringify({ type: "ok" }) + "\n");
  assert.equal((await pending).outcome, "confirmed");
});

test("workspace-only grant events preserve held prompts and live attachments while terminal downgrade fences immediately", async (t) => {
  const f = await fixture(t),
    c = client(f);
  t.after(c.connection.dispose);
  const opened = await c.request({
    op: "terminal.open",
    machine: "local",
    agentId: "ag-one",
  });
  await until(() => f.attached.length === 1);
  f.hold(true);
  const delivery = c.request({
    op: "agent.prompt",
    machine: "local",
    agentId: "ag-one",
    text: "Keep this prompt",
  });
  await until(() => f.inputs.length === 1);
  let settled = false;
  delivery.then(
    () => {
      settled = true;
    },
    () => {
      settled = true;
    },
  );
  c.changeWorkspace(true);
  c.changeWorkspace(false);
  await delay(20);
  assert.equal(settled, false);
  assert.equal(f.inputs[0].socket.destroyed, false);
  assert.equal(f.attached.length, 1);
  assert.equal(f.attached[0].socket.destroyed, false);
  assert.equal(
    c.records.some((r) => r.type === "terminal.reset"),
    false,
  );
  f.attached[0].send({
    type: "output",
    data: Buffer.from("still attached").toString("hex"),
  });
  await until(() =>
    c.records.some(
      (r) => r.type === "terminal.output" && r.data === "still attached",
    ),
  );
  f.inputs[0].socket.write(JSON.stringify({ type: "ok" }) + "\n");
  assert.equal((await delivery).outcome, "confirmed");
  const pending = c.request({
    op: "agent.prompt",
    machine: "local",
    agentId: "ag-one",
    text: "Cancel on actual downgrade",
  });
  await until(() => f.inputs.length === 2);
  c.change("view");
  await assert.rejects(pending, /permission/);
  await until(() => f.inputs[1].socket.destroyed);
  await until(() => f.attached[0].socket.destroyed);
  assert.equal(
    c.records.some(
      (r) => r.type === "terminal.reset" && r.streamId === opened.streamId,
    ),
    true,
  );
});

test("additive authoritative signal forwards exact CORE inventory facts without affecting terminal readiness", async (t) => {
  const producer = JSON.parse(await readFile(new URL("../../../crates/pu-core/tests/fixtures/signals-inventory-v1.json", import.meta.url), "utf8"));
  const f = await fixture(t), c = client(f);
  t.after(c.connection.dispose);
  f.signal(producer.agents[0].signal);
  const inventory = await c.request({ op: "agents" });
  assert.deepEqual(inventory.machines[0].agents[0].signal, producer.agents[0].signal);
  assert.equal(inventory.machines[0].agents[0].phase, "input_ready");
  f.signal(undefined);
  const legacy = await c.request({ op: "agents" });
  assert.equal(legacy.machines[0].agents[0].signal, null);
});
