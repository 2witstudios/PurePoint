import { EventEmitter } from "node:events";
import { randomUUID } from "node:crypto";
import { basename } from "node:path";
import { Projection, clip, boundedRows } from "./core.js";
import { validateImages } from "./attachments.js";
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
    this.stops = new Map();
    this.transition = null;
    this.busy = false;
    this.state = {};
    this.queue = [];
    this.queueOrigins = [];
    this.nativeQueue = [];
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
    if (this.transition) {
      this.transition.notices.push(clip(text, 4000));
      this.transition.notices = this.transition.notices.slice(-12);
    }
  }
  setUI(field, key, value, limit) {
    const map = this[field];
    if (value) map.set(key, value);
    else map.delete(key);
    if (this.transition) {
      if (value) this.transition[field].set(key, value);
      else this.transition[field].delete(key);
    }
    if (map.size > limit) {
      const oldest = map.keys().next().value;
      map.delete(oldest);
      this.transition?.[field].delete(oldest);
    }
  }
  cancelDialogs() {
    for (const [id, { dialog, timer }] of [...this.dialogs]) {
      this.dialogs.delete(id);
      this.transition?.dialogs.delete(id);
      clearTimeout(timer);
      this.rpc.answer({ id: dialog.id, cancelled: true });
    }
    this.changed();
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
    if (e.type === "extension_error")
      this.notice(
        `Extension ${clip(basename(e.extensionPath || "unknown"), 200)} failed during ${clip(e.event || "unknown hook", 100)}: ${clip(e.error || "Unknown error", 3500)}`,
      );
    if (e.type === "queue_update") {
      const previousIds = new Set(this.nativeQueue.map((item) => item.id));
      const available = this.queueOrigins.filter(
        (item) => !previousIds.has(item.id),
      );
      const next = [];
      for (const [mode, texts] of [
        ["steer", e.steering ?? []],
        ["after", e.followUp ?? []],
      ]) {
        const previous = this.nativeQueue.filter((item) => item.mode === mode);
        // Pi dequeues from the front and appends at the back. Preserve the
        // surviving suffix, so identical prompts from different clients retain
        // their correct owners after the first occurrence starts running.
        let overlap = Math.min(previous.length, texts.length);
        while (
          overlap > 0 &&
          !previous
            .slice(previous.length - overlap)
            .every((item, index) => item.text === texts[index])
        )
          overlap--;
        next.push(...previous.slice(previous.length - overlap));
        for (const text of texts.slice(overlap)) {
          const index = available.findIndex(
            (item) => item.mode === mode && item.text === text,
          );
          const origin = index < 0 ? null : available.splice(index, 1)[0];
          next.push({
            id: origin?.id ?? randomUUID(),
            clientId: origin?.clientId ?? null,
            mode,
            text,
          });
        }
      }
      this.nativeQueue = next;
      const nextIds = new Set(next.map((item) => item.id));
      this.queueOrigins = this.queueOrigins.filter(
        (item) => !previousIds.has(item.id) || nextIds.has(item.id),
      );
      this.queue = next
        .slice(0, 100)
        .map((item) => ({ ...item, text: clip(item.text) }));
    }
    if (e.type === "extension_ui_request") {
      const method = e.method;
      if (["confirm", "select", "input", "editor"].includes(method)) {
        if ([...this.stops.values()].includes(this.runId)) {
          this.rpc.answer({ id: e.id, cancelled: true });
        } else if (this.dialogs.size >= 32) {
          this.rpc.answer({ id: e.id, cancelled: true });
          this.notice(
            "An extension opened too many dialogs; the extra request was canceled.",
          );
        } else {
          // Editor answers replace the document: never truncate its initial value.
          const prefill =
            method === "editor" && typeof e.prefill === "string"
              ? e.prefill
              : clip(e.prefill, 8192);
          if (
            method === "editor" &&
            Buffer.byteLength(prefill, "utf8") > 65536
          ) {
            this.rpc.answer({ id: e.id, cancelled: true });
            this.notice(
              "An extension editor exceeded the 64 KiB text budget and was canceled.",
            );
            this.changed();
            return;
          }
          const originalOptions =
            method === "select" && Array.isArray(e.options)
              ? e.options.slice(0, 100)
              : [];
          if (
            method === "select" &&
            (!Array.isArray(e.options) ||
              !originalOptions.every((x) => typeof x === "string") ||
              originalOptions.reduce(
                (bytes, x) => bytes + Buffer.byteLength(x, "utf8"),
                0,
              ) >
                256 * 1024)
          ) {
            this.rpc.answer({ id: e.id, cancelled: true });
            this.notice(
              "An extension selection exceeded the supported option budget and was canceled.",
            );
            return;
          }
          const dialog = {
            ...e,
            title: clip(e.title, 1000),
            message: clip(e.message, 4000),
            prefill,
            options:
              method === "select"
                ? originalOptions.map((x) => clip(x, 200))
                : undefined,
            optionIds:
              method === "select"
                ? originalOptions.map(() => randomUUID())
                : undefined,
          };
          const timer = e.timeout
            ? setTimeout(() => {
                this.dialogs.delete(e.id);
                this.transition?.dialogs.delete(e.id);
                this.changed();
              }, e.timeout)
            : null;
          this.dialogs.set(e.id, { dialog, timer, originalOptions });
          this.transition?.dialogs.add(e.id);
        }
      } else if (method === "setStatus") {
        this.setUI("statuses", e.statusKey, clip(e.statusText, 1000), 32);
      } else if (method === "setWidget") {
        const key = clip(e.widgetKey, 200);
        this.setUI(
          "widgets",
          key,
          e.widgetLines ? clip(e.widgetLines.join("\n"), 4000) : "",
          16,
        );
      } else if (method === "notify") this.notice(e.message);
      else if (method === "setTitle") {
        this.titleOverride = clip(e.title, 200);
        if (this.transition) this.transition.titleOverride = this.titleOverride;
      } else if (method === "set_editor_text") {
        this.editorOffer = { id: e.id, text: clip(e.text) };
        if (this.transition) this.transition.editorOffer = this.editorOffer;
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
      capabilities: ["images"],
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
  async refresh(reset = false) {
    if (this.refreshing)
      return reset ? this.refreshFresh(true) : this.refreshing;
    this.refreshing = (async () => {
      const activityVersion = this.activityVersion;
      const state = await this.rpc.call("get_state");
      const history = await this.rpc.call("get_entries");
      if (
        reset ||
        (this.state.sessionId !== undefined &&
          this.state.sessionId !== state.sessionId)
      )
        this.resetConversation();
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
  async refreshFresh(reset = false) {
    // A mutation needs a native read started after it completed. Syncs can coalesce.
    // An older read's failure must not prevent this read; its caller still sees it.
    while (this.refreshing) await this.refreshing.catch(() => {});
    return this.refresh(reset);
  }
  validate(r) {
    if (r.version !== 1)
      throw new Error(
        "Unsupported bridge protocol version. Update the app and bridge together.",
      );
    if (
      typeof r.clientId !== "string" ||
      !/^[A-Za-z0-9_-]{1,100}$/.test(r.clientId)
    )
      throw new Error("Request requires a client identity");
    if (typeof r.id !== "string" || r.id.length > 100 || !r.id)
      throw new Error("Request requires a unique ID");
    const requestKey = `${r.clientId}:${r.id}`;
    if (this.seen.has(requestKey))
      throw new Error(
        "Request already received. Inspect history; it will not be replayed.",
      );
    this.seen.add(requestKey);
    if (this.seen.size > 2000)
      this.seen.delete(this.seen.values().next().value);
  }
  async request(r) {
    this.validate(r);
    if (r.op === "answer") {
      this.checkEpoch(r);
      return this.answer(r);
    }
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
    if (r.op === "stop") {
      // Dialog answers must bypass a prompt awaiting an extension hook's UI.
      this.checkEpoch(r);
      this.checkRun(r);
    }
    this.pendingMutations++;
    try {
      if (r.op === "stop") {
        this.stops.set(`${r.clientId}:${r.id}`, r.runId);
        this.cancelDialogs();
      }
      const operation = this.serial.then(async () => {
        // Native session_start hooks run before mutation replies. Retain their
        // bounded UI changes for reset, while interactive dialogs stay live.
        if (r.op !== "stop")
          this.transition = {
            notices: [],
            statuses: new Map(),
            widgets: new Map(),
            dialogs: new Set(),
            editorOffer: null,
            titleOverride: null,
          };
        try {
          return await this.mutate(r);
        } finally {
          this.transition = null;
        }
      });
      this.serial = operation.catch(() => {});
      return await operation;
    } finally {
      this.pendingMutations--;
      this.stops.delete(`${r.clientId}:${r.id}`);
    }
  }
  checkEpoch(r) {
    if (r.epoch !== this.epoch)
      throw new Error(
        "Conversation changed. Refresh before trying this action.",
      );
    if (this.error) throw new Error(this.error);
  }
  checkRun(r) {
    if (!this.busy || r.runId !== this.runId)
      throw new Error(
        "Run changed or already finished. This Stop was ignored.",
      );
  }
  async mutate(r) {
    this.checkEpoch(r);
    if (r.op === "send") {
      const images = validateImages(r.images);
      if (images.length && (this.busy || r.mode !== "send"))
        throw new Error("Send image attachments when Pi is idle.");
      if (
        images.length &&
        Array.isArray(this.state.model?.input) &&
        !this.state.model.input.includes("image")
      )
        throw new Error(
          "The selected Pi model does not support images. Select an image-capable model.",
        );
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
      const queued = r.mode !== "send";
      const originId = `${r.clientId}:${r.id}`;
      if (queued) {
        if (this.queueOrigins.length >= 100 || this.nativeQueue.length >= 100)
          throw new Error("Queue is full. Wait for Pi before sending more.");
        this.queueOrigins.push({
          id: originId,
          clientId: r.clientId,
          mode: r.mode,
          text: r.text,
        });
      }
      let result;
      try {
        result = await this.rpc.call("prompt", {
          message: r.text,
          ...(images.length ? { images } : {}),
          ...(r.mode === "send"
            ? {}
            : { streamingBehavior: r.mode === "steer" ? "steer" : "followUp" }),
        });
      } catch (error) {
        if (!this.queue.some((item) => item.id === originId))
          this.queueOrigins = this.queueOrigins.filter(
            (item) => item.id !== originId,
          );
        throw error;
      }
      if (result.disposition !== "queued")
        this.queueOrigins = this.queueOrigins.filter(
          (item) => item.id !== originId,
        );
      if (result.disposition === "handled") {
        // An extension may change sessions/branches without emitting agent events.
        await this.refreshFresh();
      }
      return result;
    }
    if (r.op === "stop") {
      this.checkRun(r);
      const run = this.runId;
      const origins = [...this.queueOrigins];
      const recovered = await this.rpc.call("clear_queue");
      const texts = [
        ...(recovered.steering ?? []).map((text) => ({ mode: "steer", text })),
        ...(recovered.followUp ?? []).map((text) => ({ mode: "after", text })),
      ];
      this.canceled.push(
        ...texts.map(({ mode, text }, index) => {
          const position = origins.findIndex(
            (origin) => origin.mode === mode && origin.text === text,
          );
          const origin = position < 0 ? null : origins.splice(position, 1)[0];
          return {
            id: origin?.id ?? `${r.clientId}:${r.id}:${index}`,
            clientId: origin?.clientId ?? null,
            text: clip(text),
            sessionId: this.state.sessionId,
          };
        }),
      );
      this.queueOrigins = [];
      this.nativeQueue = [];
      this.canceled = this.canceled.slice(-100);
      this.changed();
      // Native completion or extension activity can happen while queue clearing is in flight.
      if (this.busy && this.runId === run) {
        // Dialog responses bypass mutation serialization, including cancellation during Stop.
        this.cancelDialogs();
        await this.rpc.call("abort");
      }
      this.queue = [];
      await this.refreshFresh();
      return recovered;
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
    // Explicitly reselecting the same session still invalidates old actions.
    await this.refreshFresh(true);
    return { sessionId: this.state.sessionId };
  }
  resetConversation() {
    this.epoch = randomUUID();
    this.runId = null;
    this.projection.reset();
    this.editorOffer = this.transition?.editorOffer ?? null;
    this.titleOverride = this.transition?.titleOverride ?? null;
    this.queue = [];
    this.queueOrigins = [];
    this.nativeQueue = [];
    this.notices = this.transition ? [...this.transition.notices] : [];
    this.statuses = new Map(this.transition?.statuses);
    this.widgets = new Map(this.transition?.widgets);
    for (const [id, item] of this.dialogs) {
      if (!this.transition?.dialogs.has(id)) {
        clearTimeout(item.timer);
        this.dialogs.delete(id);
      }
    }
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
    } else if (d.method === "select") {
      const index = d.optionIds.indexOf(r.optionId);
      if (index < 0) throw new Error("Choose an offered option ID");
      result.value = item.originalOptions[index];
    } else {
      if (
        typeof r.value !== "string" ||
        Buffer.byteLength(r.value, "utf8") > 65536
      )
        throw new Error("Enter an answer of at most 64 KiB");
      result.value = r.value;
    }
    this.rpc.answer(result);
    clearTimeout(item.timer);
    this.dialogs.delete(d.id);
    this.transition?.dialogs.delete(d.id);
    this.changed();
    return { answered: true };
  }
  dispose() {
    this.disposed = true;
    clearTimeout(this.broadcastTimer);
    for (const x of this.dialogs.values()) clearTimeout(x.timer);
  }
}
