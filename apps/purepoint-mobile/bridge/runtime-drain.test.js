import test from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { WebSocket } from "ws";
import path from "node:path";
import os from "node:os";
import { randomUUID } from "node:crypto";
import { openRuntimeState } from "./runtime-state.js";
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("helper loss retains runtime ownership until a dispatched controller action drains", async () => {
  const home = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-controller-drain-"),
  );
  const stateDir = path.join(home, "state");
  let child, socket;
  const messages = [];
  const waitMessage = async (type) => {
    const deadline = Date.now() + 30000;
    while (!messages.some((m) => m.type === type)) {
      if (child.exitCode !== null || child.signalCode !== null)
        throw new Error("Owned fixture exited before " + type);
      if (Date.now() > deadline)
        throw new Error("Owned fixture did not reach " + type);
      await pause(10);
    }
    return messages.find((m) => m.type === type);
  };
  try {
    const runtime = new URL("./runtime.js", import.meta.url).href;
    const controller = new URL("./controller.js", import.meta.url).href;
    const rpc = new URL("./rpc.js", import.meta.url).href;
    // Delay only this disposable child's already-dispatched action. All service,
    // RPC, helper, listener and state cleanup code remains the actual source.
    child = spawn(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
      import {Controller} from ${JSON.stringify(controller)};
      import {Rpc} from ${JSON.stringify(rpc)};
      import {runManaged} from ${JSON.stringify(runtime)};
      let release;
      const gate = new Promise(resolve => { release = resolve; });
      process.on("message", () => release());
      const mutate = Controller.prototype.mutate;
      Controller.prototype.mutate = async function(request) {
        process.send({type:"dispatched"});
        await gate;
        return mutate.call(this,request);
      };
      const close = Rpc.prototype.close;
      Rpc.prototype.close = async function() {
        const child = this.child;
        const exited = child.exitCode !== null || child.signalCode !== null
          ? Promise.resolve() : new Promise(resolve => child.once("exit",resolve));
        await close.call(this);
        await exited;
        process.send({type:"rpcClosed"});
      };
      await runManaged();
      process.send({type:"ready"});
    `,
      ],
      {
        env: {
          ...process.env,
          HOME: home,
          POINT_GUARD_STATE_DIR: stateDir,
          POINT_GUARD_PU_PATH: "/usr/bin/true",
          POINT_GUARD_INSTANCE_ID: randomUUID(),
          PI_SKIP_VERSION_CHECK: "1",
        },
        stdio: ["ignore", "ignore", "ignore", "ipc"],
      },
    );
    child.on("message", (m) => messages.push(m));
    await waitMessage("ready");
    const ready = JSON.parse(
      await readFile(path.join(stateDir, "admin.json"), "utf8"),
    );
    const token = (
      await readFile(path.join(stateDir, "desktop-chat-token"), "utf8")
    ).trim();
    socket = new WebSocket(ready.nativeChatURL, {
      headers: {
        authorization: `Bearer ${token}`,
        "x-pointguard-client-id": ready.desktopClientId,
      },
    });
    await once(socket, "open");
    const receipt = new Promise((resolve, reject) => {
      socket.on("message", (bytes) => {
        const value = JSON.parse(bytes.toString());
        if (value.type === "receipt" && value.id === "sync") {
          value.ok ? resolve(value.data) : reject(new Error(value.error));
        }
      });
    });
    socket.send(
      JSON.stringify({
        version: 1,
        clientId: ready.desktopClientId,
        id: "sync",
        op: "sync",
      }),
    );
    const snapshot = await receipt;
    socket.send(
      JSON.stringify({
        version: 1,
        clientId: ready.desktopClientId,
        id: "delayed",
        epoch: snapshot.epoch,
        op: "new",
      }),
    );
    await waitMessage("dispatched");
    const helper = execFileSync("/bin/ps", ["-axo", "pid=,ppid=,args="], {
      encoding: "utf8",
    })
      .split("\n")
      .map((line) => line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/))
      .find(
        (row) =>
          row &&
          Number(row[2]) === child.pid &&
          row[3].startsWith(
            process.env.POINT_GUARD_LOCK_HELPER_PATH + " --file ",
          ),
      );
    assert.ok(helper, "Direct owned helper must be present");
    const closed = once(socket, "close");
    process.kill(Number(helper[1]), "SIGKILL"); // exact direct child of disposable fixture
    await closed;
    await waitMessage("rpcClosed");
    // The Pi child is gone but the accepted action is still gated. No second
    // writer may acquire until controller cleanup (including queued work) drains.
    for (let i = 0; i < 10; i++) {
      assert.equal(child.exitCode, null, "Service must await dispatched work");
      let competing;
      try {
        competing = await openRuntimeState(stateDir);
      } catch (error) {
        assert.equal(error.code, "lock_busy");
      } finally {
        await competing?.close();
      }
      assert.equal(
        competing,
        undefined,
        "A second writer must remain excluded until controller drain",
      );
      await pause(25);
    }
    const exited = once(child, "exit");
    child.send({ type: "release" });
    assert.equal((await exited)[0], 1);
    const restored = await openRuntimeState(stateDir);
    assert.equal(restored.value.desktopClientId, ready.desktopClientId);
    await restored.close();
  } finally {
    socket?.terminate();
    if (child && child.exitCode === null && child.signalCode === null) {
      child.send({ type: "release" });
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      await exited;
    }
    await rm(home, { recursive: true, force: true });
  }
});
