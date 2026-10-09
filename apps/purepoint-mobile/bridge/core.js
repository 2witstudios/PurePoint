import { createHash } from "node:crypto";

export const clip = (value, limit = 65536) => {
  const s = String(value ?? "");
  return s.length > limit ? s.slice(0, limit) + "\n[Display truncated]" : s;
};
export class LineDecoder {
  constructor(onRecord, limit = 8 * 1024 * 1024) {
    this.onRecord = onRecord;
    this.limit = limit;
    this.buffer = Buffer.alloc(0);
  }
  push(chunk) {
    // Buffer bytes until LF, so split UTF-8 code points and Unicode separators are safe.
    let offset = 0;
    while (offset < chunk.length) {
      const end = chunk.indexOf(10, offset);
      const part = chunk.subarray(offset, end < 0 ? chunk.length : end);
      if (this.buffer.length + part.length > this.limit)
        throw new Error("RPC record exceeds limit");
      this.buffer = Buffer.concat([this.buffer, part]);
      if (end < 0) break;
      const line = this.buffer.toString("utf8").replace(/\r$/, "");
      this.buffer = Buffer.alloc(0);
      if (line.trim()) this.onRecord(JSON.parse(line));
      offset = end + 1;
    }
  }
  end() {
    if (this.buffer.length)
      throw new Error("RPC ended with a fragmented record");
  }
}
export function safeEnvironment(source) {
  const env = { ...source };
  delete env.PU_AGENT_ID;
  delete env.PU_PROJECT_ROOT;
  for (const k of Object.keys(env))
    if (k.startsWith("PI_MOBILE_")) delete env[k];
  return env;
}
export function activeBranch(entries, leafId) {
  const byId = new Map(entries.map((x) => [x.id, x]));
  const path = [];
  const seen = new Set();
  let id = leafId;
  while (id) {
    if (seen.has(id)) throw new Error("Invalid cyclic Pi session");
    seen.add(id);
    const entry = byId.get(id);
    if (!entry) throw new Error("Incomplete Pi branch");
    path.unshift(entry);
    id = entry.parentId;
  }
  return path;
}
function contentText(content) {
  if (typeof content === "string") return clip(content);
  return clip(
    (content ?? [])
      .map((b) =>
        b.type === "text"
          ? b.text
          : b.type === "thinking"
            ? ""
            : b.type === "image"
              ? "[Image]"
              : b.type === "toolCall"
                ? ""
                : "",
      )
      .filter(Boolean)
      .join("\n\n"),
  );
}
function toolIds(entries) {
  const ids = new Set();
  for (const e of entries) {
    if (e.type !== "message") continue;
    const m = e.message;
    if (m.role === "toolResult") ids.add(m.toolCallId);
    if (Array.isArray(m.content))
      for (const block of m.content)
        if (block.type === "toolCall") ids.add(block.id);
  }
  return ids;
}
// Native custom entry timestamps are assigned at persistence, independently of
// live message timestamps (especially for queued messages). Correlate by payload.
const customKey = (m) =>
  createHash("sha256")
    .update(JSON.stringify([m.customType, m.content, m.display, m.details]))
    .digest("hex");
const visible = (m) =>
  m.role !== "system" && (m.role !== "custom" || m.display);
const key = (m) => `${m.role}-${m.timestamp ?? 0}-${m.toolCallId ?? ""}`;
function row(m, id = key(m)) {
  if (m.role === "bashExecution") {
    const status = m.cancelled
      ? "Command cancelled"
      : `Exit code: ${m.exitCode ?? "unknown"}`;
    const truncation = m.truncated
      ? `\n\n[Output truncated${m.fullOutputPath ? `. Full output: ${clip(m.fullOutputPath, 1000)}` : ""}]`
      : "";
    return {
      id,
      role: m.role,
      text: `$ ${clip(m.command, 8192)}\n\n${clip(m.output || "(no output)", 48000)}\n\n${status}${truncation}`,
      error: m.cancelled
        ? "Command cancelled"
        : m.exitCode != null && m.exitCode !== 0
          ? `Command exited with code ${m.exitCode}`
          : undefined,
    };
  }
  return {
    id,
    role: m.role,
    text: contentText(m.content),
    activity: m.role === "toolResult" ? m.toolName : undefined,
    error: m.errorMessage
      ? clip(m.errorMessage, 4000)
      : m.isError
        ? "Tool failed"
        : undefined,
  };
}
export class Projection {
  constructor() {
    this.rows = [];
    this.live = null;
    this.tools = [];
    this.persistedCustom = new Set();
    this.customSequence = 0;
  }
  get messages() {
    return [...this.rows, ...(this.live ? [row(this.live)] : [])].slice(-500);
  }
  load(entries, leafId) {
    const path = activeBranch(entries, leafId);
    const rows = [];
    for (const e of path) {
      if (e.type === "message" && visible(e.message)) rows.push(row(e.message));
      else if (e.type === "custom_message" && e.display)
        rows.push({ id: e.id, role: "custom", text: contentText(e.content) });
      else if (e.type === "compaction" || e.type === "branch_summary")
        rows.push({
          id: e.id,
          role: "notice",
          text: clip(
            `${e.type === "compaction" ? "Earlier context summarized" : "Branch summary"}\n${e.summary}`,
          ),
        });
    }
    // Preserve event-only records that have not reached the native session yet.
    const selectedTools = toolIds(path);
    const persistedTools = toolIds(entries);
    this.tools = this.tools.filter(
      (tool) => selectedTools.has(tool.id) || !persistedTools.has(tool.id),
    );
    const persisted = new Set(
      entries.filter((x) => x.type === "message").map((x) => key(x.message)),
    );
    // Match each newly persisted custom occurrence once, across every branch.
    // Previously reconciled entries must not swallow later identical live messages.
    const customCounts = new Map();
    const customEntries = entries.filter((e) => e.type === "custom_message");
    for (const e of customEntries) {
      if (this.persistedCustom.has(e.id)) continue;
      const identity = customKey(e);
      customCounts.set(identity, (customCounts.get(identity) ?? 0) + 1);
    }
    this.persistedCustom = new Set(customEntries.map((e) => e.id));
    this.rows = [
      ...rows,
      ...this.rows.filter((x) => {
        if (!x._ephemeral || persisted.has(x.id)) return false;
        const count = customCounts.get(x._customKey) ?? 0;
        if (!count) return true;
        customCounts.set(x._customKey, count - 1);
        return false;
      }),
    ].slice(-500);
    if (this.live && persisted.has(key(this.live))) this.live = null;
  }
  reset() {
    this.rows = [];
    this.live = null;
    this.tools = [];
    this.persistedCustom = new Set();
    this.customSequence = 0;
  }
  event(e) {
    if (e.type === "message_start" && e.message.role === "assistant")
      this.live = structuredClone(e.message);
    if (e.type === "message_update" && this.live) {
      const u = e.assistantMessageEvent;
      const i = u.contentIndex;
      if (Number.isInteger(i) && i >= 0 && i < 256) {
        const blocks = this.live.content;
        const kind = u.type.split("_")[0];
        if (kind === "text" || kind === "thinking") {
          blocks[i] ??= { type: kind, [kind]: "" };
          if (u.type.endsWith("_delta"))
            blocks[i][kind] = clip((blocks[i][kind] ?? "") + (u.delta ?? ""));
          if (u.type.endsWith("_end")) blocks[i][kind] = clip(u.content);
        }
        if (u.type === "toolcall_end") blocks[i] = u.toolCall;
      }
    }
    // Custom messages are complete at message_end; unique occurrence IDs also
    // preserve identical messages sent within the same millisecond.
    if (
      e.type === "message_end" ||
      (e.type === "message_start" &&
        e.message.role !== "assistant" &&
        e.message.role !== "custom")
    ) {
      const m = e.message;
      if (visible(m)) {
        const r = {
          ...row(
            m,
            m.role === "custom"
              ? `custom-live-${++this.customSequence}`
              : key(m),
          ),
          _ephemeral: true,
          ...(m.role === "custom" ? { _customKey: customKey(m) } : {}),
        };
        const i = this.rows.findIndex((x) => x.id === r.id);
        if (i >= 0) this.rows[i] = r;
        else this.rows.push(r);
        this.rows = this.rows.slice(-500);
        if (this.live && key(this.live) === r.id) this.live = null;
      }
    }
    if (e.type.startsWith("tool_execution_")) {
      let tool = this.tools.find((x) => x.id === e.toolCallId);
      if (!tool) {
        tool = {
          id: e.toolCallId,
          name: e.toolName,
          state: "running",
          text: clip(JSON.stringify(e.args ?? {})),
        };
        this.tools.push(tool);
        this.tools = this.tools.slice(-100);
      }
      if (e.type === "tool_execution_update")
        tool.text = contentText(e.partialResult?.content);
      if (e.type === "tool_execution_end") {
        tool.state = e.isError ? "failed" : "finished";
        tool.text = contentText(e.result?.content);
      }
    }
  }
}

export function boundedRows(rows, budget) {
  const selected = [];
  let used = 0;
  for (let i = rows.length - 1; i >= 0; i--) {
    const size = Buffer.byteLength(JSON.stringify(rows[i]));
    if (used + size > budget) break;
    selected.unshift(rows[i]);
    used += size;
  }
  return selected;
}
