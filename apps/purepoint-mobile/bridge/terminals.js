import {
  workspaceConnection,
  workspaceOperations,
  workspaceFeatures,
  projectMetadata,
} from "./workspace.js";
import { randomUUID } from "node:crypto";
import { StringDecoder } from "node:string_decoder";
import { createDaemons, connectDaemon, daemonRequest } from "./daemon.js";

const keys = Object.freeze({
  Esc: "\x1b",
  "Ctrl-C": "\x03",
  Tab: "\t",
  Enter: "\r",
  ArrowUp: "\x1b[A",
  ArrowDown: "\x1b[B",
  ArrowRight: "\x1b[C",
  ArrowLeft: "\x1b[D",
  y: "y",
  n: "n",
});
const ops = new Set([
  ...workspaceOperations,
  "permissions",
  "agents",
  "terminal.open",
  "terminal.close",
  "terminal.keys",
  "terminal.raw",
  "terminal.size",
  "agent.prompt",
]);
export const terminalOperation = (op) => ops.has(op);

/** Each instance is private to one authenticated WebSocket, including same-device sockets. */
export function terminalConnection({
  ws,
  identity,
  permission,
  management = () => false,
  chat = () => "off",
  ledger = new Map(),
  send,
  daemons = createDaemons(),
  queueLimit = 512 * 1024,
  now = Date.now,
}) {
  const streams = new Map();
  const closedStreams = new Set();
  const pending = new Set();
  let disposed = false;
  let terminalPermission = permission();
  const allowed = (control = false) => {
    const value = permission();
    if (disposed || value === "off" || (control && value !== "control"))
      throw new Error("Terminal permission required");
  };
  const permissions = () => ({
    terminals: permission(),
    workspaceManagement: management(),
    chat: chat(),
    workspace: workspaceFeatures,
  });
  const workspace = workspaceConnection({
    identity,
    permission,
    management,
    ledger,
    daemons,
  });
  const notify = () => send({ type: "permissions", ...permissions() });
  const stopAttach = (stream) => {
    const attach = stream.attach;
    stream.attach = null;
    attach?.close();
  };
  const closeStream = (stream, reason) => {
    if (!streams.delete(stream.id)) return;
    if (closedStreams.size >= 4096)
      closedStreams.delete(closedStreams.values().next().value);
    closedStreams.add(stream.id);
    stopAttach(stream);
    clearTimeout(stream.timer);
    stream.queue = [];
    stream.bytes = 0;
    send({ type: "terminal.closed", streamId: stream.id, reason });
  };
  const pump = (stream) => {
    if (!streams.has(stream.id) || stream.sending) return;
    clearTimeout(stream.timer);
    if (ws.readyState !== 1) return closeStream(stream, "Connection closed");
    if (ws.bufferedAmount > 64 * 1024) {
      if (now() - stream.lastProgress > 10000)
        return closeStream(stream, "Output backlog; reopen terminal");
      stream.timer = setTimeout(() => pump(stream), 20);
      return;
    }
    if (stream.recovering) {
      stream.recovering = false;
      send({
        type: "terminal.reset",
        streamId: stream.id,
        reason: "Output backlog; refreshing snapshot",
      });
      openAttach(stream);
      return;
    }
    const record = stream.queue.shift();
    if (!record) {
      if (stream.ready && stream.stableSince == null)
        stream.stableSince = now();
      if (stream.ended) closeStream(stream, stream.ended);
      return;
    }
    stream.bytes -= Buffer.byteLength(record);
    stream.sending = true;
    stream.timer = setTimeout(
      () => closeStream(stream, "Output stalled; reopen terminal"),
      10000,
    );
    ws.send(record, (error) => {
      clearTimeout(stream.timer);
      stream.sending = false;
      stream.lastProgress = now();
      if (error) closeStream(stream, "Connection closed");
      else pump(stream);
    });
  };
  const enqueue = (stream, record) => {
    // A fresh burst after a drained stable window gets its own recovery budget.
    if (stream.stableSince != null && now() - stream.stableSince >= 5000)
      stream.resets = 0;
    stream.stableSince = null;
    const serialized = JSON.stringify(record);
    const size = Buffer.byteLength(serialized);
    if (stream.recovering) return;
    if (stream.bytes + size > queueLimit) {
      stopAttach(stream);
      stream.queue = [];
      stream.bytes = 0;
      stream.decoder = new StringDecoder("utf8");
      stream.resets++;
      if (stream.resets > 2)
        return closeStream(
          stream,
          "Output exceeds connection capacity; reopen terminal",
        );
      stream.recovering = true;
      pump(stream);
      return;
    }
    stream.queue.push(serialized);
    stream.bytes += size;
    pump(stream);
  };
  function openAttach(stream) {
    if (!streams.has(stream.id)) return;
    try {
      allowed();
    } catch {
      return closeStream(stream, "Terminal permission removed");
    }
    stream.ready = false;
    const connection = connectDaemon(stream.machine.socket, {
      onFrame(frame) {
        if (stream.attach !== connection) return;
        if (frame.type === "attach_ready") {
          connection.streaming();
          // A reopened attachment (recovery, permission change) re-asserts the
          // phone's size claim; view-only connections never hold one.
          if (stream.claim && permission() === "control")
            connection.send({ type: "claim_size", ...stream.claim });
          stream.ready = true;
          pump(stream);
        } else if (frame.type === "output") {
          if (
            typeof frame.data !== "string" ||
            !/^(?:[a-fA-F0-9]{2})*$/.test(frame.data)
          )
            throw new Error("Invalid terminal bytes");
          const bytes = Buffer.from(frame.data, "hex");
          for (let offset = 0; offset < bytes.length; offset += 8192) {
            const data = stream.decoder.write(
              bytes.subarray(offset, offset + 8192),
            );
            if (data)
              enqueue(stream, {
                type: "terminal.output",
                streamId: stream.id,
                data,
              });
            if (
              stream.attach !== connection ||
              stream.recovering ||
              !streams.has(stream.id)
            )
              break;
          }
        } else if (frame.type === "pty_size" && stream.version >= 10) {
          enqueue(stream, {
            type: "terminal.size",
            streamId: stream.id,
            cols: frame.cols,
            rows: frame.rows,
          });
        } else if (frame.type === "resync") {
          stream.decoder = new StringDecoder("utf8");
          enqueue(stream, {
            type: "terminal.reset",
            streamId: stream.id,
            reason: "Daemon output resynchronized",
          });
        } else if (frame.type === "error")
          closeStream(stream, frame.message ?? "Daemon error");
      },
      onClose(error) {
        if (stream.attach !== connection || !streams.has(stream.id)) return;
        const tail = stream.decoder.end();
        if (tail)
          enqueue(stream, {
            type: "terminal.output",
            streamId: stream.id,
            data: tail,
          });
        stream.ended = error?.message ?? "Agent terminal ended";
        pump(stream);
      },
    });
    stream.attach = connection;
    connection.send({
      type: "attach",
      agent_id: stream.agentId,
      ...(stream.version >= 10
        ? {
            replay: "snapshot",
            client_id: `${identity.deviceId ?? identity.clientId}:${stream.id}`,
          }
        : {}),
    });
  }
  const operation = async (control, fn) => {
    allowed(control);
    const abort = new AbortController();
    pending.add(abort);
    const check = () => {
      allowed(control);
      if (abort.signal.aborted) throw new Error("Terminal permission changed");
    };
    try {
      const result = await fn(abort.signal, check);
      check();
      return result;
    } finally {
      pending.delete(abort);
    }
  };
  const ownStream = (id) => {
    const stream = streams.get(id);
    if (!stream) throw new Error("Unknown terminal stream for this connection");
    return stream;
  };
  const request = async (r) => {
    if (r.op === "permissions") return permissions();
    if (workspaceOperations.includes(r.op)) return workspace.request(r);
    return operation(
      [
        "terminal.keys",
        "terminal.raw",
        "terminal.size",
        "agent.prompt",
      ].includes(r.op),
      async (signal, check) => {
        if (r.op === "agents") {
          const machines = await daemons.machines();
          if (machines.length > 32)
            throw new Error("Fleet exceeds 32-machine query limit");
          check();
          const result = [];
          for (let offset = 0; offset < machines.length; offset += 4) {
            result.push(
              ...(await Promise.all(
                machines.slice(offset, offset + 4).map(async (machine) => {
                  try {
                    const { version, inventory } = await daemons.inspect(
                      machine,
                      {
                        signal,
                      },
                    );
                    const metadata = await projectMetadata(machine, { signal });
                    return {
                      id: machine.id,
                      name: machine.name,
                      ...metadata,
                      online: true,
                      protocolVersion: version,
                      screenMayBeInexact: version < 10,
                      error: null,
                      complete: inventory.complete === true,
                      agents: inventory.agents.map((a) => ({
                        id: a.id,
                        name: a.name,
                        agentType: a.agent_type,
                        state: a.state,
                        idleSeconds: a.idle_seconds ?? null,
                        startedAt: a.started_at,
                        signal: a.signal ?? null,
                        projectRoot: a.project_root ?? null,
                        worktreeId: a.worktree_id ?? null,
                        screenAware: version >= 10 && a.screen_aware === true,
                        phase: version >= 10 ? (a.phase ?? null) : null,
                      })),
                    };
                  } catch (error) {
                    return {
                      id: machine.id,
                      name: machine.name,
                      metadataStatus: error.inventoryUnavailable
                        ? "error"
                        : "offline",
                      metadataError: error.inventoryUnavailable
                        ? "Daemon inventory unavailable"
                        : "Machine unavailable",
                      projects: [],
                      worktrees: [],
                      online: error.inventoryUnavailable === true,
                      protocolVersion: error.protocolVersion ?? null,
                      screenMayBeInexact: true,
                      error: error.inventoryUnavailable
                        ? "Daemon inventory unavailable; refresh to retry"
                        : "Machine unavailable; keep the host and its existing forward connected",
                      complete: false,
                      agents: [],
                    };
                  }
                }),
              )),
            );
            check();
          }
          return { machines: result };
        }
        if (r.op === "terminal.open" || r.op === "agent.prompt") {
          if (
            typeof r.machine !== "string" ||
            typeof r.agentId !== "string" ||
            !/^[A-Za-z0-9_-]{1,128}$/.test(r.agentId)
          )
            throw new Error("Machine and agent required");
          if (
            r.op === "agent.prompt" &&
            (typeof r.text !== "string" ||
              !r.text.trim() ||
              Buffer.byteLength(r.text) > 65536)
          )
            throw new Error("Prompt requires 1–65536 UTF-8 bytes");
          const machine = await daemons.resolve(r.machine);
          check();
          try {
            const { version, inventory } = await daemons.inspect(machine, {
              signal,
            });
            check();
            const agent = inventory.agents.find((a) => a.id === r.agentId);
            if (!agent) throw new Error("Agent not found");
            if (r.op === "agent.prompt") {
              const aware = version >= 10 && agent.screen_aware === true;
              if (aware && agent.phase !== "input_ready")
                return {
                  outcome: "blocked",
                  reason:
                    agent.phase === "dialog"
                      ? "agent is waiting on a prompt, use the keys"
                      : `agent input is ${agent.phase ?? "not ready"}; use the keys or wait`,
                };
              check();
              const reply = await daemonRequest(
                machine.socket,
                {
                  type: "input",
                  agent_id: r.agentId,
                  data: Buffer.from(r.text).toString("hex"),
                  submit: true,
                  ...(version >= 10 ? { cancel_on_disconnect: true } : {}),
                },
                { signal, timeout: 360000 },
              );
              check();
              if (reply.type !== "ok")
                throw new Error("Unexpected daemon delivery response");
              return { outcome: aware ? "confirmed" : "sent", reason: null };
            }
            if (streams.size >= 8) throw new Error("Too many open terminals");
            const stream = {
              id: randomUUID(),
              machine,
              agentId: r.agentId,
              version,
              queue: [],
              bytes: 0,
              sending: false,
              recovering: false,
              resets: 0,
              ready: false,
              stableSince: null,
              decoder: new StringDecoder("utf8"),
              lastProgress: now(),
              attach: null,
              timer: null,
              ended: null,
              claim: null,
            };
            streams.set(stream.id, stream);
            openAttach(stream);
            return {
              streamId: stream.id,
              machine: machine.id,
              agentId: r.agentId,
              screenMayBeInexact: version < 10,
            };
          } catch (error) {
            check();
            if (r.op === "agent.prompt")
              return { outcome: "failed", reason: error.message };
            throw error;
          }
        }
        if (r.op === "terminal.close" && closedStreams.has(r.streamId))
          return {};
        const stream = ownStream(r.streamId);
        check();
        if (r.op === "terminal.close") {
          closeStream(stream, "Closed by viewer");
          return {};
        }
        if (r.op === "terminal.size") {
          // Explicit, Control-only size ownership for this stream's attachment.
          // Closing the stream releases it and the daemon restores the previous owner.
          if (
            !Number.isInteger(r.cols) ||
            !Number.isInteger(r.rows) ||
            r.cols < 20 ||
            r.cols > 500 ||
            r.rows < 8 ||
            r.rows > 300
          )
            throw new Error(
              "Terminal size requires 20–500 columns and 8–300 rows",
            );
          if (stream.version < 10)
            throw new Error("Agent daemon does not support phone sizing");
          if (!stream.attach || stream.recovering)
            throw new Error("Terminal is refreshing; wait for snapshot");
          stream.claim = { cols: r.cols, rows: r.rows };
          stream.attach.send({ type: "claim_size", ...stream.claim });
          return {};
        }
        const encoding = r.op === "terminal.raw" ? r.encoding : undefined;
        const data =
          r.op === "terminal.keys"
            ? Object.hasOwn(keys, r.key)
              ? keys[r.key]
              : null
            : r.data;
        if (
          typeof data !== "string" ||
          !data.length ||
          Buffer.byteLength(data) > (encoding === "base64" ? 87384 : 65536)
        )
          throw new Error("Invalid terminal input or key");
        if (r.op === "terminal.keys" && !Object.hasOwn(keys, r.key))
          throw new Error("Unsupported terminal key");
        if (encoding != null && encoding !== "base64" && encoding !== "utf8")
          throw new Error("Unsupported terminal input encoding");
        const bytes = Buffer.from(
          data,
          encoding === "base64" ? "base64" : "utf8",
        );
        if (
          !bytes.length ||
          bytes.length > 65536 ||
          (encoding === "base64" && bytes.toString("base64") !== data)
        )
          throw new Error("Invalid terminal input encoding or size");
        if (!stream.attach || stream.recovering)
          throw new Error("Terminal is refreshing; wait for snapshot");
        stream.attach.send({
          type: "input",
          agent_id: stream.agentId,
          data: bytes.toString("hex"),
          submit: false,
        });
        return {};
      },
    );
  };
  return {
    request,
    notify,
    changed() {
      workspace.changed();
      const current = permission();
      if (current === terminalPermission) {
        notify();
        return;
      }
      terminalPermission = current;
      for (const abort of pending) abort.abort();
      for (const stream of streams.values()) {
        // Close the write-capable transport before returning from a downgrade.
        stopAttach(stream);
        stream.queue = [];
        stream.bytes = 0;
        stream.recovering = false;
        if (permission() === "off")
          closeStream(stream, "Terminal permission removed");
        else {
          stream.decoder = new StringDecoder("utf8");
          stream.ended = null;
          enqueue(stream, {
            type: "terminal.reset",
            streamId: stream.id,
            reason: "Terminal permission changed",
          });
          openAttach(stream);
        }
      }
      notify();
    },
    dispose() {
      workspace.dispose();
      disposed = true;
      for (const abort of pending) abort.abort();
      for (const stream of streams.values())
        closeStream(stream, "Connection closed");
    },
  };
}
