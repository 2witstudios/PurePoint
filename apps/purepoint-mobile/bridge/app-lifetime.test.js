import test from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { watchAppLifetime } from "./app-lifetime.js";
import { openRuntimeState } from "./runtime-state.js";
import { openTrustStore } from "./trust.js";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function waitFor(action) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const result = await action();
    if (result) return result;
    await pause(20);
  }
  throw new Error("App lifetime fixture timed out");
}

test("terminal launch leaves stdin and its EOF behavior untouched", () => {
  const input = new PassThrough();
  let lost = false;
  const stop = watchAppLifetime(
    {},
    () => {
      lost = true;
    },
    input,
  );
  assert.equal(input.readableFlowing, null);
  input.emit("end");
  stop();
  assert.equal(lost, false);
});

test("app channel EOF or error signals loss once; disposing suppresses shutdown", async () => {
  for (const event of ["end", "close", "error"]) {
    const input = new PassThrough();
    let losses = 0;
    watchAppLifetime(
      { POINT_GUARD_APP_LIFETIME: "stdin" },
      () => losses++,
      input,
    );
    input.emit(event, new Error("fixture"));
    input.emit("close");
    assert.equal(losses, 1);
    const other = new PassThrough();
    const stop = watchAppLifetime(
      { POINT_GUARD_APP_LIFETIME: "stdin" },
      () => losses++,
      other,
    );
    stop();
    other.emit("end");
    assert.equal(losses, 1);
  }
  const ended = new PassThrough();
  ended.destroy();
  let losses = 0;
  watchAppLifetime(
    { POINT_GUARD_APP_LIFETIME: "stdin" },
    () => losses++,
    ended,
  );
  await pause(0);
  assert.equal(losses, 1, "Owner loss before watcher attachment is observed");
});

test("app owner crash releases live runtime and trust locks while preserving identity", async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), "pointguard-app-crash-"));
  const stateDir = path.join(home, "state");
  let owner, runtimePid, replacement, trust;
  let runtimeExited = false;
  try {
    const runtimeURL = new URL("./runtime.js", import.meta.url).href;
    // This disposable owner supplies the same pipe contract as the Mac app.
    // Killing the owner skips all orderly service shutdown code.
    owner = spawn(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
      import {spawn} from "node:child_process";
      const child = spawn(process.execPath, ["--input-type=module", "-e", ${JSON.stringify(`import {runManaged} from ${JSON.stringify(runtimeURL)}; await runManaged();`)}], {
        env: {...process.env, POINT_GUARD_APP_LIFETIME:"stdin"},
        stdio:["pipe","ignore","ignore"]
      });
      process.send({pid:child.pid});
      process.on("message", () => {});
    `,
      ],
      {
        env: {
          ...process.env,
          HOME: home,
          POINT_GUARD_STATE_DIR: stateDir,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          PI_SKIP_VERSION_CHECK: "1",
        },
        stdio: ["ignore", "ignore", "ignore", "ipc"],
      },
    );
    runtimePid = (await once(owner, "message"))[0].pid;
    const descriptorFile = path.join(stateDir, "admin.json");
    const ready = await waitFor(async () => {
      try {
        return JSON.parse(await readFile(descriptorFile, "utf8"));
      } catch (error) {
        if (error.code === "ENOENT") return false;
        throw error;
      }
    });
    assert.equal(ready.pid, runtimePid);
    await assert.rejects(openRuntimeState(stateDir), { code: "lock_busy" });
    const durable = await readFile(path.join(stateDir, "runtime.json"));
    const identity = await readFile(path.join(stateDir, "trust/trust.json"));
    const certificate = await readFile(
      path.join(stateDir, "trust/identity-cert.pem"),
    );
    const exited = once(owner, "exit");
    owner.kill("SIGKILL");
    await exited;
    await waitFor(async () => {
      // A zombie under a container's PID 1 has exited and released all descriptors.
      try {
        return /^Z/.test(
          execFileSync("/bin/ps", ["-p", String(runtimePid), "-o", "stat="], {
            encoding: "utf8",
          }).trim(),
        );
      } catch {
        return true;
      }
    });
    runtimeExited = true;
    await assert.rejects(readFile(descriptorFile), { code: "ENOENT" });
    replacement = await openRuntimeState(stateDir);
    assert.equal(replacement.value.desktopClientId, ready.desktopClientId);
    trust = await openTrustStore({ directory: path.join(stateDir, "trust") });
    assert.equal(trust.hostId, ready.hostId);
    assert.deepEqual(
      await readFile(path.join(stateDir, "trust/trust.json")),
      identity,
    );
    assert.deepEqual(
      await readFile(path.join(stateDir, "trust/identity-cert.pem")),
      certificate,
    );
    assert.deepEqual(
      await readFile(path.join(stateDir, "runtime.json")),
      durable,
    );
  } finally {
    if (owner?.exitCode === null && owner?.signalCode === null) {
      const exited = once(owner, "exit");
      owner.kill("SIGKILL");
      await exited;
    }
    if (runtimePid && !runtimeExited) {
      try {
        process.kill(runtimePid, "SIGTERM");
      } catch {}
    }
    await trust?.close();
    await replacement?.close();
    await rm(home, { recursive: true, force: true });
  }
});

test("a surviving owner remains protected and app collision wait ends after five seconds", async () => {
  const home = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-app-collision-"),
  );
  const stateDir = path.join(home, "state");
  let original, child;
  try {
    original = await openRuntimeState(stateDir);
    const durable = await readFile(path.join(stateDir, "runtime.json"));
    child = spawn(
      process.execPath,
      [new URL("./main.js", import.meta.url).pathname],
      {
        env: {
          ...process.env,
          HOME: home,
          POINT_GUARD_STATE_DIR: stateDir,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          POINT_GUARD_APP_LIFETIME: "stdin",
          PI_SKIP_VERSION_CHECK: "1",
        },
        stdio: ["pipe", "ignore", "ignore"],
      },
    );
    const exited = once(child, "exit");
    assert.equal((await exited)[0], 1);
    original.assertHeld();
    const error = JSON.parse(
      await readFile(path.join(stateDir, "error.json"), "utf8"),
    );
    assert.equal(error.code, "startup_collision");
    assert.deepEqual(
      await readFile(path.join(stateDir, "runtime.json")),
      durable,
    );
    await assert.rejects(openRuntimeState(stateDir), { code: "lock_busy" });
  } finally {
    if (child?.exitCode === null && child?.signalCode === null) {
      const exited = once(child, "exit");
      child.stdin.end();
      await exited;
    }
    await original?.close();
    await rm(home, { recursive: true, force: true });
  }
});

test("owner EOF during managed startup leaves locks available for a new launch", async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), "pointguard-app-startup-"));
  const stateDir = path.join(home, "state");
  let child, replacement;
  try {
    child = spawn(
      process.execPath,
      [new URL("./main.js", import.meta.url).pathname],
      {
        env: {
          ...process.env,
          HOME: home,
          POINT_GUARD_STATE_DIR: stateDir,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          POINT_GUARD_APP_LIFETIME: "stdin",
          PI_SKIP_VERSION_CHECK: "1",
        },
        stdio: ["pipe", "ignore", "ignore"],
      },
    );
    const exited = once(child, "exit");
    child.stdin.end();
    await exited;
    await assert.rejects(readFile(path.join(stateDir, "admin.json")), {
      code: "ENOENT",
    });
    replacement = await openRuntimeState(stateDir);
  } finally {
    if (child?.exitCode === null && child?.signalCode === null) {
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      await exited;
    }
    await replacement?.close();
    await rm(home, { recursive: true, force: true });
  }
});
