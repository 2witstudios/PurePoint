import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { Rpc } from "./rpc.js";
import { cli } from "./setup.js";
test("published vanilla Pi supports the pinned state/history/queue contract", async () => {
  const folder = await mkdtemp(path.join(os.tmpdir(), "pi-mobile-contract-"));
  const skill = path.join(folder, "pu");
  await mkdir(skill);
  await writeFile(
    path.join(skill, "SKILL.md"),
    "---\nname: pu\ndescription: PurePoint test skill\n---\nRead the CLI reference.",
  );
  // Isolate config and HOME, disable external extensions/skills; no owner auth files or model requests.
  const rpc = new Rpc(
    process.execPath,
    [
      cli,
      "--mode",
      "rpc",
      "--no-session",
      "--no-extensions",
      "--no-skills",
      "--skill",
      skill,
      "--no-approve",
    ],
    folder,
    {
      PATH: process.env.PATH,
      HOME: folder,
      PI_CODING_AGENT_DIR: path.join(folder, "agent"),
      PI_SKIP_VERSION_CHECK: "1",
    },
  );
  rpc.on("failure", () => {});
  try {
    const state = await rpc.call("get_state");
    assert.equal(state.isStreaming, false);
    const entries = await rpc.call("get_entries");
    assert.equal(
      entries.entries.some((e) => e.type === "message"),
      false,
    );
    assert.ok(entries.entries.some((e) => e.id === entries.leafId));
    assert.deepEqual(await rpc.call("clear_queue"), {
      steering: [],
      followUp: [],
    });
    await rpc.call("abort");
    assert.ok(
      (await rpc.call("get_commands")).commands.some(
        (c) => c.name === "skill:pu" && c.source === "skill",
      ),
    );
  } finally {
    await rpc.close();
    await rm(folder, { recursive: true, force: true });
  }
});
