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
  c.request({
    version: 1,
    clientId: "test-client",
    id: crypto.randomUUID(),
    epoch: c.epoch,
    op,
    ...data,
  });
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
    clientId: "test-client",
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
  assert.ok(rpc.calls.indexOf("answer") < rpc.calls.indexOf("clear_queue"));
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
    await request(c, "answer", {
      dialogId: "legacy",
      optionId: c.snapshot().dialogs[0].optionIds[0],
    });
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

for (const [name, prefill] of [
  ["9003-character document", "x".repeat(9000) + "end"],
  ["64 KiB ASCII document", "x".repeat(65536)],
  ["64 KiB Unicode document", "😀".repeat(16383) + "éé"],
]) {
  test(`editor preserves an unchanged ${name}`, async () => {
    const rpc = new Runtime();
    const c = new Controller(rpc, {});
    try {
      rpc.emit("event", {
        type: "extension_ui_request",
        id: "edit",
        method: "editor",
        title: "Edit document",
        prefill,
      });
      const dialog = c.snapshot().dialogs[0];
      assert.equal(dialog.prefill, prefill);
      await request(c, "answer", {
        dialogId: dialog.id,
        value: dialog.prefill,
      });
      assert.deepEqual(rpc.answerValue, { id: "edit", value: prefill });
    } finally {
      c.dispose();
    }
  });
}

for (const [name, prefill] of [
  ["ASCII", "x".repeat(65537)],
  ["Unicode", "😀".repeat(16384) + "a"],
]) {
  test(`editor cancels ${name} prefills above the UTF-8 answer budget visibly`, () => {
    const rpc = new Runtime();
    const c = new Controller(rpc, {});
    try {
      const revision = c.snapshot().revision;
      rpc.emit("event", {
        type: "extension_ui_request",
        id: "oversized-editor",
        method: "editor",
        prefill,
      });
      assert.deepEqual(rpc.answerValue, {
        id: "oversized-editor",
        cancelled: true,
      });
      const snapshot = c.snapshot();
      assert.equal(snapshot.dialogs.length, 0);
      assert.ok(
        snapshot.notices.some((x) => /editor.*64 KiB.*canceled/i.test(x)),
      );
      assert.ok(snapshot.revision > revision);
    } finally {
      c.dispose();
    }
  });
}

test("extension hook errors appear in snapshots without stopping continued execution", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    await c.refresh();
    rpc.emit("event", { type: "agent_start" });
    const before = c.snapshot();
    rpc.emit("event", {
      type: "extension_error",
      extensionPath: "/Users/owner/.pi/agent/extensions/check.ts",
      event: "tool_call",
      error: "Hook threw: missing configuration",
    });
    const snapshot = c.snapshot();
    assert.ok(
      snapshot.notices.some(
        (x) =>
          x ===
          "Extension check.ts failed during tool_call: Hook threw: missing configuration",
      ),
    );
    assert.equal(snapshot.busy, before.busy);
    assert.equal(snapshot.runId, before.runId);
    assert.equal(snapshot.epoch, before.epoch);
    assert.equal(snapshot.error, null);
    assert.ok(snapshot.revision > before.revision);
    assert.equal(
      snapshot.notices.some((x) => x.includes("/Users/owner")),
      false,
    );
    const result = await request(c, "send", {
      epoch: c.epoch,
      text: "Continue working",
      mode: "steer",
    });
    assert.equal(result.disposition, "started");
    assert.equal(c.error, null);
    assert.equal(c.runId, before.runId);
  } finally {
    c.dispose();
  }
});

test("extension error diagnostics bound source, hook, error and retained history", () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    for (let i = 0; i < 20; i++) {
      rpc.emit("event", {
        type: "extension_error",
        extensionPath: `/private/owner/${"s".repeat(5000)}.ts`,
        event: "hook".repeat(2000),
        error: `${i}: ${"😀".repeat(100000)}`,
      });
    }
    const snapshot = c.snapshot();
    assert.equal(snapshot.notices.length, 12);
    assert.ok(snapshot.notices.every((x) => x.length <= 4000));
    assert.ok(snapshot.notices.every((x) => x.includes("[Display truncated]")));
    assert.ok(snapshot.notices[0].includes(": 8: "));
    assert.ok(snapshot.notices.at(-1).includes(": 19: "));
    assert.equal(snapshot.error, null);
  } finally {
    c.dispose();
  }
});

for (const mode of ["steer", "after"]) {
  test(`Stop cancels a blocking ${mode} prompt dialog before joining mutations`, async () => {
    const rpc = new Runtime();
    const c = new Controller(rpc, {});
    await c.refresh();
    rpc.emit("event", { type: "agent_start" });
    const started = deferred();
    const unblocked = deferred();
    const original = rpc.call.bind(rpc);
    rpc.call = async (op) => {
      if (op === "prompt") {
        rpc.emit("event", {
          type: "extension_ui_request",
          id: "blocked-prompt",
          method: "confirm",
        });
        started.resolve();
        await unblocked.promise;
        return { disposition: "queued" };
      }
      return original(op);
    };
    rpc.answer = (value) => {
      rpc.answerValue = value;
      unblocked.resolve();
    };
    const prompt = request(c, "send", { epoch: c.epoch, text: "Work", mode });
    await started.promise;
    const stop = request(c, "stop", { epoch: c.epoch, runId: c.runId });
    stop.catch(() => {});
    try {
      assert.deepEqual(rpc.answerValue, {
        id: "blocked-prompt",
        cancelled: true,
      });
      assert.equal(c.snapshot().dialogs.length, 0);
      await prompt;
      await stop;
      assert.ok(rpc.calls.indexOf("clear_queue") < rpc.calls.indexOf("abort"));
    } finally {
      unblocked.resolve();
      await Promise.allSettled([prompt, stop]);
      c.dispose();
    }
  });
}

test("invalid or overcapacity Stop requests never cancel dialogs", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    await c.refresh();
    rpc.emit("event", { type: "agent_start" });
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "unrelated",
      method: "confirm",
    });
    const base = {
      version: 1,
      clientId: "test-client",
      id: "stop-invalid",
      op: "stop",
      epoch: c.epoch,
      runId: c.runId,
    };
    for (const [patch, pattern] of [
      [{ id: "stale-epoch", epoch: "old" }, /Conversation changed/],
      [{ id: "stale-run", runId: "old" }, /Run changed/],
      [{ version: 2 }, /version/],
      [{ id: "stale-epoch" }, /already/],
    ]) {
      await assert.rejects(c.request({ ...base, ...patch }), pattern);
      assert.equal(rpc.answerValue, undefined);
    }
    c.pendingMutations = 16;
    await assert.rejects(
      request(c, "stop", { epoch: c.epoch, runId: c.runId }),
      /Too many/,
    );
    c.pendingMutations = 0;
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "later",
      method: "confirm",
    });
    assert.equal(rpc.answerValue, undefined);
    assert.equal(c.snapshot().dialogs.length, 2);
  } finally {
    c.dispose();
  }
});

test("queued Stops cancel late dialogs only for their observed run and clean up markers", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  const started = deferred();
  const unblocked = deferred();
  const original = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "prompt") {
      started.resolve();
      await unblocked.promise;
      return { disposition: "queued" };
    }
    return original(op);
  };
  const prompt = request(c, "send", {
    epoch: c.epoch,
    text: "Work",
    mode: "steer",
  });
  await started.promise;
  const target = { epoch: c.epoch, runId: c.runId };
  const stops = [request(c, "stop", target), request(c, "stop", target)];
  for (const stop of stops) stop.catch(() => {});
  try {
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "late",
      method: "input",
    });
    assert.deepEqual(rpc.answerValue, { id: "late", cancelled: true });
    rpc.emit("event", { type: "agent_settled" });
    rpc.emit("event", { type: "agent_start" });
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "replacement",
      method: "confirm",
    });
    assert.deepEqual(
      c.snapshot().dialogs.map((d) => d.id),
      ["replacement"],
    );
    unblocked.resolve();
    await prompt;
    for (const stop of stops) await assert.rejects(stop, /Run changed/);
    assert.equal(rpc.calls.includes("abort"), false);
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "after-stop",
      method: "confirm",
    });
    assert.deepEqual(
      c.snapshot().dialogs.map((d) => d.id),
      ["replacement", "after-stop"],
    );
  } finally {
    unblocked.resolve();
    await Promise.allSettled([prompt, ...stops]);
    c.dispose();
  }
});

function extensionUI(rpc, suffix) {
  for (const event of [
    { method: "notify", message: `${suffix} notice` },
    { method: "setStatus", statusKey: suffix, statusText: `${suffix} status` },
    {
      method: "setWidget",
      widgetKey: suffix,
      widgetLines: [`${suffix} widget`],
    },
    { method: "setTitle", title: `${suffix} title` },
    { method: "set_editor_text", text: `${suffix} draft` },
  ])
    rpc.emit("event", {
      type: "extension_ui_request",
      id: `${suffix}-offer`,
      ...event,
    });
}

for (const op of ["new", "resume", "send"]) {
  test(`${op} preserves incoming session hook UI and interactive answers while clearing outgoing UI`, async () => {
    const rpc = new Runtime();
    const c = new Controller(rpc, { path: async () => "/native/two" });
    await c.refresh();
    extensionUI(rpc, "old");
    const epoch = c.epoch;
    const started = deferred();
    const answered = deferred();
    const original = rpc.call.bind(rpc);
    rpc.call = async (nativeOp) => {
      if (["new_session", "switch_session", "prompt"].includes(nativeOp)) {
        rpc.state = { ...rpc.state, sessionId: "two" };
        extensionUI(rpc, "incoming");
        rpc.emit("event", {
          type: "extension_ui_request",
          id: "hook-dialog",
          method: "confirm",
        });
        started.resolve();
        await answered.promise;
        rpc.emit("event", {
          type: "extension_ui_request",
          id: "incoming-pending",
          method: "input",
        });
        return op === "send" ? { disposition: "handled" } : {};
      }
      return original(nativeOp);
    };
    rpc.answer = () => answered.resolve();
    const mutation = request(c, op, {
      epoch,
      sessionId: "two",
      text: "/new",
      mode: "send",
    });
    try {
      await started.promise;
      assert.ok(c.snapshot().dialogs.some((d) => d.id === "hook-dialog"));
      await request(c, "answer", { dialogId: "hook-dialog", confirmed: true });
      await mutation;
      const snapshot = c.snapshot();
      assert.notEqual(snapshot.epoch, epoch);
      assert.equal(snapshot.title, "incoming title");
      assert.equal(snapshot.editor.text, "incoming draft");
      assert.ok(snapshot.notices.includes("incoming notice"));
      assert.ok(snapshot.notices.includes("incoming status"));
      assert.ok(snapshot.notices.includes("incoming widget"));
      assert.equal(
        snapshot.notices.some((text) => text.startsWith("old")),
        false,
      );
      assert.deepEqual(
        snapshot.dialogs.map((d) => d.id),
        ["incoming-pending"],
      );
    } finally {
      answered.resolve();
      await Promise.allSettled([mutation]);
      c.dispose();
    }
  });
}

for (const outcome of ["veto", "failure"]) {
  test(`session ${outcome} retains outgoing UI and epoch`, async () => {
    const rpc = new Runtime();
    const c = new Controller(rpc, {});
    await c.refresh();
    extensionUI(rpc, "old");
    const before = c.snapshot();
    const original = rpc.call.bind(rpc);
    rpc.call = async (op) => {
      if (op === "new_session") {
        if (outcome === "failure") throw new Error("Switch failed");
        return { cancelled: true };
      }
      return original(op);
    };
    try {
      await assert.rejects(
        request(c, "new", { epoch: c.epoch }),
        /canceled|Switch failed/,
      );
      const after = c.snapshot();
      assert.equal(after.epoch, before.epoch);
      assert.equal(after.title, before.title);
      assert.deepEqual(after.editor, before.editor);
      assert.deepEqual(after.notices, before.notices);
    } finally {
      c.dispose();
    }
  });
}

test("a failed Stop retains other queued Stop markers until their own cleanup", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  rpc.emit("event", { type: "agent_start" });
  const firstClear = deferred();
  const secondClear = deferred();
  const secondStarted = deferred();
  const original = rpc.call.bind(rpc);
  let clears = 0;
  rpc.call = async (op) => {
    if (op === "clear_queue") {
      if (++clears === 1) return firstClear.promise;
      secondStarted.resolve();
      return secondClear.promise;
    }
    return original(op);
  };
  const target = { epoch: c.epoch, runId: c.runId };
  const stops = [request(c, "stop", target), request(c, "stop", target)];
  for (const stop of stops) stop.catch(() => {});
  try {
    firstClear.reject(new Error("First queue clear failed"));
    await assert.rejects(stops[0], /First queue/);
    await secondStarted.promise;
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "while-second-pending",
      method: "confirm",
    });
    assert.deepEqual(rpc.answerValue, {
      id: "while-second-pending",
      cancelled: true,
    });
    secondClear.reject(new Error("Second queue clear failed"));
    await assert.rejects(stops[1], /Second queue/);
    rpc.emit("event", {
      type: "extension_ui_request",
      id: "after-errors",
      method: "confirm",
    });
    assert.deepEqual(
      c.snapshot().dialogs.map((d) => d.id),
      ["after-errors"],
    );
  } finally {
    firstClear.resolve({});
    secondClear.resolve({});
    await Promise.allSettled(stops);
    c.dispose();
  }
});

test("identical queued prompts keep FIFO ownership when the first starts running", async () => {
  const rpc = new Runtime();
  const original = rpc.call.bind(rpc);
  const queued = [];
  rpc.call = async (op, args) => {
    if (op !== "prompt") return original(op);
    queued.push(args.message);
    rpc.emit("event", {
      type: "queue_update",
      enqueuedMode: "after",
      inputSource: "rpc",
      followUp: [...queued],
    });
    return { disposition: "queued" };
  };
  const c = new Controller(rpc, {});
  try {
    await c.refresh();
    rpc.emit("event", { type: "agent_start" });
    await request(c, "send", {
      clientId: "phone",
      text: "Same prompt",
      mode: "after",
      epoch: c.epoch,
    });
    c.event({ type: "queue_update", followUp: ["Same prompt"] });
    await request(c, "send", {
      clientId: "desktop",
      text: "Same prompt",
      mode: "after",
      epoch: c.epoch,
    });
    c.event({ type: "queue_update", followUp: ["Same prompt", "Same prompt"] });
    const secondId = c.queue[1].id;
    c.event({ type: "queue_update", followUp: ["Same prompt"] });
    assert.equal(c.queue[0].id, secondId);
    assert.equal(c.queue[0].clientId, "desktop");
    rpc.queue = [];
    rpc.call = async (op) =>
      op === "clear_queue"
        ? { followUp: ["Same prompt"], steering: [] }
        : original(op);
    await request(c, "stop", {
      epoch: c.epoch,
      runId: c.runId,
      clientId: "phone",
    });
    assert.equal(c.canceled[0].clientId, "desktop");
    assert.equal(c.canceled[0].id, secondId);
  } finally {
    c.dispose();
  }
});

test("v1 requires client identity and scopes duplicate request IDs per client", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  try {
    await c.refresh();
    await assert.rejects(
      c.request({ version: 1, id: "missing", op: "sync" }),
      /client identity/,
    );
    await request(c, "sync", { id: "same", clientId: "phone" });
    await request(c, "sync", { id: "same", clientId: "desktop" });
    await assert.rejects(
      request(c, "sync", { id: "same", clientId: "phone" }),
      /already received/,
    );
  } finally {
    c.dispose();
  }
});

for (const mode of ["steer", "after"]) {
  const field = mode === "steer" ? "steering" : "followUp";
  test(`${mode} native input expansion preserves recovery ownership without leaking origins`, async () => {
    const rpc = new Runtime();
    const original = rpc.call.bind(rpc);
    let texts = [];
    rpc.call = async (op, args) => {
      if (op === "prompt") {
        // Native input handlers, skills and templates replace the submitted text.
        texts.push(`Expanded: ${args.message}`);
        rpc.emit("event", {
          type: "queue_update",
          enqueuedMode: mode,
          inputSource: "rpc",
          [field]: [...texts],
        });
        return { disposition: "queued" };
      }
      if (op === "clear_queue") {
        const recovered = { [field]: [...texts] };
        texts = [];
        rpc.emit("event", { type: "queue_update", queueCleared: true });
        return recovered;
      }
      return original(op);
    };
    const c = new Controller(rpc, {});
    try {
      await c.refresh();
      rpc.emit("event", { type: "agent_start" });
      for (let index = 0; index < 105; index++) {
        await request(c, "send", {
          clientId: "phone",
          mode,
          text: `/skill:test ${index}`,
        });
        assert.equal(c.queue[0].clientId, "phone");
        assert.equal(c.queue[0].text, `Expanded: /skill:test ${index}`);
        texts = [];
        rpc.emit("event", { type: "queue_update" });
        assert.equal(c.nativeQueue.length, 0);
        assert.equal(c.pendingEnqueue, null);
      }
      await request(c, "send", {
        clientId: "desktop",
        mode,
        text: "/template final",
      });
      const id = c.queue[0].id;
      await request(c, "stop", { clientId: "phone", runId: c.runId });
      assert.deepEqual(
        c.canceled.map(({ id, clientId, text }) => ({ id, clientId, text })),
        [{ id, clientId: "desktop", text: "Expanded: /template final" }],
      );
    } finally {
      c.dispose();
    }
  });

  test(`${mode} Stop reconciles identical-message consumption during clear_queue`, async () => {
    const rpc = new Runtime();
    const original = rpc.call.bind(rpc);
    const clearing = deferred();
    const release = deferred();
    let texts = [];
    rpc.call = async (op, args) => {
      if (op === "prompt") {
        texts.push(args.message);
        rpc.emit("event", {
          type: "queue_update",
          enqueuedMode: mode,
          inputSource: "rpc",
          [field]: [...texts],
        });
        return { disposition: "queued" };
      }
      if (op === "clear_queue") {
        clearing.resolve();
        await release.promise;
        const recovered = { [field]: [...texts] };
        texts = [];
        rpc.emit("event", { type: "queue_update", queueCleared: true });
        return recovered;
      }
      return original(op);
    };
    const c = new Controller(rpc, {});
    try {
      await c.refresh();
      rpc.emit("event", { type: "agent_start" });
      for (const clientId of ["phone", "desktop"])
        await request(c, "send", { clientId, mode, text: "Identical" });
      const desktopId = c.queue[1].id;
      const stop = request(c, "stop", { clientId: "phone", runId: c.runId });
      await clearing.promise;
      texts.shift();
      rpc.emit("event", {
        type: "queue_update",
        enqueuedMode: mode,
        inputSource: "rpc",
        [field]: [...texts],
      });
      assert.equal(c.queue[0].clientId, "desktop");
      release.resolve();
      await stop;
      assert.deepEqual(
        c.canceled.map(({ id, clientId }) => ({ id, clientId })),
        [{ id: desktopId, clientId: "desktop" }],
      );
      assert.equal(c.nativeQueue.length, 0);
    } finally {
      release.resolve();
      c.dispose();
    }
  });
}

test("ownership fence rejects new and queued actions and drain awaits accepted work", async () => {
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  await c.refresh();
  let release, entered;
  const started = new Promise((resolve) => {
    entered = resolve;
  });
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const call = rpc.call.bind(rpc);
  rpc.call = async (op) => {
    if (op === "prompt") {
      entered();
      await gate;
    }
    return call(op);
  };
  const first = request(c, "send", { text: "accepted", mode: "send" });
  await started;
  const queued = request(c, "new").then(
    () => {
      throw new Error("Queued action dispatched after fence");
    },
    (error) => error,
  );
  c.fence();
  await assert.rejects(request(c, "sync"), /closing/);
  await assert.rejects(
    request(c, "send", { text: "late", mode: "send" }),
    /closing/,
  );
  let drained = false;
  const drain = c.drain().then(() => {
    drained = true;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(drained, false);
  release();
  await first;
  assert.match((await queued).message, /Queued input was not dispatched/);
  await drain;
  assert.equal(c.pendingMutations, 0);
  assert.equal(rpc.calls.filter((op) => op === "prompt").length, 1);
  assert.equal(rpc.calls.includes("new_session"), false);
  c.dispose();
});
