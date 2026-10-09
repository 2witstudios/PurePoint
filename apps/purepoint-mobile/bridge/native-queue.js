import { AsyncLocalStorage } from "node:async_hooks";

// Adapter for pinned Pi 1.1.0. Keep native expansion and queue behavior intact,
// adding correlation only where native methods synchronously mutate the queue.
export function installQueueCorrelation(AgentSession) {
  const input = new AsyncLocalStorage();
  const boundaries = new WeakMap();
  const prompt = AgentSession.prototype.prompt;
  AgentSession.prototype.prompt = function (text, options) {
    return input.run(options?.source ?? "interactive", () =>
      prompt.call(this, text, options),
    );
  };
  const queueInput = AgentSession.prototype._queueUserInput;
  AgentSession.prototype._queueUserInput = function (
    text,
    images,
    behavior,
    source,
  ) {
    return input.run(source, () =>
      queueInput.call(this, text, images, behavior, source),
    );
  };
  const emit = AgentSession.prototype._emit;
  AgentSession.prototype._emit = function (event) {
    const boundary = boundaries.get(this);
    return emit.call(
      this,
      event.type === "queue_update" && boundary
        ? { ...event, ...boundary }
        : event,
    );
  };
  /** @type {Array<[string, object]>} */
  const methods = [
    ["_queueSteer", { enqueuedMode: "steer" }],
    ["_queueFollowUp", { enqueuedMode: "after" }],
    ["clearQueue", { queueCleared: true }],
  ];
  for (const [method, boundary] of methods) {
    const original = AgentSession.prototype[method];
    AgentSession.prototype[method] = function (...args) {
      const previous = boundaries.get(this);
      boundaries.set(this, { ...boundary, inputSource: input.getStore() });
      try {
        return original.apply(this, args);
      } finally {
        if (previous) boundaries.set(this, previous);
        else boundaries.delete(this);
      }
    };
  }
}
