import test from "node:test";
import assert from "node:assert/strict";
import { Rpc } from "./rpc.js";
import { fileURLToPath } from "node:url";
test("real child RPC boundary correlates requests and survives observer detachment", async () => {
  const rpc = new Rpc(
    process.execPath,
    [fileURLToPath(new URL("./fixture.js", import.meta.url))],
    process.cwd(),
    process.env,
  );
  try {
    const events = [];
    rpc.on("event", (e) => events.push(e));
    const [state, entries] = await Promise.all([
      rpc.call("get_state"),
      rpc.call("get_entries"),
    ]);
    assert.equal(state.isStreaming, false);
    assert.deepEqual(entries.entries, []);
    const accepted = await rpc.call("prompt", { message: "Hello" });
    assert.equal(accepted.disposition, "started");
    rpc.removeAllListeners("event");
    await new Promise((r) => setTimeout(r, 250));
    const history = await rpc.call("get_entries");
    assert.equal(history.entries.length, 2);
    assert.equal((await rpc.call("get_state")).isStreaming, false);
    await assert.rejects(rpc.call("unsupported"), /Unsupported/);
  } finally {
    await rpc.close();
  }
});
test("child exit rejects pending operations visibly", async () => {
  const rpc = new Rpc(
    process.execPath,
    ["-e", 'process.stdin.once("data",()=>process.exit(7))'],
    process.cwd(),
    process.env,
  );
  rpc.on("failure", () => {});
  await assert.rejects(rpc.call("get_state"), /exited/);
  await rpc.close();
});
