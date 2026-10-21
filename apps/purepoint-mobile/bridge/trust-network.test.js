import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter, once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import https from "node:https";
import WebSocket from "ws";
import { serve } from "./network.js";
import { openTrustStore } from "./trust.js";
import { Controller } from "./controller.js";
function connect(url, credential, clientId) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, {
      rejectUnauthorized: false,
      headers: {
        Authorization: `Bearer ${credential}`,
        "X-PointGuard-Client-ID": clientId,
      },
    });
    ws.once("message", (bytes) => {
      assert.equal(JSON.parse(bytes.toString()).type, "permissions");
      resolve(ws);
    });
    ws.once("error", reject);
  });
}
function request(base, route, body, credential) {
  return new Promise((resolve, reject) => {
    const req = https.request(
      base + route,
      {
        method: body ? "POST" : "GET",
        rejectUnauthorized: false,
        headers: {
          ...(credential ? { Authorization: `Bearer ${credential}` } : {}),
          "Content-Type": "application/json",
        },
      },
      (res) => {
        let data = "";
        res.on("data", (x) => {
          data += x;
        });
        res.on("end", () =>
          resolve({
            status: res.statusCode,
            body: data ? JSON.parse(data) : null,
          }),
        );
      },
    );
    req.on("error", reject);
    req.end(body ? JSON.stringify(body) : undefined);
  });
}
test("remote credentials bind ownership, reject admin rights and close revoked sockets without disturbing others", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-network-"));
  const trust = await openTrustStore({ directory });
  const c = new EventEmitter();
  let dispatched = 0;
  c.request = async (r) => {
    dispatched++;
    return { clientId: r.clientId };
  };
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    trust,
    tls: trust.tls,
  });
  try {
    const base = `https://127.0.0.1:${server.address().port}`;
    const url = base.replace("https:", "wss:") + "/v1";
    const code = JSON.parse(trust.createEnrollment({ endpoint: url }).payload);
    const response = await request(base, "/pair/enroll", {
      version: 1,
      enrollmentToken: code.enrollmentToken,
      name: "Phone",
    });
    assert.equal(response.status, 200);
    const phone = response.body;
    const other = await trust.enroll({
      enrollmentToken: JSON.parse(
        trust.createEnrollment({ endpoint: url }).payload,
      ).enrollmentToken,
      name: "Other",
    });
    await trust.setTerminals(other.deviceId, "view");
    assert.equal(
      (await request(base, "/pair/verify", null, phone.credential)).body
        .clientId,
      phone.deviceId,
    );
    assert.equal(
      (
        await request(
          base,
          "/admin/v1",
          { operation: "providers" },
          phone.credential,
        )
      ).status,
      404,
    );
    await assert.rejects(
      connect(url, "local-admin-bootstrap-token-0000000000", phone.clientId),
    );
    await assert.rejects(connect(url, phone.credential, other.clientId));
    const ws = await connect(url, phone.credential, phone.clientId);
    const second = await connect(url, other.credential, other.clientId);
    const receipt = once(ws, "message");
    ws.send(
      JSON.stringify({
        version: 1,
        op: "sync",
        id: "spoof",
        clientId: other.clientId,
      }),
    );
    assert.equal(JSON.parse((await receipt)[0]).ok, false);
    assert.equal(dispatched, 0);
    const closed = once(ws, "close");
    await trust.revokeDevice(phone.deviceId);
    assert.equal((await closed)[0], 4001);
    assert.equal(second.readyState, WebSocket.OPEN);
    assert.equal(
      (await request(base, "/pair/verify", null, phone.credential)).status,
      401,
    );
    await assert.rejects(connect(url, phone.credential, phone.clientId));
    const valid = once(second, "message");
    second.send(
      JSON.stringify({
        version: 1,
        op: "sync",
        id: "valid",
        clientId: other.clientId,
      }),
    );
    assert.equal(JSON.parse((await valid)[0]).data.clientId, other.clientId);
    second.close();
  } finally {
    await server.shutdown();
    await trust.close();
    await rm(directory, { recursive: true, force: true });
  }
});
test("legacy token option and plaintext remote trust cannot silently restore shared-token authorization", async () => {
  const c = new EventEmitter();
  c.request = async () => ({});
  await assert.rejects(
    serve(c, {
      host: "127.0.0.1",
      port: 0,
      token: "old-shared-token-00000000000000",
    }),
  );
  await assert.rejects(
    serve(c, {
      host: "100.100.1.2",
      port: 0,
      localAdmin: {
        token: "admin-token-00000000000000000000",
        clientId: "desktop",
      },
    }),
  );
});

const chatReads = ["sync", "sessions", "history"];
const chatSends = ["send", "stop", "new", "resume", "answer"];
const tiers = {
  off: ["off", false],
  view: ["view", false],
  control: ["control", false],
  manage: ["control", true],
};
async function chatHarness(fn) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-chat-grant-"));
  const trust = await openTrustStore({ directory });
  const c = new EventEmitter();
  const dispatched = [];
  c.request = async (r) => {
    dispatched.push(r.op);
    return { op: r.op };
  };
  const server = await serve(c, {
    host: "127.0.0.1",
    port: 0,
    trust,
    tls: trust.tls,
  });
  const url = `wss://127.0.0.1:${server.address().port}/v1`;
  const grant = async (device, tier) => {
    const [terminals, management] = tiers[tier];
    await trust.setTerminals(device.deviceId, terminals);
    await trust.setWorkspaceManagement(device.deviceId, management);
  };
  // Without a tier the device keeps the grants it was paired with.
  const pair = async (tier) => {
    const device = await trust.enroll({
      enrollmentToken: JSON.parse(
        trust.createEnrollment({ endpoint: url }).payload,
      ).enrollmentToken,
      name: "Phone",
    });
    if (tier) await grant(device, tier);
    const ws = await connect(url, device.credential, device.clientId);
    const inbox = [];
    ws.on("message", (bytes) => inbox.push(JSON.parse(bytes.toString())));
    let next = 0;
    const call = async (op, fields = {}) => {
      const id = `${op}-${++next}`;
      const receipt = new Promise((resolve) => {
        const listener = (bytes) => {
          const r = JSON.parse(bytes.toString());
          if (r.type === "receipt" && r.id === id) {
            ws.off("message", listener);
            resolve(r);
          }
        };
        ws.on("message", listener);
      });
      ws.send(
        JSON.stringify({
          version: 1,
          id,
          op,
          clientId: device.clientId,
          ...fields,
        }),
      );
      return receipt;
    };
    return { device, ws, inbox, call };
  };
  try {
    await fn({ trust, c, dispatched, pair, grant });
  } finally {
    await server.shutdown();
    await trust.close();
    await rm(directory, { recursive: true, force: true });
  }
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 50));

test("chat follows the device grants: View reads, Control with Workspace management sends, everything else is rejected before the controller", async () =>
  chatHarness(async ({ dispatched, pair }) => {
    const expected = {
      off: [],
      view: chatReads,
      control: chatReads,
      manage: [...chatReads, ...chatSends],
    };
    const phones = [];
    for (const [tier, allowed] of Object.entries(expected)) {
      const phone = await pair(tier);
      phones.push(phone);
      assert.equal(
        (await phone.call("permissions")).data.chat,
        { off: "off", view: "read", control: "read", manage: "send" }[tier],
      );
      dispatched.length = 0;
      for (const op of [...chatReads, ...chatSends, "steer", "bogus"]) {
        const receipt = await phone.call(op);
        assert.equal(receipt.ok, allowed.includes(op), `${tier} ${op}`);
        if (!receipt.ok) assert.equal(receipt.data, undefined);
        if (!receipt.ok && chatSends.includes(op))
          assert.match(
            receipt.error,
            tier === "off" ? /View or Control/ : /Control and Workspace/,
          );
      }
      assert.deepEqual(dispatched, allowed, tier);
    }
    for (const p of phones) p.ws.close();
  }));

test("chat snapshots and editor offers reach only devices with View or Control", async () =>
  chatHarness(async ({ c, pair }) => {
    const off = await pair("off");
    const view = await pair("view");
    const manage = await pair("manage");
    c.emit("snapshot", { type: "snapshot", revision: 1 });
    c.emit("editor", { type: "editor", text: "draft" });
    await settle();
    const chat = (p) =>
      p.inbox.filter((r) => r.type === "snapshot" || r.type === "editor");
    assert.equal(chat(off).length, 0);
    assert.equal(chat(view).length, 2);
    assert.equal(chat(manage).length, 2);
    for (const p of [off, view, manage]) p.ws.close();
  }));

test("a live grant downgrade stops broadcasts, rejects further chat ops and notifies the device", async () =>
  chatHarness(async ({ trust, c, dispatched, pair, grant }) => {
    const phone = await pair("manage");
    assert.equal((await phone.call("permissions")).data.chat, "send");
    const downgrade = trust.setWorkspaceManagement(
      phone.device.deviceId,
      false,
    );
    assert.equal((await phone.call("send")).ok, false);
    await downgrade;
    await settle();
    assert.ok(
      phone.inbox.some((r) => r.type === "permissions" && r.chat === "read"),
    );
    assert.equal((await phone.call("sync")).ok, true);
    const off = trust.setTerminals(phone.device.deviceId, "off");
    c.emit("snapshot", { type: "snapshot", revision: 2 });
    await off;
    await settle();
    assert.equal(
      phone.inbox.filter((r) => r.type === "snapshot" && r.revision === 2)
        .length,
      0,
    );
    assert.ok(
      phone.inbox.some((r) => r.type === "permissions" && r.chat === "off"),
    );
    assert.equal((await phone.call("sync")).ok, false);
    assert.deepEqual(dispatched, ["sync"]);
    await grant(phone.device, "view");
    await settle();
    assert.equal((await phone.call("sync")).ok, true);
    phone.ws.close();
  }));

test("a device with agents and terminals Off cannot reach agents through Point Guard chat", async () =>
  chatHarness(async ({ dispatched, pair }) => {
    // A freshly paired device with no grant changes: terminals off, no management.
    const phone = await pair();
    const granted = (await phone.call("permissions")).data;
    assert.equal(granted.terminals, "off");
    assert.equal(granted.workspaceManagement, false);
    assert.equal(granted.chat, "off");
    assert.equal((await phone.call("agents")).ok, false);
    const attempts = [
      ["send", { text: "list all agents and kill agent X", mode: "send" }],
      ["send", { text: "pu send agent 'git push --force'", mode: "steer" }],
      ["answer", { dialogId: "confirm", confirmed: true }],
      ["resume", { sessionId: "other" }],
      ["history", { sessionId: "other" }],
      ["sessions", {}],
      ["sync", {}],
      ["new", {}],
      ["stop", { runId: "run" }],
    ];
    for (const [op, fields] of attempts)
      assert.equal((await phone.call(op, fields)).ok, false, op);
    assert.deepEqual(dispatched, []);
    phone.ws.close();
  }));

test("a downgrade while a chat request is in flight withholds its result", async () =>
  chatHarness(async ({ trust, c, pair }) => {
    let release;
    const held = new Promise((resolve) => (release = resolve));
    c.request = async (r) => {
      await held;
      return { op: r.op, transcript: "secret" };
    };
    const phone = await pair("manage");
    const sending = phone.call("send", { text: "hi", mode: "send" });
    const reading = phone.call("sync");
    await settle();
    // Read access remains, but the send result needs Control and Workspace management.
    await trust.setWorkspaceManagement(phone.device.deviceId, false);
    release();
    const sent = await sending;
    assert.equal(sent.ok, false);
    assert.equal(sent.data, undefined);
    assert.match(sent.error, /permission changed/);
    assert.equal((await reading).ok, true);
    let again;
    const later = new Promise((resolve) => (again = resolve));
    c.request = async (r) => {
      await later;
      return { op: r.op };
    };
    const syncing = phone.call("sync");
    await settle();
    await trust.setTerminals(phone.device.deviceId, "off");
    again();
    const synced = await syncing;
    assert.equal(synced.ok, false);
    assert.equal(synced.data, undefined);
    phone.ws.close();
  }));

test("a chat action queued behind a running prompt does not run after a downgrade", async () =>
  chatHarness(async ({ trust, c, pair }) => {
    const prompts = [];
    let release;
    const held = new Promise((resolve) => (release = resolve));
    const rpc = Object.assign(new EventEmitter(), {
      async call(op, data) {
        if (op === "get_state")
          return {
            sessionId: "one",
            model: { id: "test" },
            isStreaming: false,
          };
        if (op === "get_entries") return { entries: [], leafId: null };
        if (op === "prompt") {
          prompts.push(data?.message ?? data);
          if (prompts.length === 1) await held;
          return { disposition: "started" };
        }
        return {};
      },
      answer() {},
    });
    const controller = new Controller(rpc, {});
    await controller.refresh();
    c.request = (r, options) => controller.request(r, options);
    try {
      const phone = await pair("manage");
      const send = (text) =>
        phone.call("send", { text, mode: "send", epoch: controller.epoch });
      const first = send("first");
      const second = send("second");
      await settle();
      assert.equal(prompts.length, 1);
      await trust.setWorkspaceManagement(phone.device.deviceId, false);
      release();
      await first;
      const queued = await second;
      assert.equal(queued.ok, false);
      assert.match(queued.error, /permission changed/);
      assert.equal(prompts.length, 1, "queued prompt never reached the RPC");
      phone.ws.close();
    } finally {
      controller.dispose();
    }
  }));
