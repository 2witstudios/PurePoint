import net from "node:net";
import path from "node:path";
import os from "node:os";
import { readFile } from "node:fs/promises";
import { StringDecoder } from "node:string_decoder";

/** Bounded newline JSON transport. No CLI, SSH, or daemon startup. */
export function connectDaemon(
  socketPath,
  {
    onFrame,
    onClose = (error = undefined) => {},
    signal = undefined,
    timeout = 5000,
  },
) {
  const socket = net.createConnection(socketPath);
  let pending = "";
  const decoder = new StringDecoder("utf8");
  let closed = false;
  const finish = (error) => {
    if (closed) return;
    closed = true;
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
    socket.destroy();
    onClose(error);
  };
  const abort = () =>
    finish(new Error("Terminal permission changed or connection closed"));
  const timer = setTimeout(
    () => finish(new Error("Daemon request timed out")),
    timeout,
  );
  timer.unref();
  socket.on("error", finish);
  socket.on("close", () => finish(new Error("Daemon disconnected")));
  socket.on("data", (data) => {
    pending += decoder.write(data);
    // Legacy replay is chunked too; no single frame should approach this bound.
    if (pending.length > 2 * 1024 * 1024)
      return finish(new Error("Daemon frame too large"));
    let end;
    while (!closed && (end = pending.indexOf("\n")) !== -1) {
      const line = pending.slice(0, end);
      pending = pending.slice(end + 1);
      try {
        onFrame(JSON.parse(line));
      } catch (error) {
        finish(error);
      }
    }
  });
  signal?.addEventListener("abort", abort, { once: true });
  if (signal?.aborted) abort();
  return {
    send(request) {
      if (closed || signal?.aborted)
        throw new Error("Daemon connection closed");
      const serialized = JSON.stringify(request) + "\n";
      if (socket.writableLength + Buffer.byteLength(serialized) > 256 * 1024)
        throw new Error("Daemon input backlog; wait before typing");
      socket.write(serialized);
    },
    streaming() {
      clearTimeout(timer);
    },
    close() {
      finish();
    },
  };
}

export function daemonRequest(
  socketPath,
  request,
  { signal = undefined, timeout = 5000 } = {},
) {
  return new Promise((resolve, reject) => {
    const connection = connectDaemon(socketPath, {
      signal,
      timeout,
      onFrame(frame) {
        connection.close();
        if (frame.type === "error")
          reject(new Error(frame.message ?? frame.code ?? "Daemon error"));
        else resolve(frame);
      },
      onClose(error) {
        if (error) reject(error);
      },
    });
    try {
      connection.send(request);
    } catch (error) {
      connection.close();
      reject(error);
    }
  });
}

export function createDaemons({
  puHome = process.env.POINT_GUARD_PU_HOME ?? path.join(os.homedir(), ".pu"),
} = {}) {
  const machines = async () => {
    let registry = { machines: [] };
    try {
      registry = JSON.parse(
        await readFile(path.join(puHome, "machines.json"), "utf8"),
      );
    } catch (error) {
      if (error.code !== "ENOENT")
        throw new Error("Cannot read machine registry");
    }
    if (!Array.isArray(registry.machines))
      throw new Error("Invalid machine registry");
    return [
      {
        id: "local",
        name: os.hostname(),
        socket: path.join(puHome, "daemon.sock"),
      },
      ...registry.machines
        .filter(
          (m) =>
            m &&
            typeof m.id === "string" &&
            /^[a-zA-Z0-9_-]+$/.test(m.id) &&
            m.id !== "local",
        )
        .map((m) => ({
          id: m.id,
          name:
            typeof m.name === "string" && m.name.trim() ? m.name.trim() : m.id,
          socket: path.join(puHome, "machines", m.id, "daemon.sock"),
        })),
    ];
  };
  const resolve = async (id) => {
    const machine = (await machines()).find((m) => m.id === id);
    if (!machine) throw new Error("Unknown machine");
    return machine;
  };
  const inspect = async (machine, options = {}) => {
    const health = await daemonRequest(
      machine.socket,
      { type: "health" },
      options,
    );
    if (
      health.type !== "health_report" ||
      !Number.isInteger(health.protocol_version)
    )
      throw new Error("Invalid daemon health");
    try {
      const inventory = await daemonRequest(
        machine.socket,
        { type: "inventory", kind: "agents" },
        options,
      );
      if (
        inventory.type !== "inventory" ||
        !Array.isArray(inventory.agents) ||
        inventory.agents.length > 4096
      )
        throw new Error("Invalid daemon inventory");
      return { version: health.protocol_version, inventory };
    } catch (error) {
      throw Object.assign(new Error("Daemon inventory unavailable"), {
        inventoryUnavailable: true,
        protocolVersion: health.protocol_version,
      });
    }
  };
  return { machines, resolve, inspect };
}
