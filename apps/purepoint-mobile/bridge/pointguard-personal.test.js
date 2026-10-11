import test from "node:test";
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readFile,
  writeFile,
  chmod,
  symlink,
  stat,
  rm,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import pointguard from "../support/pointguard/extensions/pointguard.js";
import {
  seedPersonal,
  personalSections,
  readPersonal,
} from "../support/pointguard/lib/personal.js";
import { root, launchArguments } from "./setup.js";
const defaults = path.join(root, "support/pointguard/defaults");
test("personal seeds publish once, preserve edits/empty files across restart and reject unsafe files", async (t) => {
  const home = await mkdtemp(path.join(os.tmpdir(), "pointguard-personal-"));
  t.after(() => rm(home, { recursive: true, force: true }));
  const dir = path.join(home, "personal");
  await Promise.all([seedPersonal(dir, defaults), seedPersonal(dir, defaults)]);
  assert.equal((await stat(dir)).mode & 0o777, 0o700);
  assert.equal((await stat(path.join(dir, "SOUL.md"))).mode & 0o777, 0o600);
  await writeFile(path.join(dir, "SOUL.md"), "An explicitly edited soul.");
  await writeFile(path.join(dir, "USER.md"), "");
  await seedPersonal(dir, defaults);
  assert.deepEqual(await personalSections(dir), {
    pointguard_soul: "An explicitly edited soul.",
    pointguard_user: "",
  });
  await chmod(path.join(dir, "SOUL.md"), 0o644);
  await assert.rejects(seedPersonal(dir, defaults), /private/);
  assert.equal(
    await readFile(path.join(dir, "SOUL.md"), "utf8"),
    "An explicitly edited soul.",
  );
  await chmod(path.join(dir, "SOUL.md"), 0o600);
  await writeFile(path.join(dir, "SOUL.md"), "x".repeat(32769));
  await assert.rejects(readPersonal(path.join(dir, "SOUL.md")), /32 KiB/);
  await rm(path.join(dir, "SOUL.md"));
  await symlink(path.join(dir, "USER.md"), path.join(dir, "SOUL.md"));
  await assert.rejects(seedPersonal(dir, defaults));
});
test("hook rereads current sections without replacing native tools/skills, and reload waits for idle", async (t) => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-hook-"));
  const previous = process.env.POINT_GUARD_PERSONAL_DIR;
  process.env.POINT_GUARD_PERSONAL_DIR = dir;
  t.after(async () => {
    if (previous === undefined) delete process.env.POINT_GUARD_PERSONAL_DIR;
    else process.env.POINT_GUARD_PERSONAL_DIR = previous;
    await rm(dir, { recursive: true, force: true });
  });
  const handlers = {},
    commands = {};
  pointguard({
    on: (name, handler) => {
      handlers[name] = handler;
    },
    registerCommand: (name, command) => {
      commands[name] = command;
    },
  });
  await handlers.session_start({ reason: "resume" });
  const tools = ["read", "bash", "edit", "write"],
    skills = [{ name: "fixture" }];
  const event = {
    systemPromptOptions: {
      sections: { existing: "preserve" },
      selectedTools: tools,
      skills,
      promptGuidelines: ["native"],
    },
  };
  await handlers.before_agent_start(event);
  await writeFile(path.join(dir, "SOUL.md"), "current soul");
  await handlers.before_agent_start(event);
  assert.equal(
    event.systemPromptOptions.sections.pointguard_soul,
    "current soul",
  );
  assert.equal(event.systemPromptOptions.sections.existing, "preserve");
  assert.equal(event.systemPromptOptions.selectedTools, tools);
  assert.equal(event.systemPromptOptions.skills, skills);
  assert.deepEqual(event.systemPromptOptions.promptGuidelines, ["native"]);
  assert.deepEqual(Object.keys(event.systemPromptOptions.sections).sort(), [
    "existing",
    "pointguard_operating",
    "pointguard_soul",
    "pointguard_user",
  ]);
  const order = [];
  await commands.pointguard.handler("reload", {
    waitForIdle: async () => {
      order.push("idle");
    },
    ui: { notify: () => order.push("notice") },
    reload: async () => {
      order.push("reload");
    },
  });
  assert.deepEqual(order, ["idle", "notice", "reload"]);
});
test("launch requires a native package root and selects only the explicit workflow counterpart", async () => {
  await assert.rejects(
    launchArguments(os.tmpdir(), { POINT_GUARD_WORKFLOW_SOURCE: "relative" }),
    /absolute Pi package root/,
  );
  await assert.rejects(
    launchArguments(os.tmpdir(), {
      POINT_GUARD_WORKFLOW_SOURCE: path.join(root, "support/pu"),
    }),
  );
});

test("explicit local symlink selects one canonical native package instead of bundled workflow source", async (t) => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-source-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const actual = path.join(root, "support/pointguard-workflows");
  const alias = path.join(dir, "package-alias");
  await symlink(actual, alias);
  const args = await launchArguments(os.tmpdir(), {
    POINT_GUARD_WORKFLOW_SOURCE: alias,
  });
  const sources = args.flatMap((x, i) =>
    x === "--extension" ? [args[i + 1]] : [],
  );
  assert.deepEqual(sources, [path.join(root, "support/pointguard"), actual]);
  assert.ok(!args.includes("--append-system-prompt"));
});

test("unsafe personal FIFO refuses promptly instead of blocking startup", async (t) => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-fifo-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const fifo = path.join(dir, "SOUL.md");
  execFileSync("/usr/bin/mkfifo", ["-m", "600", fifo]);
  const moduleURL = new URL(
    "../support/pointguard/lib/personal.js",
    import.meta.url,
  ).href;
  const script = `import { readPersonal } from ${JSON.stringify(moduleURL)};
    try { await readPersonal(process.argv[1]); process.exit(1); }
    catch (error) { if (!error.message.includes("regular")) throw error; }`;
  execFileSync(process.execPath, ["--input-type=module", "-e", script, fifo], {
    timeout: 2000,
  });
});
