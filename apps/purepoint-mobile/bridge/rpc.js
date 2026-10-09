import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { randomUUID } from "node:crypto";
import { LineDecoder, safeEnvironment, clip } from "./core.js";
export class Rpc extends EventEmitter {
  constructor(executable, args, cwd, env) {
    super();
    this.pending = new Map();
    this.failure = null;
    this.closing = false;
    this.child = spawn(executable, args, {
      cwd,
      env: safeEnvironment(env),
      stdio: ["pipe", "pipe", "pipe"],
    });
    const decoder = new LineDecoder((record) => {
      if (record.type === "response") {
        const p = this.pending.get(record.id);
        if (p) {
          clearTimeout(p.timer);
          this.pending.delete(record.id);
          if (record.success) p.onResponse?.(record.data ?? {});
          record.success
            ? p.resolve(record.data ?? {})
            : p.reject(
                new Error(clip(record.error ?? "Pi rejected command", 4000)),
              );
        }
      } else this.emit("event", record);
    });
    this.child.stdout.on("data", (chunk) => {
      try {
        decoder.push(chunk);
      } catch {
        this.fail(
          new Error(
            "Invalid or oversized Pi RPC output. Check the pinned runtime and extensions.",
          ),
        );
        this.child.stdin.end();
      }
    });
    // Consume stderr independently. Never forward it: extensions may accidentally log credentials.
    this.diagnosticBytes = 0;
    this.child.stderr.on("data", (chunk) => {
      this.diagnosticBytes = Math.min(
        this.diagnosticBytes + chunk.length,
        1024 * 1024,
      );
    });
    this.child.on("error", () =>
      this.fail(
        new Error(
          "Pi could not start. Run npm ci and check the configured working folder.",
        ),
      ),
    );
    this.child.stdin.on("error", () =>
      this.fail(
        new Error(
          "Pi input pipe failed. Delivery is uncertain; reconnect and inspect history.",
        ),
      ),
    );
    this.child.on("exit", (code, signal) => {
      if (!this.closing) {
        try {
          decoder.end();
        } catch {}
        this.fail(
          new Error(
            `Pi exited (${code ?? signal}). Check native Pi provider setup in a terminal, then restart the bridge.`,
          ),
        );
      }
    });
  }
  fail(error) {
    if (this.failure) return;
    this.failure = error;
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(error);
    }
    this.pending.clear();
    this.emit("failure", error);
  }
  write(record) {
    if (this.failure) throw this.failure;
    if (this.child.stdin.writableLength > 1024 * 1024)
      throw new Error("Pi input is backlogged; inspect state before retrying.");
    this.child.stdin.write(JSON.stringify(record) + "\n");
  }
  /** @returns {Promise<any>} */
  call(type, data = {}, timeout = 30000, onResponse = undefined) {
    if (this.pending.size >= 32)
      return Promise.reject(new Error("Too many Pi commands in flight"));
    const id = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const error = new Error(
          type === "prompt"
            ? "Pi prompt timed out. Delivery is uncertain; inspect native history and restart the bridge before sending again."
            : "Pi command timed out. Delivery is uncertain; inspect history before sending again.",
        );
        // A timed-out input hook can still enqueue later. Fence subsequent RPC
        // prompts so that it cannot inherit another client's queue ownership.
        if (type === "prompt") this.fail(error);
        else {
          this.pending.delete(id);
          reject(error);
        }
      }, timeout);
      this.pending.set(id, { resolve, reject, timer, onResponse });
      try {
        this.write({ ...data, type, id });
      } catch (e) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(e);
      }
    });
  }
  answer(data) {
    this.write({ ...data, type: "extension_ui_response" });
  }
  async close() {
    this.closing = true;
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new Error("Bridge shutting down"));
    }
    this.pending.clear();
    if (this.child.exitCode !== null) return;
    await new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.child.kill("SIGTERM");
        resolve(null);
      }, 2000);
      this.child.once("exit", () => {
        clearTimeout(timer);
        resolve(null);
      });
      this.child.stdin.end();
    });
  }
}
