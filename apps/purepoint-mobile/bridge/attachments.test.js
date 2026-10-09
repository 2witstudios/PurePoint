import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { Controller } from "./controller.js";
const image = {
  type: "image",
  mimeType: "image/png",
  data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aTz0AAAAASUVORK5CYII=",
};
test("given images should forward validated native content once and reject invalid or queued uploads", async () => {
  class Runtime extends EventEmitter {
    calls = [];
    async call(op, data) {
      this.calls.push({ op, data });
      if (op === "get_state")
        return {
          sessionId: "one",
          model: { id: "vision", input: ["text", "image"] },
          isStreaming: false,
        };
      if (op === "get_entries") return { entries: [], leafId: null };
      return { disposition: "started" };
    }
  }
  const rpc = new Runtime();
  const c = new Controller(rpc, {});
  const send = (images, mode = "send") =>
    c.request({
      version: 1,
      clientId: "test-client",
      id: crypto.randomUUID(),
      op: "send",
      epoch: c.epoch,
      text: "Describe this",
      mode,
      images,
    });
  try {
    await c.refresh();
    await send([image]);
    assert.deepEqual(rpc.calls.find((x) => x.op === "prompt").data.images, [
      image,
    ]);
    assert.ok(c.snapshot().capabilities.includes("images"));
    await assert.rejects(send([{ ...image, data: "not-base64" }]));
    await assert.rejects(send([{ ...image, mimeType: "image/jpeg" }]));
    await assert.rejects(send([{ ...image, mimeType: "__proto__" }]));
    await assert.rejects(
      send([
        { ...image, data: Buffer.alloc(512 * 1024 + 1).toString("base64") },
      ]),
    );
    await assert.rejects(send(Array(5).fill(image)));
    c.busy = true;
    await assert.rejects(send([image], "after"), /idle/i);
    c.busy = false;
    c.state.model.input = ["text"];
    await assert.rejects(send([image]), /image/i);
    assert.equal(rpc.calls.filter((x) => x.op === "prompt").length, 1);
  } finally {
    c.dispose();
  }
});
