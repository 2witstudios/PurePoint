import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { cli } from "./setup.js";
test("published vanilla Pi supports the pinned state/history/queue contract", async () => {
  const folder = await mkdtemp(path.join(os.tmpdir(), "pi-mobile-contract-"));
  const skill = path.join(folder, "pu");
  await mkdir(skill);
  await writeFile(
    path.join(skill, "SKILL.md"),
    "---\nname: pu\ndescription: PurePoint test skill\n---\nRead the CLI reference.",
  );
  // Isolate config and HOME, disable external extensions/skills; no owner auth files or model requests.
  const rpc = new Rpc(
    process.execPath,
    [
      cli,
      "--mode",
      "rpc",
      "--no-session",
      "--no-extensions",
      "--no-skills",
      "--skill",
      skill,
      "--no-approve",
    ],
    folder,
    {
      PATH: process.env.PATH,
      HOME: folder,
      PI_CODING_AGENT_DIR: path.join(folder, "agent"),
      PI_SKIP_VERSION_CHECK: "1",
    },
  );
  rpc.on("failure", () => {});
  try {
    const state = await rpc.call("get_state");
    assert.equal(state.isStreaming, false);
    const entries = await rpc.call("get_entries");
    assert.equal(
      entries.entries.some((e) => e.type === "message"),
      false,
    );
    assert.ok(entries.entries.some((e) => e.id === entries.leafId));
    assert.deepEqual(await rpc.call("clear_queue"), {
      steering: [],
      followUp: [],
    });
    await rpc.call("abort");
    assert.ok(
      (await rpc.call("get_commands")).commands.some(
        (c) => c.name === "skill:pu" && c.source === "skill",
      ),
    );
  } finally {
    await rpc.close();
    await rm(folder, { recursive: true, force: true });
  }
});

test("handled extension new/switch commands reconcile sessions against pinned Pi", async () => {
  const folder = await mkdtemp(path.join(os.tmpdir(), "pi-mobile-extension-"));
  const extension = path.join(folder, "sessions.ts");
  const target = path.join(folder, "target.jsonl");
  await writeFile(
    target,
    [
      {
        type: "session",
        version: 3,
        id: "target-session",
        timestamp: "2026-01-01T00:00:00Z",
        cwd: folder,
      },
      {
        type: "message",
        id: "target-message",
        parentId: null,
        timestamp: "2026-01-01T00:00:01Z",
        message: { role: "user", timestamp: 1, content: "Target conversation" },
      },
    ]
      .map((entry) => JSON.stringify(entry))
      .join("\n") + "\n",
  );
  await writeFile(
    extension,
    `export default function(pi) {
    pi.registerCommand("mobile-new", {handler: async (_args, ctx) => {await ctx.newSession();}});
    pi.registerCommand("mobile-switch", {handler: async (_args, ctx) => {await ctx.switchSession(${JSON.stringify(target)});}});
  }`,
  );
  const rpc = new Rpc(
    process.execPath,
    [
      cli,
      "--mode",
      "rpc",
      "--no-session",
      "--no-extensions",
      "--extension",
      extension,
      "--no-skills",
      "--no-approve",
    ],
    folder,
    {
      PATH: process.env.PATH,
      HOME: folder,
      PI_CODING_AGENT_DIR: path.join(folder, "agent"),
      PI_SKIP_VERSION_CHECK: "1",
    },
  );
  rpc.on("failure", () => {});
  const c = new Controller(rpc, {});
  const events = [];
  rpc.on("event", (event) => events.push(event.type));
  try {
    await c.refresh();
    const initialId = c.state.sessionId;
    for (const text of ["/mobile-new", "/mobile-switch"]) {
      const epoch = c.epoch;
      const result = await c.request({
        version: 1,
        id: crypto.randomUUID(),
        op: "send",
        epoch,
        text,
        mode: "send",
      });
      assert.equal(result.disposition, "handled");
      assert.notEqual(c.epoch, epoch);
      await assert.rejects(
        c.request({
          version: 1,
          id: crypto.randomUUID(),
          op: "send",
          epoch,
          text: "stale",
          mode: "send",
        }),
        /Conversation changed/,
      );
      if (text === "/mobile-new") assert.notEqual(c.state.sessionId, initialId);
      else {
        assert.equal(c.state.sessionId, "target-session");
        assert.equal(c.snapshot().messages.at(-1).text, "Target conversation");
      }
    }
    assert.equal(events.includes("agent_start"), false);
    assert.equal(events.includes("agent_settled"), false);
  } finally {
    c.dispose();
    await rpc.close();
    await rm(folder, { recursive: true, force: true });
  }
});
