import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { AgentSession } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js";
import { installQueueCorrelation } from "./native-queue.js";
import { Controller } from "./controller.js";

installQueueCorrelation(AgentSession);

// Exercise the actual pinned prompt/expansion/enqueue/clear methods without
// provider calls, owner settings, native session files or credentials.
for (const mode of ["steer", "after"]) {
  test(`${mode} pinned native queue correlation survives interleaved extension input and clear`, async () => {
    const session = Object.create(AgentSession.prototype);
    const events = new EventEmitter();
    session._eventListeners = [(event) => events.emit("event", event)];
    session._isAgentRunActive = true;
    session._steeringMessages = [];
    session._followUpMessages = [];
    session._extensionRunner = { getCommand: () => null };
    session.agent = {
      state: { isStreaming: true },
      steer() {},
      followUp() {},
      clearAllQueues() {},
    };
    session._runInputHandlers = async (text) => ({
      text: `Transformed ${text}`,
    });
    session._expandSkillCommand = (text) => `Expanded ${text}`;
    Object.defineProperty(session, "promptTemplates", { value: [] });
    const state = {
      sessionId: "native",
      model: { id: "test" },
      isStreaming: true,
    };
    const behavior = mode === "steer" ? "steer" : "followUp";
    events.call = async (op, args = {}, _timeout, acknowledge) => {
      if (op === "get_state") return state;
      if (op === "get_entries") return { entries: [] };
      if (op === "prompt") {
        let result;
        // Both inputs reach native enqueue before the RPC input's response.
        await Promise.all([
          session.prompt(args.message, {
            source: "rpc",
            streamingBehavior: behavior,
            preflightResult: (disposition) => {
              result = { disposition };
              acknowledge?.(result);
            },
          }),
          session.prompt("extension side effect", {
            source: "extension",
            streamingBehavior: behavior,
          }),
        ]);
        return result;
      }
      if (op === "clear_queue") {
        const result = session.clearQueue();
        // Native RPC dispatch awaits its handler; extensions can enqueue here.
        await session.prompt("after clear", {
          source: "extension",
          streamingBehavior: behavior,
        });
        acknowledge?.(result);
        return result;
      }
      return {};
    };
    events.answer = () => {};
    const c = new Controller(events, {});
    const request = (op, extra) =>
      c.request({
        version: 1,
        clientId: "phone",
        id: crypto.randomUUID(),
        epoch: c.epoch,
        op,
        ...extra,
      });
    try {
      await c.refresh();
      events.emit("event", { type: "agent_start" });
      await request("send", { mode, text: "client input" });
      assert.equal(c.queue[0].clientId, "phone");
      assert.equal(c.queue[0].text, "Expanded Transformed client input");
      assert.equal(c.queue[1].clientId, null);
      await request("stop", { runId: c.runId });
      assert.equal(c.canceled[0].clientId, "phone");
      assert.equal(c.canceled[0].text, "Expanded Transformed client input");
      assert.equal(c.canceled[1].clientId, null);
      assert.equal(c.queue[0].text, "Expanded Transformed after clear");
      assert.equal(c.queue[0].clientId, null);
    } finally {
      c.dispose();
    }
  });
}
