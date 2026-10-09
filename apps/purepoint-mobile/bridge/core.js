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
const key = (m) => `${m.role}-${m.timestamp ?? 0}-${m.toolCallId ?? ""}`;
function row(m, id = key(m)) {
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
  }
  get messages() {
    return [...this.rows, ...(this.live ? [row(this.live)] : [])].slice(-500);
  }
  load(entries, leafId) {
    const path = activeBranch(entries, leafId);
    const rows = [];
    for (const e of path) {
      if (e.type === "message" && e.message.role !== "system")
        rows.push(row(e.message));
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
    const ids = new Set(rows.map((x) => x.id));
    const persisted = new Set(
      entries.filter((x) => x.type === "message").map((x) => key(x.message)),
    );
    this.rows = [
      ...rows,
      ...this.rows.filter((x) => !persisted.has(x.id) && x._ephemeral),
    ].slice(-500);
    if (this.live && persisted.has(key(this.live))) this.live = null;
  }
  reset() {
    this.rows = [];
    this.live = null;
    this.tools = [];
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
    if (
      e.type === "message_end" ||
      (e.type === "message_start" && e.message.role !== "assistant")
    ) {
      const m = e.message;
      if (m.role !== "system") {
        const r = { ...row(m), _ephemeral: true };
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
