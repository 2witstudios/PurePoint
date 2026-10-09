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
