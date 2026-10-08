import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { Controller } from "./controller.js";
class Runtime extends EventEmitter {
  constructor() {
    super();
    this.calls = [];
    this.state = {
      sessionId: "one",
      model: { id: "test" },
      isStreaming: false,
    };
    this.queue = ["recover me"];
  }
  async call(op) {
    this.calls.push(op);
    if (op === "get_state") return this.state;
    if (op === "get_entries") return { entries: [], leafId: null };
    if (op === "clear_queue") return { steering: this.queue, followUp: [] };
    if (op === "prompt") {
      this.emit("event", { type: "agent_start" });
      return { disposition: "started" };
    }
    return {};
  }
  answer(x) {
    this.answerValue = x;
  }
}
const request = (c, op, data = {}) =>
  c.request({ version: 1, id: crypto.randomUUID(), op, ...data });
test("stop clears queue before abort and rejects stale run targets", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  await request(c, "send", { epoch: c.epoch, text: "go", mode: "send" });
  const target = c.runId;
  const stopped = await request(c, "stop", { epoch: c.epoch, runId: target });
  assert.deepEqual(stopped.steering, ["recover me"]);
  assert.ok(rpc.calls.indexOf("clear_queue") < rpc.calls.indexOf("abort"));
  rpc.emit("event", { type: "agent_settled" });
  rpc.emit("event", { type: "agent_start" });
  await assert.rejects(
    request(c, "stop", { epoch: c.epoch, runId: target }),
    /changed/,
  );
  c.dispose();
});
test("switching waits for idle while browsing remains available", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {
    list: async () => [{ id: "old", path: "/native/old" }],
    history: async () => ({ messages: [{ id: "old", text: "Past" }] }),
  });
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  await assert.rejects(request(c, "new", { epoch: c.epoch }), /Stop/);
  assert.equal(
    (await request(c, "history", { sessionId: "old" })).messages[0].text,
    "Past",
  );
  c.dispose();
});
test("reconnect snapshots retain partial work and correlated extension requests", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  rpc.emit("event", {
    type: "message_start",
    message: { role: "assistant", timestamp: 1, content: [] },
  });
  rpc.emit("event", {
    type: "message_update",
    assistantMessageEvent: {
      type: "text_delta",
      contentIndex: 0,
      delta: "Still working",
    },
  });
  rpc.emit("event", {
    type: "extension_ui_request",
    id: "dialog",
    method: "confirm",
    title: "Continue?",
  });
  const s = await request(c, "sync");
  assert.equal(s.messages[0].text, "Still working");
  assert.equal(s.dialogs[0].id, "dialog");
  await request(c, "answer", { dialogId: "dialog", confirmed: false });
  assert.equal(rpc.answerValue.confirmed, false);
  assert.equal(c.snapshot().dialogs.length, 0);
  c.dispose();
});
test("unknown versions and duplicate request ids cannot execute work", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  await assert.rejects(
    c.request({ version: 2, id: "x", op: "send" }),
    /version/,
  );
  const r = {
    version: 1,
    id: "same",
    op: "send",
    text: "once",
    mode: "send",
    epoch: c.epoch,
  };
  await c.request(r);
  await assert.rejects(c.request(r), /already/);
  assert.equal(rpc.calls.filter((x) => x === "prompt").length, 1);
  c.dispose();
});
test("completion during refresh cannot resurrect stale busy state", async () => {
  const rpc = new Runtime();
  let resolveHistory;
  rpc.call = async (op) => {
    if (op === "get_state") return { ...rpc.state, isStreaming: true };
    if (op === "get_entries")
      return await new Promise((resolve) => {
        resolveHistory = resolve;
      });
    return {};
  };
  const c = new Controller(rpc, {});
  const refreshing = c.refresh();
  await Promise.resolve();
  rpc.emit("event", { type: "agent_settled" });
  resolveHistory({ entries: [], leafId: null });
  await refreshing;
  assert.equal(c.snapshot().busy, false);
  c.dispose();
});
test("completion and a new run while clearing queue prevents stale abort", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  const runId = c.runId;
  const original = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "clear_queue") {
      rpc.emit("event", { type: "agent_settled" });
      rpc.emit("event", { type: "agent_start" });
      return { steering: [], followUp: [] };
    }
    return original(op);
  };
  await request(c, "stop", { epoch: c.epoch, runId });
  assert.equal(rpc.calls.includes("abort"), false);
  c.dispose();
});
test("extension editor offers survive disconnected snapshots", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", {
    type: "extension_ui_request",
    id: "offer",
    method: "set_editor_text",
    text: "Suggested text",
  });
  assert.equal((await request(c, "sync")).editor.text, "Suggested text");
  c.dispose();
});
test("lost Stop acknowledgement still exposes canceled text after reconnect", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  await request(c, "stop", { epoch: c.epoch, runId: c.runId });
  const reconnected = await request(c, "sync");
  assert.equal(reconnected.canceled[0].text, "recover me");
  c.dispose();
});
test("Stop cancels correlated extension dialogs so abort can reach idle", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  rpc.emit("event", {
    type: "extension_ui_request",
    id: "blocked-tool",
    method: "confirm",
    title: "Continue?",
  });
  rpc.answer = (value) => {
    rpc.answerValue = value;
    rpc.calls.push("answer");
  };
  await request(c, "stop", { epoch: c.epoch, runId: c.runId });
  assert.equal(rpc.answerValue?.cancelled, true);
  assert.equal(c.snapshot().dialogs.length, 0);
  assert.ok(rpc.calls.indexOf("clear_queue") < rpc.calls.indexOf("answer"));
  assert.ok(rpc.calls.indexOf("answer") < rpc.calls.indexOf("abort"));
  c.dispose();
});
