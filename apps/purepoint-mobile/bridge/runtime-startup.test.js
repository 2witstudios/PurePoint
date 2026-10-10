import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  readFile,
  access,
  rm,
  writeFile,
} from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import os from "node:os";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
test("given cancel during fresh owned startup should release lock and attribute sanitized failure", async () => {
  const folder = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-start-cancel-"),
  );
  let child;
  try {
    const home = path.join(folder, "home");
    await mkdir(home, { mode: 0o700 });
    const state = path.join(home, "state");
    const instanceId = randomUUID();
    child = spawn(
      process.execPath,
      [fileURLToPath(new URL("./main.js", import.meta.url)), "--managed"],
      {
        env: {
          HOME: home,
          PATH: "/no-external-tools",
          POINT_GUARD_STATE_DIR: state,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          POINT_GUARD_INSTANCE_ID: instanceId,
          PI_SKIP_VERSION_CHECK: "1",
        },
        stdio: "ignore",
      },
    );
    for (let i = 0; i < 200; i++) {
      try {
        await access(path.join(state, "admin-token"));
        break;
      } catch {}
      if (child.exitCode !== null)
        throw new Error("Startup exited before proof signal.");
      await pause(5);
    }
    await access(path.join(state, "admin-token"));
    const exited = once(child, "exit");
    child.kill("SIGTERM");
    const [, signal] = await exited;
    assert.equal(signal, null);
    await assert.rejects(access(path.join(state, "runtime.lock")));
    const error = JSON.parse(
      await readFile(path.join(state, "error.json"), "utf8"),
    );
    assert.equal(error.instanceId, instanceId);
    assert.equal(error.pid, child.pid);
    assert.equal(error.code, "startup_canceled");
    const retry = spawn(
      process.execPath,
      [fileURLToPath(new URL("./main.js", import.meta.url)), "--managed"],
      {
        env: {
          HOME: home,
          PATH: "/no-external-tools",
          POINT_GUARD_STATE_DIR: state,
          POINT_GUARD_PU_PATH: "/nonexistent-proof-cli",
          POINT_GUARD_INSTANCE_ID: randomUUID(),
        },
        stdio: "ignore",
      },
    );
    assert.equal((await once(retry, "exit"))[0], 1);
    const failure = JSON.parse(
      await readFile(path.join(state, "error.json"), "utf8"),
    );
    assert.equal(failure.code, "missing_runtime");
    await assert.rejects(access(path.join(state, "runtime.lock")));
  } finally {
    if (child && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      await exited;
    }
    await rm(folder, { recursive: true, force: true });
  }
});

test("given nonprivate owner capability should identify credential recovery rather than cwd", async () => {
  const folder = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-capability-"),
  );
  try {
    const state = path.join(folder, "state");
    await mkdir(state, { mode: 0o700 });
    const tokenFile = path.join(state, "admin-token");
    await writeFile(tokenFile, "private-proof-credential", { mode: 0o644 });
    const instanceId = randomUUID();
    const child = spawn(
      process.execPath,
      [fileURLToPath(new URL("./main.js", import.meta.url)), "--managed"],
      {
        env: {
          HOME: folder,
          PATH: "/no-external-tools",
          POINT_GUARD_STATE_DIR: state,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          POINT_GUARD_INSTANCE_ID: instanceId,
        },
        stdio: "ignore",
      },
    );
    assert.equal((await once(child, "exit"))[0], 1);
    const failure = JSON.parse(
      await readFile(path.join(state, "error.json"), "utf8"),
    );
    assert.equal(failure.code, "local_capability_state");
    assert.equal(failure.instanceId, instanceId);
    assert.equal(failure.pid, child.pid);
    assert.equal(
      JSON.stringify(failure).includes("private-proof-credential"),
      false,
    );
    assert.equal(await readFile(tokenFile, "utf8"), "private-proof-credential");
    await assert.rejects(access(path.join(state, "runtime.lock")));
  } finally {
    await rm(folder, { recursive: true, force: true });
  }
});
