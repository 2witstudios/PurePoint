import { EventEmitter } from "node:events";
import { randomUUID } from "node:crypto";
import { Projection, clip, boundedRows } from "./core.js";
export class Controller extends EventEmitter {
  constructor(rpc, sessions) {
    super();
    this.rpc = rpc;
    this.sessions = sessions;
    this.projection = new Projection();
    this.epoch = randomUUID();
    this.revision = 0;
    this.activityVersion = 0;
    this.editorOffer = null;
    this.runId = null;
    this.stoppingRun = null;
    this.busy = false;
    this.state = {};
    this.queue = [];
    this.canceled = [];
    this.dialogs = new Map();
    this.notices = [];
    this.statuses = new Map();
    this.widgets = new Map();
    this.titleOverride = null;
    this.seen = new Set();
    this.serial = Promise.resolve();
    this.pendingMutations = 0;
    this.refreshing = null;
    this.error = null;
    this.broadcastTimer = null;
    this.disposed = false;
    rpc.on("event", (e) => this.event(e));
    rpc.on("failure", (error) => {
      this.error = error.message;
      this.busy = false;
      this.runId = null;
      for (const item of this.dialogs.values()) clearTimeout(item.timer);
      this.dialogs.clear();
      for (const tool of this.projection.tools)
        if (tool.state === "running") tool.state = "unresolved";
      this.changed();
    });
  }
  changed() {
    this.revision++;
    if (!this.broadcastTimer && !this.disposed)
      this.broadcastTimer = setTimeout(() => {
        this.broadcastTimer = null;
        this.emit("snapshot", this.snapshot());
      }, 50);
  }
  notice(text) {
    this.notices.push(clip(text, 4000));
    this.notices = this.notices.slice(-12);
  }
  event(e) {
    if (e.type === "agent_start") {
      this.activityVersion++;
      if (!this.busy) this.runId = randomUUID();
      this.busy = true;
    }
    if (e.type === "agent_settled") {
      this.activityVersion++;
      this.busy = false;
      this.runId = null;
      for (const tool of this.projection.tools)
        if (tool.state === "running")
          tool.state = e.aborted ? "interrupted" : "unresolved";
      this.refresh().catch((err) => {
        this.error = err.message;
        this.changed();
      });
    }
    if (e.type === "auto_compaction_start")
      this.notice("Pi is summarizing earlier context.");
    if (e.type === "auto_retry_start")
      this.notice("Pi is retrying the provider request.");
    if (e.type === "queue_update")
      this.queue = [
        ...(e.steering ?? []).map((text) => ({
          mode: "steer",
          text: clip(text),
        })),
        ...(e.followUp ?? []).map((text) => ({
          mode: "after",
          text: clip(text),
        })),
      ].slice(0, 100);
    if (e.type === "extension_ui_request") {
      const method = e.method;
      if (["confirm", "select", "input", "editor"].includes(method)) {
        if (this.stoppingRun && this.stoppingRun === this.runId) {
          this.rpc.answer({ id: e.id, cancelled: true });
        } else if (this.dialogs.size >= 32) {
          this.rpc.answer({ id: e.id, cancelled: true });
          this.notice(
            "An extension opened too many dialogs; the extra request was canceled.",
          );
        } else {
          const dialog = {
            ...e,
            title: clip(e.title, 1000),
            message: clip(e.message, 4000),
            prefill: clip(e.prefill, 8192),
            options: e.options?.slice(0, 100).map((x) => clip(x, 200)),
          };
          const timer = e.timeout
            ? setTimeout(() => {
                this.dialogs.delete(e.id);
                this.changed();
              }, e.timeout)
            : null;
          this.dialogs.set(e.id, { dialog, timer });
        }
      } else if (method === "setStatus") {
        if (e.statusText)
          this.statuses.set(e.statusKey, clip(e.statusText, 1000));
        else this.statuses.delete(e.statusKey);
        if (this.statuses.size > 32)
          this.statuses.delete(this.statuses.keys().next().value);
      } else if (method === "setWidget") {
        const key = clip(e.widgetKey, 200);
        if (e.widgetLines)
          this.widgets.set(key, clip(e.widgetLines.join("\n"), 4000));
        else this.widgets.delete(key);
        if (this.widgets.size > 16)
          this.widgets.delete(this.widgets.keys().next().value);
      } else if (method === "notify") this.notice(e.message);
      else if (method === "setTitle") this.titleOverride = clip(e.title, 200);
      else if (method === "set_editor_text") {
        this.editorOffer = { id: e.id, text: clip(e.text) };
        this.emit("editor", { type: "editor", ...this.editorOffer });
        this.notice(
          "An extension offered composer text. Your draft is preserved.",
        );
      } else {
        this.rpc.answer({ id: e.id, cancelled: true });
        this.notice(`Unsupported extension interaction: ${clip(method, 100)}`);
      }
    }
    this.projection.event(e);
    this.changed();
  }
  snapshot() {
    return {
      type: "snapshot",
      version: 1,
      epoch: this.epoch,
      revision: this.revision,
      runId: this.runId,
      busy: this.busy,
      sessionId: this.state.sessionId ?? "",
      title: this.titleOverride ?? clip(this.state.sessionName ?? "Pi", 200),
      messages: boundedRows(this.projection.messages, 1024 * 1024),
      tools: boundedRows(this.projection.tools, 256 * 1024),
      queue: boundedRows(this.queue, 128 * 1024),
      canceled: boundedRows(this.canceled, 128 * 1024),
      dialogs: [...this.dialogs.values()].map((x) => x.dialog),
      notices: [
        ...this.notices,
        ...this.statuses.values(),
        ...this.widgets.values(),
      ],
      error: this.error,
      editor: this.editorOffer,
    };
  }
  async refresh() {
    if (this.refreshing) return this.refreshing;
    this.refreshing = (async () => {
      const activityVersion = this.activityVersion;
      const state = await this.rpc.call("get_state");
      const history = await this.rpc.call("get_entries");
      this.state = state;
      // Events are consumed continuously. Never let an older get_state clear a live run.
      if (activityVersion === this.activityVersion) {
        this.busy = !!(state.isStreaming || state.isCompacting);
        if (this.busy) this.runId ??= randomUUID();
        else this.runId = null;
      }
      this.projection.load(history.entries, history.leafId);
      this.changed();
      return this.snapshot();
    })();
    try {
      return await this.refreshing;
    } finally {
      this.refreshing = null;
    }
  }
  validate(r) {
    if (r.version !== 1)
      throw new Error(
        "Unsupported bridge protocol version. Update the app and bridge together.",
      );
    if (typeof r.id !== "string" || r.id.length > 100 || !r.id)
      throw new Error("Request requires a unique ID");
    if (this.seen.has(r.id))
      throw new Error(
        "Request already received. Inspect history; it will not be replayed.",
      );
    this.seen.add(r.id);
    if (this.seen.size > 2000)
      this.seen.delete(this.seen.values().next().value);
  }
  async request(r) {
    this.validate(r);
    if (r.op === "answer") return this.answer(r);
    if (r.op === "sync") return this.refresh();
    if (r.op === "sessions")
      return {
        sessions: (await this.sessions.list()).map((x) => ({
          id: x.id,
          title: clip(x.name || x.firstMessage || "Untitled conversation", 200),
          date: x.modified ?? x.created,
        })),
      };
    if (r.op === "history") return this.sessions.history(r.sessionId);
    if (!["send", "stop", "new", "resume"].includes(r.op))
      throw new Error("Unsupported bridge operation");
    if (this.pendingMutations >= 16)
      throw new Error("Too many pending actions");
    this.pendingMutations++;
    const operation = this.serial.then(() => this.mutate(r));
    this.serial = operation.catch(() => {});
    try {
      return await operation;
    } finally {
      this.pendingMutations--;
    }
  }
  checkEpoch(r) {
    if (r.epoch !== this.epoch)
      throw new Error(
        "Conversation changed. Refresh before trying this action.",
      );
    if (this.error) throw new Error(this.error);
  }
  async mutate(r) {
    this.checkEpoch(r);
    if (r.op === "send") {
      if (
        typeof r.text !== "string" ||
        !r.text.trim() ||
        Buffer.byteLength(r.text, "utf8") > 65536
      )
        throw new Error("Write a message of at most 64 KiB.");
      if (!["send", "steer", "after"].includes(r.mode))
        throw new Error("Choose Send, Steer or After reply.");
      if (this.busy && r.mode === "send")
        throw new Error("Pi is running. Choose Steer or After reply.");
      if (!this.state.model)
        throw new Error(
          "No Pi model configured. Open local Pi and configure a provider/model first.",
        );
      return await this.rpc.call("prompt", {
        message: r.text,
        ...(r.mode === "send"
          ? {}
          : { streamingBehavior: r.mode === "steer" ? "steer" : "followUp" }),
      });
    }
    if (r.op === "stop") {
      if (!this.busy || r.runId !== this.runId)
        throw new Error(
          "Run changed or already finished. This Stop was ignored.",
        );
      const run = this.runId;
      this.stoppingRun = run;
      try {
        const recovered = await this.rpc.call("clear_queue");
        const texts = [
          ...(recovered.steering ?? []),
          ...(recovered.followUp ?? []),
        ];
        this.canceled.push(
          ...texts.map((text, index) => ({
            id: `${r.id}:${index}`,
            text: clip(text),
            sessionId: this.state.sessionId,
          })),
        );
        this.canceled = this.canceled.slice(-100);
        this.changed();
        // Native completion or extension activity can happen while queue clearing is in flight.
        if (this.busy && this.runId === run) {
          // Dialog responses bypass mutation serialization, including cancellation during Stop.
          for (const { dialog, timer } of this.dialogs.values()) {
            this.rpc.answer({ id: dialog.id, cancelled: true });
            clearTimeout(timer);
          }
          this.dialogs.clear();
          this.changed();
          await this.rpc.call("abort");
        }
        this.queue = [];
        await this.refresh();
        return recovered;
      } finally {
        this.stoppingRun = null;
      }
    }
    const native = await this.rpc.call("get_state");
    if (
      this.busy ||
      native.isStreaming ||
      native.isCompacting ||
      native.pendingMessageCount > 0
    )
      throw new Error(
        "Pi is running. Stop explicitly before changing conversations.",
      );
    const result =
      r.op === "new"
        ? await this.rpc.call("new_session")
        : await this.rpc.call("switch_session", {
            sessionPath: await this.sessions.path(r.sessionId),
          });
    if (result.cancelled)
      throw new Error("A Pi extension canceled the conversation change.");
    this.epoch = randomUUID();
    this.runId = null;
    this.projection.reset();
    this.editorOffer = null;
    this.titleOverride = null;
    this.queue = [];
    for (const item of this.dialogs.values()) clearTimeout(item.timer);
    this.dialogs.clear();
    await this.refresh();
    return { sessionId: this.state.sessionId };
  }
  answer(r) {
    const item = this.dialogs.get(r.dialogId);
    if (!item)
      throw new Error(
        "This extension request expired or was already answered.",
      );
    const d = item.dialog;
    const result = { id: d.id };
    if (r.cancelled) result.cancelled = true;
    else if (d.method === "confirm") {
      if (typeof r.confirmed !== "boolean")
        throw new Error("Choose Allow or Decline");
      result.confirmed = r.confirmed;
    } else {
      if (
        typeof r.value !== "string" ||
        Buffer.byteLength(r.value, "utf8") > 65536
      )
        throw new Error("Enter an answer of at most 64 KiB");
      if (d.method === "select" && !d.options.includes(r.value))
        throw new Error("Choose an offered option");
      result.value = r.value;
    }
    this.rpc.answer(result);
    clearTimeout(item.timer);
    this.dialogs.delete(d.id);
    this.changed();
    return { answered: true };
  }
  dispose() {
    this.disposed = true;
    clearTimeout(this.broadcastTimer);
    for (const x of this.dialogs.values()) clearTimeout(x.timer);
  }
}
