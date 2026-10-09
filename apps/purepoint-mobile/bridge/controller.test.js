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

test("selection IDs should preserve original values despite clipped or colliding labels", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    const prefix = "x".repeat(220);
    const options = [prefix + "first", prefix + "second", "Short option"];
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "choose",
      method: "select",
      options,
    });
    const dialog = c.snapshot().dialogs[0];
    assert.equal(dialog.options[0], dialog.options[1]);
    assert.equal(dialog.optionIds.length, 3);
    assert.notEqual(dialog.optionIds[0], dialog.optionIds[1]);
    await assert.rejects(
      request(c, "answer", { dialogId: "choose", value: dialog.options[0] }),
      /option/,
    );
    assert.equal(rpc.answerValue, undefined);
    await assert.rejects(
      request(c, "answer", { dialogId: "choose", optionId: "foreign-option" }),
      /option/,
    );
    await request(c, "answer", {
      dialogId: "choose",
      optionId: dialog.optionIds[1],
    });
    assert.equal(rpc.answerValue.value, options[1]);
    assert.equal(c.snapshot().dialogs.length, 0);
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "legacy",
      method: "select",
      options: ["Short option"],
    });
    await request(c, "answer", { dialogId: "legacy", value: "Short option" });
    assert.equal(rpc.answerValue.value, "Short option");
  } finally {
    c.dispose();
  }
});

test("selection budgets should cancel oversized native values without truncating answers", () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "oversized",
      method: "select",
      options: ["x".repeat(256 * 1024 + 1)],
    });
    assert.deepEqual(rpc.answerValue, { id: "oversized", cancelled: true });
    assert.equal(c.snapshot().dialogs.length, 0);
    assert.ok(c.snapshot().notices.some((x) => x.includes("option budget")));
  } finally {
    c.dispose();
  }
});

test("handled native session commands refresh and invalidate queued stale actions", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  c.editorOffer = { id: "old", text: "old draft" };
  c.titleOverride = "old title";
  c.queue = [{ mode: "after", text: "old queued text" }];
  c.statuses.set("old", "old status");
  c.widgets.set("old", "old widget");
  c.notice("old notice");
  c.projection.event({
    type: "message_end",
    message: { role: "user", timestamp: 1, content: "Old conversation" },
  });
  const original = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "prompt") {
      rpc.calls.push(op);
      rpc.state = {
        ...rpc.state,
        sessionId: "two",
        sessionName: "New conversation",
      };
      return { disposition: "handled" };
    }
    return original(op);
  };
  const epoch = c.epoch;
  const handled = request(c, "send", { epoch, text: "/new", mode: "send" });
  const stale = request(c, "send", {
    epoch,
    text: "old conversation input",
    mode: "send",
  });
  assert.deepEqual(await handled, { disposition: "handled" });
  await assert.rejects(stale, /Conversation changed/);
  const s = c.snapshot();
  assert.equal(s.sessionId, "two");
  assert.notEqual(s.epoch, epoch);
  assert.equal(s.title, "New conversation");
  assert.deepEqual(s.messages, []);
  assert.deepEqual(s.tools, []);
  assert.deepEqual(s.queue, []);
  assert.deepEqual(s.notices, []);
  assert.equal(s.editor, null);
  assert.equal(rpc.calls.filter((op) => op === "prompt").length, 1);
  c.dispose();
});

test("sync detects native session switches while same-session refresh retains epoch", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  const epoch = c.epoch;
  assert.equal((await request(c, "sync")).epoch, epoch);
  rpc.state = { ...rpc.state, sessionId: "resumed" };
  const s = await request(c, "sync");
  assert.equal(s.sessionId, "resumed");
  assert.notEqual(s.epoch, epoch);
  await assert.rejects(request(c, "new", { epoch }), /Conversation changed/);
  c.dispose();
});

test("handled commands request fresh state after an older sync finishes", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  const epoch = c.epoch;
  let resolveHistory;
  let resolvePrompt;
  let historyStarted;
  let promptStarted;
  const historyReady = new Promise((resolve) => {
    historyStarted = resolve;
  });
  const promptReady = new Promise((resolve) => {
    promptStarted = resolve;
  });
  const original = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "get_entries" && !resolveHistory)
      return new Promise((resolve) => {
        resolveHistory = resolve;
        historyStarted();
      });
    if (op === "prompt") {
      rpc.state = { ...rpc.state, sessionId: "changed" };
      return new Promise((resolve) => {
        resolvePrompt = resolve;
        promptStarted();
      });
    }
    return original(op);
  };
  const sync = request(c, "sync");
  await historyReady;
  const command = request(c, "send", { epoch, text: "/new", mode: "send" });
  await promptReady;
  resolvePrompt({ disposition: "handled" });
  resolveHistory({ entries: [], leafId: null });
  await sync;
  await command;
  assert.equal(c.state.sessionId, "changed");
  assert.notEqual(c.epoch, epoch);
  c.dispose();
});

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("overlapping sync requests share one native read", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  const history = deferred();
  const started = deferred();
  const original = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "get_entries") {
      rpc.calls.push(op);
      started.resolve();
      return history.promise;
    }
    return original(op);
  };
  try {
    const first = request(c, "sync");
    await started.promise;
    const second = request(c, "sync");
    history.resolve({ entries: [], leafId: null });
    const snapshots = await Promise.all([first, second]);
    assert.deepEqual(snapshots[0], snapshots[1]);
    assert.deepEqual(rpc.calls, ["get_state", "get_entries"]);
  } finally {
    c.dispose();
  }
});

for (const op of ["stop", "new", "resume", "send"]) {
  for (const fails of [false, true]) {
    test(`${op} reads fresh native state after an older sync ${fails ? "fails" : "finishes"}`, async () => {
      const rpc = new Runtime();
      rpc.state.isStreaming = op === "stop";
      const c = new Controller(rpc, { path: async () => "/native/one" });
      await c.refresh();
      const epoch = c.epoch;
      c.editorOffer = { id: "old", text: "Old draft" };
      const history = deferred();
      const started = deferred();
      const mutated = deferred();
      const original = rpc.call.bind(rpc);
      let holdHistory = true;
      rpc.call = async (nativeOp) => {
        if (nativeOp === "get_state") {
          rpc.calls.push(nativeOp);
          return { ...rpc.state };
        }
        if (nativeOp === "get_entries" && holdHistory) {
          holdHistory = false;
          rpc.calls.push(nativeOp);
          started.resolve();
          return history.promise;
        }
        if (
          ["abort", "new_session", "switch_session", "prompt"].includes(
            nativeOp,
          )
        ) {
          rpc.calls.push(nativeOp);
          rpc.state = {
            ...rpc.state,
            isStreaming: false,
            sessionName: "Post-mutation title",
            // Explicit new/resume must reset even when the session ID is unchanged.
            sessionId: op === "send" ? "changed" : "one",
          };
          mutated.resolve();
          return nativeOp === "prompt" ? { disposition: "handled" } : {};
        }
        return original(nativeOp);
      };
      try {
        // Attach rejection handling before deliberately failing this read.
        const sync = request(c, "sync").then(
          (snapshot) => ({ snapshot }),
          (error) => ({ error }),
        );
        await started.promise;
        const mutation = request(c, op, {
          epoch,
          runId: c.runId,
          sessionId: "one",
          text: "/new",
          mode: "send",
        });
        await mutated.promise;
        const stateReads = rpc.calls.filter((x) => x === "get_state").length;
        if (fails) history.reject(new Error("Older read failed"));
        else history.resolve({ entries: [], leafId: null });
        const old = await sync;
        await mutation;
        if (fails) assert.match(old.error.message, /Older read failed/);
        else assert.equal(old.snapshot.title, "Pi");
        const fresh = c.snapshot();
        assert.equal(fresh.title, "Post-mutation title");
        assert.equal(fresh.busy, false);
        assert.equal(fresh.runId, null);
        assert.equal(
          rpc.calls.filter((x) => x === "get_state").length,
          stateReads + 1,
        );
        if (op === "stop") {
          assert.equal(fresh.epoch, epoch);
          assert.equal(fresh.editor.text, "Old draft");
          assert.equal(fresh.canceled[0].text, "recover me");
        } else {
          assert.notEqual(fresh.epoch, epoch);
          assert.equal(fresh.editor, null);
        }
      } finally {
        c.dispose();
      }
    });
  }
}

test("explicit reset during an older refresh performs a new read and rotates epoch", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  const epoch = c.epoch;
  const history = deferred();
  const started = deferred();
  const original = rpc.call.bind(rpc);
  let holdHistory = true;
  rpc.call = async (op) => {
    if (op === "get_entries" && holdHistory) {
      holdHistory = false;
      rpc.calls.push(op);
      started.resolve();
      return history.promise;
    }
    return original(op);
  };
  try {
    const older = c.refresh();
    await started.promise;
    const reset = c.refresh(true);
    history.resolve({ entries: [], leafId: null });
    await older;
    await reset;
    assert.notEqual(c.epoch, epoch);
    assert.equal(rpc.calls.filter((x) => x === "get_state").length, 3);
  } finally {
    c.dispose();
  }
});
