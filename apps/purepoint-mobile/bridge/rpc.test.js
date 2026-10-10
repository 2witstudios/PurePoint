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

test("response callbacks freeze correlation before later events in the same stdout chunk", async () => {
  const rpc = new Rpc(
    process.execPath,
    [
      "-e",
      `
    process.stdin.once("data", chunk => {
      const request = JSON.parse(chunk.toString());
      const records = [
        { type: "queue_update", followUp: ["Expanded input"] },
        { type: "response", id: request.id, success: true, data: { disposition: "queued" } },
        { type: "queue_update", followUp: ["Expanded input", "Extension side effect"] }
      ];
      process.stdout.write(records.map(record => JSON.stringify(record)).join("\\n") + "\\n");
    });
  `,
    ],
    process.cwd(),
    process.env,
  );
  const order = [];
  rpc.on("event", (event) => order.push(event.followUp));
  try {
    const result = await rpc.call(
      "prompt",
      { message: "/template" },
      30000,
      (data) => order.push(data.disposition),
    );
    assert.equal(result.disposition, "queued");
    assert.deepEqual(order, [
      ["Expanded input"],
      "queued",
      ["Expanded input", "Extension side effect"],
    ]);
  } finally {
    await rpc.close();
  }
});

test("uncertain prompt timeout fences later prompts until bridge restart", async () => {
  const rpc = new Rpc(
    process.execPath,
    [
      "-e",
      `
    process.stdin.on("data", () => {});
  `,
    ],
    process.cwd(),
    process.env,
  );
  let failure;
  rpc.on("failure", (error) => {
    failure = error;
  });
  try {
    await assert.rejects(
      rpc.call("prompt", { message: "Delayed input hook" }, 20),
      /restart the bridge/,
    );
    assert.match(failure.message, /Delivery is uncertain/);
    await assert.rejects(
      rpc.call("prompt", { message: "Another client's input" }),
      /restart the bridge/,
    );
    assert.equal(rpc.pending.size, 0);
  } finally {
    await rpc.close();
  }
});

test("shutdown fence rejects pending and future writes before transport close", async () => {
  const rpc = new Rpc(
    process.execPath,
    ["-e", "process.stdin.resume()"],
    process.cwd(),
    process.env,
  );
  try {
    const pending = rpc.call("get_state").then(
      () => {
        throw new Error("Unexpected reply");
      },
      (error) => error,
    );
    rpc.fence();
    assert.match((await pending).message, /shutting down/);
    await assert.rejects(
      rpc.call("prompt", { message: "must not dispatch" }),
      /shutting down/,
    );
    assert.throws(
      () => rpc.answer({ id: "late", cancelled: true }),
      /shutting down/,
    );
    assert.equal(rpc.pending.size, 0);
  } finally {
    await rpc.close();
  }
});
