// Deterministic RPC child. No credentials, native session writes, tools or provider calls.
import { LineDecoder } from "./core.js";
import { randomUUID } from "node:crypto";
let sessionId = "fixture-session",
  entries = [],
  leafId = null,
  busy = false,
  timer = null,
  queue = [],
  waiting = null,
  active = null;
let stamp = 1000;
const out = (x) => process.stdout.write(JSON.stringify(x) + "\n");
const reply = (r, data = {}) =>
  out({ type: "response", id: r.id, command: r.type, success: true, data });
const queueUpdate = () =>
  out({
    type: "queue_update",
    steering: queue.filter((x) => x.mode === "steer").map((x) => x.text),
    followUp: queue.filter((x) => x.mode === "after").map((x) => x.text),
  });
function message(role, content) {
  const m = { role, content, timestamp: ++stamp };
  out({ type: "message_start", message: m });
  return m;
}
function finish(m) {
  entries.push({
    type: "message",
    id: randomUUID(),
    parentId: leafId,
    message: m,
  });
  leafId = entries.at(-1).id;
  out({ type: "message_end", message: m });
  active = null;
}
function settled(aborted = false) {
  if (queue.length && !aborted) {
    const next = queue.shift();
    queueUpdate();
    run(next.text);
  } else {
    busy = false;
    out({ type: "agent_settled", aborted });
  }
}
function run(text, images = []) {
  busy = true;
  out({ type: "agent_start" });
  finish(
    message("user", images.length ? [{ type: "text", text }, ...images] : text),
  );
  const methods = {
    "/fixture-dialog": "editor",
    "/fixture-confirm": "confirm",
    "/fixture-select": "select",
    "/fixture-input": "input",
  };
  if (methods[text]) {
    waiting = "fixture-dialog-" + stamp;
    out({
      type: "extension_ui_request",
      id: waiting,
      method: methods[text],
      title: "Pi needs your input",
      message: "A deterministic extension interaction.",
      options: ["Continue", "Pause"],
      prefill: "A draft from the extension",
      placeholder: "Your answer",
    });
    return;
  }
  if (text === "/fixture-draft")
    out({
      type: "extension_ui_request",
      id: "fixture-offer-" + stamp,
      method: "set_editor_text",
      text: "Suggested by an extension. Your existing draft is preserved.",
    });
  const m = message("assistant", []);
  active = m;
  out({
    type: "tool_execution_start",
    toolCallId: "fixture-tool-" + stamp,
    toolName: "read",
    args: { path: "README.md" },
  });
  const toolId = "fixture-tool-" + stamp;
  out({
    type: "extension_ui_request",
    id: "fixture-status-" + stamp,
    method: "setStatus",
    statusKey: "fixture",
    statusText: "Reading project notes",
  });
  timer = setTimeout(
    () => {
      out({
        type: "tool_execution_end",
        toolCallId: toolId,
        toolName: "read",
        result: { content: [{ type: "text", text: "Fixture file read." }] },
        isError: false,
      });
      out({
        type: "message_update",
        assistantMessageEvent: {
          type: "text_delta",
          contentIndex: 0,
          delta: "Here is a calm, native Pi conversation.\n\n",
        },
      });
      m.content = [
        { type: "text", text: "Here is a calm, native Pi conversation.\n\n" },
      ];
      timer = setTimeout(
        () => {
          m.content = [
            {
              type: "text",
              text: 'Here is a calm, native Pi conversation.\n\n**Ready to work.** You can keep typing while Pi replies.\n\n```swift\nlet idea = "Something worth building"\n```',
            },
          ];
          if (text === "/fixture-error")
            m.errorMessage =
              "Fixture provider unavailable. Check local Pi provider setup.";
          finish(m);
          out({
            type: "extension_ui_request",
            id: "fixture-status-end-" + stamp,
            method: "setStatus",
            statusKey: "fixture",
          });
          settled();
        },
        text === "/fixture-slow" ? 3000 : 80,
      );
    },
    text === "/fixture-slow" ? 3000 : 80,
  );
}
const decoder = new LineDecoder((r) => {
  switch (r.type) {
    case "get_state":
      reply(r, {
        sessionId,
        sessionName: "A little room to think",
        isStreaming: busy,
        isCompacting: false,
        pendingMessageCount: queue.length,
        model: { id: "fixture", provider: "fixture" },
      });
      break;
    case "get_entries":
      reply(r, { entries, leafId });
      break;
    case "get_commands":
      reply(r, {
        commands: [
          { name: "skill:pu", source: "skill", path: "/fixture/pu/SKILL.md" },
        ],
      });
      break;
    case "prompt":
      if (busy) {
        queue.push({
          text: r.message,
          mode: r.streamingBehavior === "steer" ? "steer" : "after",
        });
        queueUpdate();
        reply(r, { disposition: "queued" });
      } else {
        run(r.message, r.images);
        reply(r, { disposition: "started" });
      }
      break;
    case "clear_queue": {
      const data = {
        steering: queue.filter((x) => x.mode === "steer").map((x) => x.text),
        followUp: queue.filter((x) => x.mode === "after").map((x) => x.text),
      };
      queue = [];
      queueUpdate();
      reply(r, data);
      break;
    }
    case "abort":
      clearTimeout(timer);
      waiting = null;
      if (active) {
        active.errorMessage = "Stopped before the response completed.";
        finish(active);
      }
      settled(true);
      reply(r);
      break;
    case "new_session":
    case "switch_session":
      sessionId = r.sessionPath ?? randomUUID();
      entries = [];
      leafId = null;
      if (sessionId === "fixture-history") {
        for (const [role, content] of [
          ["user", "Where should we begin?"],
          ["assistant", "With one clear idea."],
        ]) {
          const id = randomUUID();
          entries.push({
            type: "message",
            id,
            parentId: leafId,
            message: { role, content, timestamp: ++stamp },
          });
          leafId = id;
        }
      }
      reply(r, { cancelled: false });
      break;
    case "extension_ui_response":
      if (r.id === waiting) {
        waiting = null;
        finish(
          message("assistant", [
            {
              type: "text",
              text: r.cancelled
                ? "Extension interaction canceled."
                : `Extension received: ${r.value ?? r.confirmed}`,
            },
          ]),
        );
        settled();
      }
      break;
    default:
      out({
        type: "response",
        id: r.id,
        command: r.type,
        success: false,
        error: "Unsupported command",
      });
  }
});
process.stdin.on("data", (c) => decoder.push(c));
process.stdin.on("end", () => {
  clearTimeout(timer);
  process.exit(0);
});
