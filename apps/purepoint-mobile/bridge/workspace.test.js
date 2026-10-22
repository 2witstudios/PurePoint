import test from "node:test";
import assert from "node:assert/strict";
import net from "node:net";
import { StringDecoder } from "node:string_decoder";
import { EventEmitter, once } from "node:events";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import WebSocket from "ws";
import { createDaemons } from "./daemon.js";
import { workspaceConnection, projectMetadata } from "./workspace.js";
import { terminalConnection } from "./terminals.js";
import { openTrustStore } from "./trust.js";
import { serve } from "./network.js";

async function fixture(t) {
  const home = await mkdtemp(path.join(os.tmpdir(), "mw-"));
  const events = new EventEmitter();
  const sockets = new Set(),
    requests = [],
    agents = [];
  let held = null,
    hold = false,
    partial = false,
    legacy = false,
    drop = false,
    holdMetadata = false,
    protocolVersion = 10,
    catalogKeys = ["custom"],
    presetType = "custom";
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    socket.on("error", () => {});
    let pending = "";
    const decoder = new StringDecoder("utf8");
    const send = (value) => {
      const write = () => socket.write(JSON.stringify(value) + "\n");
      if (
        hold &&
        ["rename_result", "kill_result", "create_worktree_result"].includes(
          value.type,
        )
      )
        held = write;
      else write();
    };
    socket.on("data", (bytes) => {
      pending += decoder.write(bytes);
      let index;
      while ((index = pending.indexOf("\n")) >= 0) {
        const r = JSON.parse(pending.slice(0, index));
        pending = pending.slice(index + 1);
        requests.push(r);
        events.emit(r.type, r);
        if (r.type === "health")
          send({ type: "health_report", protocol_version: protocolVersion });
        else if (r.type === "inventory") {
          if (holdMetadata) continue;
          if (partial && r.kind === "worktrees")
            send({ type: "error", message: "old daemon" });
          else
            send({
              type: "inventory",
              complete: true,
              projects: [
                {
                  project_root: "/registered",
                  name: "Empty supported project",
                  available: true,
                  counts: { agents: agents.length },
                },
              ],
              worktrees: [
                {
                  project_root: "/registered",
                  id: "wt-one",
                  name: "One",
                  path: "/registered/.pu/worktrees/wt-one",
                  branch: "pu/one",
                  status: "active",
                  agent_ids: agents
                    .filter((a) => a.worktree_id)
                    .map((a) => a.id),
                },
              ],
              agents,
            });
        } else if (r.type === "get_config")
          send({
            type: "config_report",
            agents: catalogKeys.map((id) => ({
              name: "My Agent",
              ...(legacy ? {} : { id, available: true }),
              command: "secret-command",
              launch_args: ["secret-arg"],
            })),
            ...(legacy ? {} : { terminal_available: true }),
          });
        else if (r.type === "list_agent_defs")
          send({
            type: "agent_def_list",
            agent_defs: [
              {
                name: "review",
                agent_type: presetType,
                inline_prompt: "Review carefully\n\tRésumé",
              },
              {
                name: "unsafe",
                agent_type: presetType,
                command: "do-not-expose",
              },
            ],
          });
        else if (r.type === "spawn") {
          const run = () => {
            const a = {
              id: `ag-${agents.length + 1}`,
              name: r.name ?? "Terminal",
              agent_type: r.agent,
              project_root: r.project_root,
              worktree_id: r.worktree ?? null,
              state: "running",
              started_at: "2026-10-16T00:00:00Z",
            };
            agents.push(a);
            if (drop) socket.destroy();
            else
              send({
                type: "spawn_result",
                agent_id: a.id,
                worktree_id: a.worktree_id,
              });
          };
          if (hold) held = run;
          else run();
        } else if (r.type === "rename") {
          agents.find((a) => a.id === r.agent_id).name = r.name;
          send({ type: "rename_result", agent_id: r.agent_id });
        } else if (r.type === "kill")
          send({ type: "kill_result", killed: [r.target.agent] });
        else if (r.type === "create_worktree")
          send({ type: "create_worktree_result", worktree_id: "wt-created" });
        else if (r.type === "remove_project")
          send({
            type: "project_removed",
            project_root: r.project_root,
            removed: true,
            suspended: [],
          });
      }
    });
  });
  await new Promise((resolve) =>
    server.listen(path.join(home, "daemon.sock"), resolve),
  );
  t.after(async () => {
    for (const s of sockets) s.destroy();
    await new Promise((resolve) => server.close(resolve));
    await rm(home, { recursive: true, force: true });
  });
  const daemons = createDaemons({ puHome: home });
  let terminals = "control",
    management = true;
  const ledger = new Map();
  const connect = (deviceId = "phone") =>
    workspaceConnection({
      identity: { deviceId },
      permission: () => terminals,
      management: () => management,
      ledger,
      daemons,
    });
  return {
    home,
    events,
    daemons,
    requests,
    agents,
    ledger,
    connect,
    catalog(keys, type = "custom") {
      catalogKeys = keys;
      presetType = type;
    },
    holdMetadata(v) {
      holdMetadata = v;
    },
    protocolVersion(v) {
      protocolVersion = v;
    },
    partial(v) {
      partial = v;
    },
    legacy(v) {
      legacy = v;
    },
    hold(v) {
      hold = v;
    },
    drop(v) {
      drop = v;
    },
    release() {
      held?.();
    },
    grant(t, m) {
      terminals = t;
      management = m;
    },
  };
}
const form = (extra = {}) => ({
  op: "workspace.terminal",
  operationId: "request-0001",
  machine: "local",
  projectId: "/registered",
  target: "root",
  ...extra,
});

async function until(fn) {
  for (let i = 0; i < 300; i++) {
    if (fn()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw Error("Requirement timed out");
}

test("registered empty projects, partial metadata, offline machines and malformed names stay distinct", async (t) => {
  const f = await fixture(t);
  assert.equal(
    (await projectMetadata((await f.daemons.machines())[0])).projects.length,
    1,
  );
  f.partial(true);
  assert.equal(
    (await projectMetadata((await f.daemons.machines())[0])).metadataStatus,
    "partial",
  );
  await writeFile(
    path.join(f.home, "machines.json"),
    JSON.stringify({
      machines: [
        null,
        { id: "no-name" },
        { id: "no-name", name: "Duplicate must not replace first" },
        { id: "local", name: "Cannot replace built-in local" },
        { id: "wrong-name", name: 42 },
        { id: "empty-name", name: " " },
        { id: "bad/id", name: "Bad" },
      ],
    }),
  );
  assert.deepEqual(
    (await f.daemons.machines()).slice(1).map((m) => m.name),
    ["no-name", "wrong-name", "empty-name"],
  );
  const c = terminalConnection({
    ws: { readyState: 1 },
    identity: { deviceId: "phone" },
    permission: () => "view",
    send() {},
    daemons: f.daemons,
  });
  t.after(c.dispose);
  const inventory = await c.request({ op: "agents" });
  assert.equal(inventory.machines[0].metadataStatus, "partial");
  assert.equal(inventory.machines[1].metadataStatus, "offline");
  assert.equal(
    new Set(inventory.machines.map((m) => m.id)).size,
    inventory.machines.length,
  );
  assert.equal(inventory.machines.length, 4);
  assert.equal((await f.daemons.resolve("no-name")).name, "no-name");
});

test("canonical catalog strips config secrets and command presets; old daemon capability explicit", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  const result = await c.request({
    op: "workspace.catalog",
    machine: "local",
    projectId: "/registered",
  });
  assert.deepEqual(result.agents, [
    { id: "custom", name: "My Agent", available: true },
  ]);
  assert.equal(result.presets.length, 1);
  assert.equal(JSON.stringify(result).includes("secret"), false);
  f.legacy(true);
  await assert.rejects(
    c.request({
      op: "workspace.catalog",
      machine: "local",
      projectId: "/registered",
    }),
    /workspace_unsupported/,
  );
});

test("scoped root and worktree terminals use safe Spawn and appear in matching inventory", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  for (const target of ["root", "worktree"]) {
    const result = await c.request(
      form({
        operationId: `request-${target}`,
        target,
        ...(target === "worktree" ? { worktreeId: "wt-one" } : {}),
      }),
    );
    assert.equal(result.state, "succeeded");
  }
  assert.deepEqual(
    f.agents.map((a) => [a.project_root, a.worktree_id]),
    [
      ["/registered", null],
      ["/registered", "wt-one"],
    ],
  );
  const mutations = f.requests.filter((r) => r.type === "spawn");
  assert.equal(mutations.length, 2);
  assert.equal(
    mutations.every(
      (r) => r.agent === "terminal" && !r.command && !r.extra_args && !r.env,
    ),
    true,
  );
  assert.equal(
    (await projectMetadata((await f.daemons.machines())[0])).worktrees[0]
      .agentIds[0],
    f.agents[1].id,
  );
});

test("all management fields are bounded and project/machine/agent membership is authoritative", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  for (const [i, extra] of [
    { machine: "another" },
    { projectId: "/other" },
    { target: "worktree", worktreeId: "wt-other" },
    { command: "arbitrary" },
    {
      op: "workspace.stop",
      target: undefined,
      agentId: "ag-other",
      confirm: true,
    },
    { op: "workspace.spawn", agentType: "My Agent", prompt: "Do work" },
  ].entries()) {
    const r = { ...form({ operationId: `invalid-${i}`, ...extra }) };
    if (r.target === undefined) delete r.target;
    try {
      const result = await c.request(r);
      assert.equal(result.state, "failed");
    } catch (error) {
      assert.match(error.message, /workspace_invalid/);
    }
  }
  assert.equal(
    f.requests.some((r) => ["spawn", "kill"].includes(r.type)),
    false,
  );
});

test("ledger deduplicates across reconnect, isolates devices, rejects payload mismatch and never replays uncertain commit", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  f.hold(true);
  const pending = c.request(form());
  await until(() => f.requests.some((r) => r.type === "spawn"));
  await assert.rejects(
    c.request(form({ id: "different-envelope" })),
    /workspace_duplicate/,
  );
  await assert.rejects(
    c.request(form({ target: "worktree", worktreeId: "wt-one" })),
    /workspace_duplicate/,
  );
  assert.equal(
    (
      await c.request({
        op: "workspace.operation",
        operationId: "request-0001",
      })
    ).state,
    "pending",
  );
  c.dispose();
  assert.equal(
    await pending.then((r) => r.state).catch(() => "fenced"),
    "fenced",
  );
  f.drop(true);
  f.release();
  await until(() => f.agents.length === 1);
  const reconnected = f.connect();
  t.after(reconnected.dispose);
  assert.equal((await reconnected.request(form())).state, "uncertain");
  assert.equal(f.requests.filter((r) => r.type === "spawn").length, 1);
  assert.equal(
    (
      await f
        .connect("other")
        .request({ op: "workspace.operation", operationId: "request-0001" })
    ).state,
    "unknown",
  );
  const restarted = workspaceConnection({
    identity: { deviceId: "phone" },
    permission: () => "control",
    management: () => true,
    ledger: new Map(),
    daemons: f.daemons,
  });
  t.after(restarted.dispose);
  assert.equal(
    (
      await restarted.request({
        op: "workspace.operation",
        operationId: "request-0001",
      })
    ).state,
    "unknown",
  );
});

test("downgrade fences held pre-dispatch metadata and post-dispatch results without claiming rollback", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.hold(true);
  const pending = c.request(form());
  await until(() => f.requests.some((r) => r.type === "spawn"));
  f.grant("view", false);
  c.changed();
  const result = await pending;
  assert.equal(result.state, "uncertain");
  assert.equal(result.detail, undefined);
  await assert.rejects(
    c.request(form({ operationId: "another-01" })),
    /workspace_permission/,
  );
  f.release();
  assert.equal(
    (
      await c.request({
        op: "workspace.operation",
        operationId: "request-0001",
      })
    ).detail,
    undefined,
  );
  f.grant("off", false);
  await assert.rejects(
    c.request({ op: "workspace.operation", operationId: "request-0001" }),
    /workspace_permission/,
  );
});

test("permission loss before metadata resolves prevents all mutation dispatch", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.holdMetadata(true);
  const pending = c.request(form());
  await until(() => f.requests.some((r) => r.type === "inventory"));
  f.grant("view", false);
  c.changed();
  assert.equal((await pending).state, "failed");
  assert.equal(
    f.requests.some((r) => r.type === "spawn"),
    false,
  );
});

test("committed daemon mutation with lost receipt remains uncertain without automatic replay", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.drop(true);
  const result = await c.request(form());
  assert.equal(result.state, "uncertain");
  assert.equal(f.agents.length, 1);
  const reconnect = f.connect();
  t.after(reconnect.dispose);
  assert.equal((await reconnect.request(form())).state, "uncertain");
  assert.equal(f.requests.filter((r) => r.type === "spawn").length, 1);
});

test("expanded presets bounded, validated rename and exact single-agent stop", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  let r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0001",
      agentType: "custom",
      prompt: "x".repeat(65536),
      presetId: "review",
    }),
  );
  assert.equal(r.state, "failed");
  r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0002",
      agentType: "custom",
      prompt: "Do work\n\t日本語",
      presetId: "review",
    }),
  );
  assert.equal(r.state, "succeeded");
  r = await c.request({
    op: "workspace.rename",
    operationId: "rename-001",
    machine: "local",
    projectId: "/registered",
    agentId: r.detail.agentId,
    name: "Renamed",
  });
  assert.equal(r.state, "succeeded");
  assert.equal(f.agents[0].name, "Renamed");
  r = await c.request({
    op: "workspace.stop",
    operationId: "stop-00001",
    machine: "local",
    projectId: "/registered",
    agentId: f.agents[0].id,
    confirm: true,
  });
  assert.equal(r.state, "succeeded");
  assert.deepEqual(f.requests.find((x) => x.type === "kill").target, {
    agent: f.agents[0].id,
  });
});

test("spawn instructions are optional; blank launches bare and plan mode needs instructions or a preset", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  let r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0101",
      agentType: "custom",
      prompt: "",
    }),
  );
  assert.equal(r.state, "succeeded");
  assert.equal(f.requests.at(-1).prompt, "");
  r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0102",
      agentType: "custom",
      prompt: " \n\t",
    }),
  );
  assert.equal(r.state, "succeeded");
  assert.equal(f.requests.at(-1).prompt, "");
  const spawns = f.requests.filter((q) => q.type === "spawn").length;
  r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0105",
      agentType: "custom",
      prompt: " ".repeat(65537),
    }),
  );
  assert.equal(r.state, "failed");
  assert.equal(f.requests.filter((q) => q.type === "spawn").length, spawns);
  r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0103",
      agentType: "custom",
      prompt: "",
      planMode: true,
    }),
  );
  assert.equal(r.state, "failed");
  assert.equal(f.requests.filter((q) => q.type === "spawn").length, spawns);
  r = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "spawn-0104",
      agentType: "custom",
      prompt: "",
      planMode: true,
      presetId: "review",
    }),
  );
  assert.equal(r.state, "succeeded");
  assert.match(f.requests.at(-1).prompt, /\S/);
});

test("real authenticated TLS WS old grants deny management, off leaks no fleet, downgrade events fence and reconnect reconciles", async (t) => {
  const f = await fixture(t),
    directory = path.join(f.home, "trust");
  const trust = await openTrustStore({ directory });
  t.after(() => trust.close());
  const controller = new EventEmitter();
  controller.request = async () => ({ chatReady: true });
  const server = await serve(controller, {
    host: "127.0.0.1",
    port: 0,
    trust,
    tls: trust.tls,
    daemons: f.daemons,
  });
  t.after(() => server.shutdown());
  const url = `wss://127.0.0.1:${server.address().port}/v1`;
  const phone = await trust.enroll({
    enrollmentToken: JSON.parse(
      trust.createEnrollment({ endpoint: url }).payload,
    ).enrollmentToken,
    name: "Phone",
  });
  await trust.setWorkspaceManagement(phone.deviceId, false);
  await trust.setTerminals(phone.deviceId, "off");
  let sequence = 0;
  const connect = async () => {
    const ws = new WebSocket(url, {
      rejectUnauthorized: false,
      headers: {
        Authorization: `Bearer ${phone.credential}`,
        "X-PointGuard-Client-ID": phone.clientId,
      },
    });
    const messages = [];
    ws.on("message", (b) => messages.push(JSON.parse(b.toString())));
    await once(ws, "open");
    t.after(() => ws.terminate());
    const request = async (fields) => {
      const id = `transport-${++sequence}`;
      ws.send(
        JSON.stringify({ version: 1, clientId: phone.clientId, id, ...fields }),
      );
      await until(() =>
        messages.some((m) => m.type === "receipt" && m.id === id),
      );
      return messages.find((m) => m.type === "receipt" && m.id === id);
    };
    return { ws, messages, request };
  };
  let c = await connect();
  assert.equal((await c.request({ op: "agents" })).ok, false);
  assert.equal(f.requests.length, 0);
  await trust.setTerminals(phone.deviceId, "control");
  assert.equal(trust.authorize(phone.credential).workspaceManagement, false);
  assert.equal((await c.request(form())).ok, false);
  assert.equal(
    f.requests.some((r) => r.type === "spawn"),
    false,
  );
  await trust.setWorkspaceManagement(phone.deviceId, true);
  const launched = await c.request(form());
  assert.equal(launched.data.state, "succeeded");
  c.ws.terminate();
  c = await connect();
  assert.equal((await c.request(form())).data.state, "succeeded");
  assert.equal(f.agents.length, 1);
  await trust.setWorkspaceManagement(phone.deviceId, false);
  assert.equal(
    (
      await c.request({
        op: "workspace.operation",
        operationId: "request-0001",
      })
    ).data.detail,
    undefined,
  );
  assert.equal(
    (await c.request(form({ operationId: "denied-002" }))).ok,
    false,
  );
  await trust.setWorkspaceManagement(phone.deviceId, true);
  f.hold(true);
  const held = c.request(form({ operationId: "held-00001" }));
  await until(() => f.requests.filter((r) => r.type === "spawn").length === 2);
  await trust.setWorkspaceManagement(phone.deviceId, false);
  const heldReceipt = await held;
  assert.equal(heldReceipt.data.state, "uncertain");
  assert.equal(heldReceipt.data.detail, undefined);
  f.release();
  assert.equal(f.agents.length, 2);
  await trust.setWorkspaceManagement(phone.deviceId, true);
  assert.equal(
    (await c.request(form({ operationId: "held-00001" }))).data.state,
    "uncertain",
  );
  assert.equal(f.requests.filter((r) => r.type === "spawn").length, 2);
  assert.equal((await c.request({ op: "sync" })).data.chatReady, true);
});

test("opaque bounded config keys preserve dotted, spaced and Unicode agent types in catalog, presets and fixed launches", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  const keys = [
    "custom",
    "claude.v2",
    "custom agent",
    "自定义代理",
    " spaced ",
    "é".repeat(64),
  ];
  f.catalog(keys, "custom agent");
  const catalog = await c.request({
    op: "workspace.catalog",
    machine: "local",
    projectId: "/registered",
  });
  assert.deepEqual(
    catalog.agents.map((a) => a.id),
    keys,
  );
  assert.equal(catalog.terminalAvailable, true);
  assert.equal(catalog.presets[0].agentType, "custom agent");
  for (const [index, agentType] of keys.entries()) {
    const result = await c.request(
      form({
        op: "workspace.spawn",
        operationId: `opaque-${index}`,
        agentType,
        prompt: "Do work",
        ...(agentType === "custom agent" ? { presetId: "review" } : {}),
      }),
    );
    assert.equal(result.state, "succeeded");
  }
  const terminal = await c.request(form({ operationId: "opaque-terminal" }));
  assert.equal(terminal.state, "succeeded");
  const requests = f.requests.filter((r) => r.type === "spawn");
  assert.deepEqual(
    requests.map((r) => r.agent),
    [...keys, "terminal"],
  );
  assert.equal(
    requests.every(
      (r) =>
        r.project_root === "/registered" &&
        r.root === true &&
        !r.command &&
        !r.extra_args &&
        !r.env,
    ),
    true,
  );
  const unknown = await c.request(
    form({
      op: "workspace.spawn",
      operationId: "opaque-unknown",
      agentType: "unknown.agent",
      prompt: "Do work",
    }),
  );
  assert.equal(unknown.state, "failed");
  assert.equal(
    f.requests.filter((r) => r.type === "spawn").length,
    keys.length + 1,
  );
  for (const [index, agentType] of ["", "bad\nkey", "é".repeat(65)].entries()) {
    const result = await c.request(
      form({
        op: "workspace.spawn",
        operationId: `opaque-invalid-${index}`,
        agentType,
        prompt: "Do work",
      }),
    );
    assert.equal(result.state, "failed");
  }
});

test("screen-confirmed spawn receipts stay pending beyond the old deadline for root, existing and new worktrees", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.hold(true);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const [index, target] of ["root", "worktree", "newWorktree"].entries()) {
    const observed = once(f.events, "spawn");
    const operationId = `slow-spawn-${index}`;
    const pending = c.request(
      form({
        op: "workspace.spawn",
        operationId,
        target,
        agentType: "custom",
        prompt: "日本語\n".repeat(6553) + "abcdef",
        ...(target === "worktree"
          ? { worktreeId: "wt-one" }
          : target === "newWorktree"
            ? { name: "slow-fixture" }
            : {}),
      }),
    );
    const [ipc] = await observed;
    assert.equal(Buffer.byteLength(ipc.prompt), 65536);
    t.mock.timers.tick(65001);
    await new Promise(setImmediate);
    assert.equal(
      (await c.request({ op: "workspace.operation", operationId })).state,
      "pending",
    );
    f.release();
    assert.equal((await pending).state, "succeeded");
  }
  assert.equal(f.requests.filter((r) => r.type === "spawn").length, 3);
});

test("full spawn deadline expiration preserves uncertain ledger across reconnect without rollback or replay", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.hold(true);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const observed = once(f.events, "spawn"),
    operationId = "spawn-deadline";
  const pending = c.request(
    form({
      op: "workspace.spawn",
      operationId,
      agentType: "custom",
      prompt: "Do work",
    }),
  );
  const [ipc] = await observed;
  assert.equal(ipc.cancel_on_disconnect, undefined);
  t.mock.timers.tick(359999);
  await new Promise(setImmediate);
  assert.equal(
    (await c.request({ op: "workspace.operation", operationId })).state,
    "pending",
  );
  t.mock.timers.tick(2);
  await new Promise(setImmediate);
  assert.equal((await pending).state, "uncertain");
  f.release();
  assert.equal(f.agents.length, 1);
  c.dispose();
  const reconnect = f.connect();
  t.after(reconnect.dispose);
  assert.equal(
    (await reconnect.request({ op: "workspace.operation", operationId })).state,
    "uncertain",
  );
  assert.equal(
    (
      await reconnect.request(
        form({
          op: "workspace.spawn",
          operationId,
          agentType: "custom",
          prompt: "Do work",
        }),
      )
    ).state,
    "uncertain",
  );
  assert.equal(f.requests.filter((r) => r.type === "spawn").length, 1);
});

test("terminal, worktree, rename and stop retain their 65-second bridge caps and uncertain ledger", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  f.hold(true);
  f.agents.push({
    id: "ag-known",
    name: "Known",
    project_root: "/registered",
    worktree_id: null,
  });
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const cases = [
    { op: "workspace.terminal", target: "root", ipc: "spawn" },
    { op: "workspace.createWorktree", name: "fixture", ipc: "create_worktree" },
    {
      op: "workspace.rename",
      agentId: "ag-known",
      name: "Renamed",
      ipc: "rename",
    },
    { op: "workspace.stop", agentId: "ag-known", confirm: true, ipc: "kill" },
  ];
  for (const [index, { ipc, ...fields }] of cases.entries()) {
    const operationId = `short-budget-${index}`,
      observed = once(f.events, ipc);
    const pending = c.request({
      machine: "local",
      projectId: "/registered",
      operationId,
      ...fields,
    });
    await observed;
    t.mock.timers.tick(64999);
    await new Promise(setImmediate);
    assert.equal(
      (await c.request({ op: "workspace.operation", operationId })).state,
      "pending",
    );
    t.mock.timers.tick(2);
    await new Promise(setImmediate);
    assert.equal((await pending).state, "uncertain");
    f.release();
    assert.equal(
      (await c.request({ op: "workspace.operation", operationId })).state,
      "uncertain",
    );
  }
  assert.equal(
    f.requests.filter((r) =>
      ["spawn", "create_worktree", "rename", "kill"].includes(r.type),
    ).length,
    4,
  );
});

test("project removal needs confirmation and a daemon that can forget projects for every device", async (t) => {
  const f = await fixture(t),
    c = f.connect();
  t.after(c.dispose);
  const remove = (operationId, extra = {}) =>
    c.request({
      op: "workspace.removeProject",
      operationId,
      machine: "local",
      projectId: "/registered",
      ...extra,
    });
  assert.equal((await remove("remove-0001")).state, "failed");
  const old = await remove("remove-0002", { confirm: true });
  assert.equal(old.state, "failed");
  assert.match(old.message, /Update the machine's daemon/);
  assert.equal(f.requests.filter((r) => r.type === "remove_project").length, 0);

  f.protocolVersion(13);
  const removed = await remove("remove-0003", { confirm: true });
  assert.equal(removed.state, "succeeded");
  const sent = f.requests.filter((r) => r.type === "remove_project");
  assert.deepEqual(sent, [
    {
      type: "remove_project",
      project_root: "/registered",
      suspend_agents: true,
    },
  ]);
});
