import test from "node:test";
import assert from "node:assert/strict";
import {
  LineDecoder,
  Projection,
  activeBranch,
  safeEnvironment,
} from "./core.js";
const verify = ({ given, should, actual, expected }) =>
  assert.deepEqual(actual, expected, `${given}: ${should}`);
test("fragmented UTF8 and Unicode separators", () => {
  const records = [];
  const decoder = new LineDecoder((x) => records.push(x), 1000);
  const bytes = Buffer.from(
    JSON.stringify({ text: "π\u2028yes" }) +
      "\r\n" +
      JSON.stringify({ ok: true }) +
      "\n",
  );
  for (const byte of bytes) decoder.push(Buffer.from([byte]));
  verify({
    given: "fragmented JSONL with Unicode text",
    should: "decode LF records intact",
    actual: records,
    expected: [{ text: "π\u2028yes" }, { ok: true }],
  });
  assert.throws(
    () => new LineDecoder(() => {}, 3).push(Buffer.from("xxxx")),
    /limit/,
  );
});
test("branch history excludes abandoned work and retains compaction markers", () => {
  const entries = [
    {
      id: "a",
      parentId: null,
      type: "message",
      message: { role: "user", content: "a", timestamp: 1 },
    },
    {
      id: "b",
      parentId: "a",
      type: "message",
      message: { role: "assistant", content: "abandoned", timestamp: 2 },
    },
    { id: "c", parentId: "a", type: "compaction", summary: "summary" },
    {
      id: "d",
      parentId: "c",
      type: "message",
      message: { role: "user", content: "d", timestamp: 3 },
    },
  ];
  verify({
    given: "a branched compacted native session",
    should: "walk only the active path",
    actual: activeBranch(entries, "d").map((x) => x.id),
    expected: ["a", "c", "d"],
  });
  const p = new Projection();
  p.load(entries, "d");
  assert.equal(
    p.messages.some((x) => x.text === "abandoned"),
    false,
  );
  assert.ok(p.messages.some((x) => x.text.includes("summary")));
});
test("stream reconciliation and reconnect", () => {
  const p = new Projection();
  const m = { role: "assistant", timestamp: 5, content: [] };
  p.event({ type: "message_start", message: m });
  p.event({
    type: "message_update",
    assistantMessageEvent: {
      type: "text_delta",
      contentIndex: 0,
      delta: "part",
    },
  });
  assert.equal(p.messages[0].text, "part");
  p.event({
    type: "message_end",
    message: { ...m, content: [{ type: "text", text: "complete" }] },
  });
  p.load(
    [
      {
        id: "native",
        parentId: null,
        type: "message",
        message: { ...m, content: [{ type: "text", text: "complete" }] },
      },
    ],
    "native",
  );
  verify({
    given: "live completion followed by native history refresh",
    should: "replace partial without duplication",
    actual: p.messages.map((x) => x.text),
    expected: ["complete"],
  });
});
test("builder identity is cleared without replacing native provider environment", () => {
  const env = safeEnvironment({
    PU_AGENT_ID: "builder",
    PU_PROJECT_ROOT: "wrong",
    ANTHROPIC_API_KEY: "preserve",
    PI_MOBILE_TOKEN_FILE: "/secret",
  });
  assert.equal(env.PU_AGENT_ID, undefined);
  assert.equal(env.PU_PROJECT_ROOT, undefined);
  assert.equal(env.PI_MOBILE_TOKEN_FILE, undefined);
  assert.equal(env.ANTHROPIC_API_KEY, "preserve");
});
test("native branch changes remove persisted event rows from abandoned paths", () => {
  const p = new Projection();
  const m = { role: "assistant", timestamp: 20, content: "Abandoned" };
  p.event({ type: "message_end", message: m });
  p.load(
    [
      { id: "left", parentId: null, type: "message", message: m },
      {
        id: "right",
        parentId: null,
        type: "message",
        message: { role: "user", timestamp: 21, content: "New branch" },
      },
    ],
    "right",
  );
  assert.equal(
    p.messages.some((x) => x.text === "Abandoned"),
    false,
  );
});
test("display data is bounded while native storage remains authoritative", () => {
  const p = new Projection();
  for (let i = 0; i < 600; i++)
    p.event({
      type: "message_end",
      message: { role: "assistant", timestamp: i, content: "x".repeat(70000) },
    });
  assert.equal(p.messages.length, 500);
  assert.ok(p.messages[0].text.length < 66000);
});

test("branch changes discard persisted abandoned tools while preserving unpersisted activity", () => {
  const p = new Projection();
  for (const id of [
    "shared",
    "abandoned",
    "call-only",
    "live",
    "completed-live",
  ])
    p.event({
      type: id === "live" ? "tool_execution_start" : "tool_execution_end",
      toolCallId: id,
      toolName: "read",
      result: { content: [{ type: "text", text: id }] },
    });
  const entries = [
    {
      id: "root",
      parentId: null,
      type: "message",
      message: {
        role: "toolResult",
        timestamp: 1,
        toolCallId: "shared",
        toolName: "read",
        content: "shared",
      },
    },
    {
      id: "left",
      parentId: "root",
      type: "message",
      message: {
        role: "toolResult",
        timestamp: 2,
        toolCallId: "abandoned",
        toolName: "read",
        content: "abandoned",
      },
    },
    {
      id: "left-call",
      parentId: "left",
      type: "message",
      message: {
        role: "assistant",
        timestamp: 3,
        content: [
          { type: "toolCall", id: "call-only", name: "read", arguments: {} },
        ],
      },
    },
    {
      id: "right",
      parentId: "root",
      type: "message",
      message: { role: "user", timestamp: 4, content: "Selected branch" },
    },
  ];
  p.load(entries, "left-call");
  assert.equal(p.tools.length, 5);
  p.load(entries, "right");
  assert.equal(p.tools.find((tool) => tool.id === "live").state, "running");
  assert.deepEqual(
    p.tools.map((tool) => tool.id),
    ["shared", "live", "completed-live"],
  );
  p.load(entries, "right");
  assert.deepEqual(
    p.tools.map((tool) => tool.id),
    ["shared", "live", "completed-live"],
  );
});

test("hidden custom messages never enter live or refreshed transcripts", () => {
  const p = new Projection();
  const message = {
    role: "custom",
    customType: "context",
    content: "Private context",
    display: false,
    timestamp: 100,
  };
  p.event({ type: "message_start", message });
  p.event({ type: "message_end", message });
  assert.deepEqual(p.messages, []);
  p.load(
    [
      {
        id: "hidden",
        parentId: null,
        type: "custom_message",
        ...message,
        timestamp: new Date(105).toISOString(),
      },
    ],
    "hidden",
  );
  assert.deepEqual(p.messages, []);
});

test("custom persistence replaces live occurrences and removes abandoned branch messages", () => {
  const p = new Projection();
  const message = {
    role: "custom",
    customType: "note",
    content: "Repeated note",
    display: true,
    timestamp: 100,
  };
  const entries = [
    {
      id: "first",
      parentId: null,
      type: "custom_message",
      ...message,
      timestamp: new Date(105).toISOString(),
    },
    {
      id: "second",
      parentId: "first",
      type: "custom_message",
      ...message,
      timestamp: new Date(205).toISOString(),
    },
    {
      id: "other",
      parentId: null,
      type: "message",
      message: { role: "user", content: "Other branch", timestamp: 300 },
    },
  ];
  p.event({ type: "message_end", message });
  p.event({ type: "message_end", message: { ...message, timestamp: 200 } });
  p.load(entries, "second");
  assert.deepEqual(
    p.messages.map((row) => row.id),
    ["first", "second"],
  );
  // An identical new occurrence must survive a sync before it is persisted.
  p.event({ type: "message_end", message: { ...message, timestamp: 400 } });
  p.load(entries, "second");
  assert.equal(p.messages.length, 3);
  entries.push({
    id: "third",
    parentId: "second",
    type: "custom_message",
    ...message,
    timestamp: new Date(450).toISOString(),
  });
  p.load(entries, "third");
  assert.deepEqual(
    p.messages.map((row) => row.id),
    ["first", "second", "third"],
  );
  p.load(entries, "other");
  assert.deepEqual(
    p.messages.map((row) => row.text),
    ["Other branch"],
  );
});

test("custom messages sharing a timestamp retain their distinct contents", () => {
  const p = new Projection();
  for (const content of ["First", "Second"])
    p.event({
      type: "message_end",
      message: {
        role: "custom",
        customType: "note",
        display: true,
        timestamp: 100,
        content,
      },
    });
  assert.deepEqual(
    p.messages.map((row) => row.text),
    ["First", "Second"],
  );
});

test("native shell execution rows preserve command output and outcome in live and history", () => {
  for (const outcome of [
    { exitCode: 0, cancelled: false, truncated: false },
    { exitCode: 7, cancelled: false, truncated: false },
    {
      exitCode: undefined,
      cancelled: true,
      truncated: true,
      fullOutputPath: "/tmp/shell-output",
    },
  ]) {
    const p = new Projection();
    const message = {
      role: "bashExecution",
      command: "printf shell-output",
      output: "shell-output",
      timestamp: 42,
      ...outcome,
    };
    p.event({ type: "message_end", message });
    const live = p.messages[0];
    assert.ok(live.text.includes(message.command));
    assert.ok(live.text.includes(message.output));
    if (outcome.cancelled) {
      assert.match(live.text, /cancelled/i);
      assert.match(live.text, /truncated/i);
      assert.ok(live.text.includes(outcome.fullOutputPath));
      assert.match(live.error, /cancelled/i);
    } else {
      assert.ok(live.text.includes(`Exit code: ${outcome.exitCode}`));
      if (outcome.exitCode) assert.match(live.error, /7/);
      else assert.equal(live.error, undefined);
    }
    p.load(
      [{ id: "shell", parentId: null, type: "message", message }],
      "shell",
    );
    assert.equal(p.messages.length, 1);
    assert.equal(p.messages[0].text, live.text);
  }
});

test("identical custom messages in the same millisecond remain separate occurrences", () => {
  const p = new Projection();
  const message = {
    role: "custom",
    customType: "note",
    content: "Repeated",
    display: true,
    timestamp: 100,
  };
  for (let i = 0; i < 2; i++) {
    p.event({ type: "message_start", message });
    p.event({ type: "message_end", message });
  }
  assert.equal(p.messages.length, 2);
  assert.notEqual(p.messages[0].id, p.messages[1].id);
  const entries = ["a", "b"].map((id, index) => ({
    id,
    parentId: index ? "a" : null,
    type: "custom_message",
    ...message,
    timestamp: new Date(110 + index).toISOString(),
  }));
  p.load(entries, "b");
  assert.deepEqual(
    p.messages.map((row) => row.id),
    ["a", "b"],
  );
});

test("shell projection bounds output without losing status and handles empty output", () => {
  const p = new Projection();
  const message = {
    role: "bashExecution",
    command: "test",
    output: "x".repeat(100000),
    exitCode: 3,
    cancelled: false,
    truncated: false,
    timestamp: 1,
  };
  p.load([{ id: "shell", parentId: null, type: "message", message }], "shell");
  assert.ok(p.messages[0].text.length < 65536);
  assert.match(p.messages[0].text, /Display truncated/);
  assert.match(p.messages[0].text, /Exit code: 3/);
  p.load(
    [
      {
        id: "empty",
        parentId: null,
        type: "message",
        message: { ...message, output: "", exitCode: 0 },
      },
    ],
    "empty",
  );
  assert.match(p.messages[0].text, /no output/);
  assert.match(p.messages[0].text, /Exit code: 0/);
});

const nativeMessage = (id, message, parentId = null) => ({
  id,
  parentId,
  type: "message",
  message,
});
const complete = (p, message) => {
  p.event({ type: "message_start", message });
  p.event({ type: "message_end", message });
};

for (const contents of [
  ["First", "Second"],
  ["Identical", "Identical"],
]) {
  test(`same-millisecond user occurrences retain ${contents[0] === contents[1] ? "identical" : "distinct"} messages through start/end and native refresh`, () => {
    const p = new Projection();
    const messages = contents.map((text) => ({
      role: "user",
      timestamp: 100,
      content: [{ type: "text", text }],
    }));
    for (const message of messages) complete(p, message);
    assert.deepEqual(
      p.messages.map((r) => r.text),
      contents,
    );
    assert.equal(new Set(p.messages.map((r) => r.id)).size, 2);
    const entries = messages.map((message, i) =>
      nativeMessage(`native-${i}`, message, i ? "native-0" : null),
    );
    p.load(entries, "native-1");
    assert.deepEqual(
      p.messages.map((r) => r.id),
      ["native-0", "native-1"],
    );
    p.load(entries, "native-1");
    assert.deepEqual(
      p.messages.map((r) => r.text),
      contents,
    );
  });
}

test("partial persistence and repeated refreshes consume each identical live occurrence only once", () => {
  const p = new Projection();
  const message = { role: "user", timestamp: 100, content: "Repeated" };
  complete(p, message);
  complete(p, message);
  const entries = [nativeMessage("first", message)];
  p.load(entries, "first");
  assert.equal(p.messages.length, 2);
  assert.equal(p.messages[0].id, "first");
  const secondId = p.messages[1].id;
  p.load(entries, "first");
  assert.equal(p.messages[1].id, secondId);
  complete(p, message);
  const thirdId = p.messages[2].id;
  p.load(entries, "first");
  assert.equal(p.messages.length, 3);
  entries.push(nativeMessage("second", message, "first"));
  p.load(entries, "second");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["first", "second", thirdId],
  );
  entries.push(nativeMessage("third", message, "second"));
  p.load(entries, "third");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["first", "second", "third"],
  );
});

test("same-millisecond assistant partial/final occurrences remain separate from persisted predecessors", () => {
  const p = new Projection();
  const first = {
    role: "assistant",
    timestamp: 10,
    content: [{ type: "text", text: "First" }],
    stopReason: "stop",
  };
  complete(p, first);
  const entries = [nativeMessage("first", first)];
  p.load(entries, "first");
  p.event({
    type: "message_start",
    message: { role: "assistant", timestamp: 10, content: [] },
  });
  p.event({
    type: "message_update",
    assistantMessageEvent: {
      type: "text_delta",
      contentIndex: 0,
      delta: "Second partial",
    },
  });
  const partialId = p.messages[1].id;
  assert.notEqual(partialId, "first");
  p.load(entries, "first");
  p.load(entries, "first");
  assert.deepEqual(
    p.messages.map((r) => r.text),
    ["First", "Second partial"],
  );
  assert.equal(p.messages[1].id, partialId);
  const second = {
    ...first,
    content: [{ type: "text", text: "Second final" }],
  };
  p.event({ type: "message_end", message: second });
  assert.deepEqual(
    p.messages.map((r) => r.text),
    ["First", "Second final"],
  );
  assert.equal(p.messages[1].id, partialId);
  entries.push(nativeMessage("second", second, "first"));
  p.load(entries, "second");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["first", "second"],
  );
});

test("an identical new assistant occurrence survives old native entries and branch changes", () => {
  const p = new Projection();
  const message = {
    role: "assistant",
    timestamp: 10,
    content: [{ type: "text", text: "Repeated" }],
  };
  const entries = [
    nativeMessage("old", message),
    nativeMessage("other", {
      role: "user",
      timestamp: 11,
      content: "Other branch",
    }),
  ];
  complete(p, message);
  p.load(entries, "old");
  p.event({ type: "message_start", message: { ...message, content: [] } });
  p.event({
    type: "message_update",
    assistantMessageEvent: {
      type: "text_delta",
      contentIndex: 0,
      delta: "Repeated",
    },
  });
  p.load(entries, "other");
  assert.deepEqual(
    p.messages.map((r) => r.text),
    ["Other branch", "Repeated"],
  );
  p.event({ type: "message_end", message });
  p.load(entries, "other");
  assert.deepEqual(
    p.messages.map((r) => r.text),
    ["Other branch", "Repeated"],
  );
  entries.push(nativeMessage("new", message, "other"));
  p.load(entries, "new");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["other", "new"],
  );
  p.load(entries, "old");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["old"],
  );
});

test("tool and shell occurrences use unique entry IDs while tool IDs keep the client correlation suffix", () => {
  for (const message of [
    {
      role: "toolResult",
      timestamp: 100,
      toolCallId: "call-123",
      toolName: "read",
      content: "Repeated",
    },
    {
      role: "bashExecution",
      timestamp: 100,
      command: "printf hi",
      output: "hi",
      exitCode: 0,
    },
  ]) {
    const p = new Projection();
    complete(p, message);
    complete(p, message);
    assert.equal(p.messages.length, 2);
    assert.equal(new Set(p.messages.map((r) => r.id)).size, 2);
    if (message.role === "toolResult")
      assert.ok(p.messages.every((r) => r.id.endsWith("-call-123")));
    const entries = [
      nativeMessage("first", message),
      nativeMessage("second", message, "first"),
    ];
    p.load(entries, "second");
    assert.equal(p.messages.length, 2);
    assert.equal(new Set(p.messages.map((r) => r.id)).size, 2);
    if (message.role === "toolResult")
      assert.deepEqual(
        p.messages.map((r) => r.id),
        ["first-call-123", "second-call-123"],
      );
    else
      assert.deepEqual(
        p.messages.map((r) => r.id),
        ["first", "second"],
      );
  }
});

test("full native payload matching does not conflate messages with the same clipped display text", () => {
  const p = new Projection();
  const prefix = "x".repeat(70000);
  const first = { role: "user", timestamp: 100, content: prefix + "first" };
  const second = { ...first, content: prefix + "second" };
  complete(p, first);
  complete(p, second);
  const secondId = p.messages[1].id;
  p.load([nativeMessage("first", first)], "first");
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["first", secondId],
  );
});

for (const texts of [
  ["First", "Second"],
  ["Identical", "Identical"],
]) {
  test(`interleaved same-timestamp user starts/ends pair ${texts[0] === texts[1] ? "identical" : "distinct"} occurrences`, () => {
    const p = new Projection();
    const messages = texts.map((content) => ({
      role: "user",
      timestamp: 10,
      content,
    }));
    for (const message of messages) p.event({ type: "message_start", message });
    const ids = p.messages.map((r) => r.id);
    for (const message of messages) p.event({ type: "message_end", message });
    assert.deepEqual(
      p.messages.map((r) => r.text),
      texts,
    );
    assert.deepEqual(
      p.messages.map((r) => r.id),
      ids,
    );
    assert.equal(new Set(ids).size, 2);
    const entries = messages.map((m, i) =>
      nativeMessage(`native-${i}`, m, i ? "native-0" : null),
    );
    p.load(entries, "native-1");
    assert.deepEqual(
      p.messages.map((r) => r.id),
      ["native-0", "native-1"],
    );
  });
}

test("native persistence between a user start and end does not create a second occurrence", () => {
  const p = new Projection();
  const message = {
    role: "user",
    timestamp: 10,
    content: "Started then persisted",
  };
  p.event({ type: "message_start", message });
  p.load([nativeMessage("native", message)], "native");
  p.event({ type: "message_end", message });
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["native"],
  );
});

test("native-only colliding timestamps use entry IDs and repeated live occurrences retain the row bound", () => {
  const message = { role: "user", timestamp: 10, content: "Repeated" };
  const p = new Projection();
  p.load(
    [nativeMessage("one", message), nativeMessage("two", message, "one")],
    "two",
  );
  assert.deepEqual(
    p.messages.map((r) => r.id),
    ["one", "two"],
  );
  p.reset();
  for (let i = 0; i < 600; i++) complete(p, message);
  assert.equal(p.messages.length, 500);
  assert.equal(new Set(p.messages.map((r) => r.id)).size, 500);
});
