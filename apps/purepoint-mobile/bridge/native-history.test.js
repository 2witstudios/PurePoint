import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
test("native session list and read-only branch history skip corrupt/partial records without rewriting the Pi file", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-native-history-"));
  try {
    const sessionDir = path.join(dir, "agent/sessions/--project--");
    await mkdir(sessionDir, { recursive: true });
    const file = path.join(sessionDir, "2026-01-01_native-session.jsonl");
    const records = [
      {
        type: "session",
        version: 3,
        id: "native-session",
        timestamp: "2026-01-01T00:00:00Z",
        cwd: "/project",
      },
      {
        type: "message",
        id: "a",
        parentId: null,
        timestamp: "2026-01-01T00:00:01Z",
        message: { role: "user", timestamp: 1, content: "Original" },
      },
      {
        type: "message",
        id: "b",
        parentId: "a",
        timestamp: "2026-01-01T00:00:02Z",
        message: {
          role: "assistant",
          timestamp: 2,
          content: [{ type: "text", text: "Abandoned" }],
        },
      },
      {
        type: "branch_summary",
        id: "c",
        parentId: "a",
        timestamp: "2026-01-01T00:00:03Z",
        fromId: "b",
        summary: "A prior exploration",
      },
      {
        type: "message",
        id: "d",
        parentId: "c",
        timestamp: "2026-01-01T00:00:04Z",
        message: {
          role: "assistant",
          timestamp: 3,
          content: [{ type: "text", text: "Current branch" }],
        },
      },
    ];
    const lines = records.map((x) => JSON.stringify(x));
    lines.splice(2, 0, "{malformed record", "   ");
    lines.push('{"type":"message","id":"unfinished"');
    const original = lines.join("\n");
    await writeFile(file, original);
    const code = `import {nativeSessions} from './bridge/setup.js';const sessions=await nativeSessions();const list=await sessions.list();if(list.length!==1)throw Error('session discovery');const history=await sessions.history('native-session');console.log(JSON.stringify(history));`;
    const result = await promisify(execFile)(
      process.execPath,
      ["--input-type=module", "-e", code],
      {
        cwd: new URL("../", import.meta.url),
        env: {
          PATH: process.env.PATH,
          HOME: dir,
          PI_CODING_AGENT_DIR: path.join(dir, "agent"),
        },
      },
    );
    const history = JSON.parse(result.stdout);
    assert.equal(
      history.messages.some((x) => x.text === "Abandoned"),
      false,
    );
    assert.equal(history.messages.at(-1).text, "Current branch");
    assert.ok(history.messages.some((x) => x.text === "Original"));
    assert.ok(
      history.messages.some((x) => x.text.includes("A prior exploration")),
    );
    assert.equal(await readFile(file, "utf8"), original);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
