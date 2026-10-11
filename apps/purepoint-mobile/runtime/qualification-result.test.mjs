import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn } from "node:child_process";
import { promisify } from "node:util";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  catalogResult,
  confirmExit,
  qualificationFailed,
  requirements,
} from "./qualification-result.mjs";
import { Rpc } from "../bridge/rpc.js";

const run = promisify(execFile);
const mobile = fileURLToPath(new URL("../", import.meta.url));

test("missing native candidate capabilities and lifecycle errors cannot pass", () => {
  for (const [name, required] of Object.entries(requirements)) {
    const record = {
      name,
      extensionErrors: [],
      catalog: {
        tools: (required.tools ?? []).map((name) => ({ name })),
        skills: required.skills ?? [],
      },
    };
    assert.match(catalogResult(record), /^PASS/);
    if (required.tools) record.catalog.tools.pop();
    else record.catalog.skills = [];
    assert.match(catalogResult(record), /^FAIL missing candidate capabilities/);
    record.extensionErrors = [{}];
    assert.equal(catalogResult(record), "FAIL extension lifecycle");
  }
  assert.equal(
    qualificationFailed([
      { result: "REJECT engine" },
      { result: "PASS", teardown: true },
    ]),
    false,
  );
  assert.equal(
    qualificationFailed([{ result: "FAIL startup", teardown: true }]),
    true,
  );
  assert.equal(
    qualificationFailed([{ result: "PASS", teardown: false }]),
    true,
  );
});

test("child exit confirmation waits for actual exit and times out boundedly", async () => {
  const child = spawn(process.execPath, [
    "-e",
    "setTimeout(()=>process.exit(0),150)",
  ]);
  assert.equal(await confirmExit(child, 3000), true);
  assert.equal(await confirmExit(child, 10), true);
  assert.equal(child.listenerCount("exit"), 0);
  const stubborn = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"]);
  try {
    assert.equal(await confirmExit(stubborn, 50), false);
    assert.equal(stubborn.listenerCount("exit"), 0);
  } finally {
    stubborn.kill("SIGKILL");
    assert.equal(await confirmExit(stubborn, 3000), true);
  }
});

test("actual Rpc.close SIGTERM fallback is confirmed before teardown passes", async () => {
  const rpc = new Rpc(
    process.execPath,
    ["-e", "process.stdin.resume();setInterval(()=>{},1000)"],
    os.tmpdir(),
    { PATH: "/usr/bin:/bin" },
  );
  rpc.on("failure", () => {});
  try {
    await rpc.close();
    assert.equal(await confirmExit(rpc.child), true);
    assert.equal(rpc.child.signalCode, "SIGTERM");
  } finally {
    if (rpc.child.exitCode === null && rpc.child.signalCode === null) {
      rpc.child.kill("SIGKILL");
      await confirmExit(rpc.child);
    }
  }
});

test(
  "real native RPC qualifier writes diagnostics and fails the shell for missing skills or lifecycle failures",
  { timeout: 90000 },
  async () => {
    const scratch = await mkdtemp(
      path.join(os.tmpdir(), "qualification-regression-"),
    );
    try {
      for (const [name, required] of Object.entries(requirements)) {
        const pkg = path.join(scratch, name.replaceAll("/", "-"), "package");
        await mkdir(pkg, { recursive: true });
        const manifest = { name, version: "fixture", pi: {} };
        if (required.tools) {
          manifest.pi.extensions = ["index.js"];
          await writeFile(
            path.join(pkg, "index.js"),
            `export default function(pi){ for(const name of ${JSON.stringify(required.tools)}) pi.registerTool({name,label:name,description:name,parameters:{type:'object',properties:{}},execute:async()=>({content:[{type:'text',text:'fixture'}]})}); }`,
          );
        } else {
          manifest.pi.skills = ["skills"];
          await mkdir(path.join(pkg, "skills/playwright-browser"), {
            recursive: true,
          });
          await writeFile(
            path.join(pkg, "skills/playwright-browser/SKILL.md"),
            "---\nname: playwright-browser\ndescription: Browser fixture for native catalog qualification.\n---\nFixture only.\n",
          );
        }
        await writeFile(
          path.join(pkg, "package.json"),
          JSON.stringify(manifest),
        );
      }
      const rejected = path.join(scratch, "pi-agent-browser-native/package");
      await mkdir(rejected, { recursive: true });
      await writeFile(
        path.join(rejected, "package.json"),
        JSON.stringify({
          name: "pi-agent-browser-native",
          version: "fixture",
          engines: { node: ">=24.21.0" },
          pi: {},
        }),
      );
      const resultFile = path.join(scratch, "result.json");
      const args = [
        path.join(mobile, "runtime/qualify-marketplace.mjs"),
        mobile,
        scratch,
        resultFile,
      ];
      await run(process.execPath, args, { timeout: 60000 });
      let records = JSON.parse(await readFile(resultFile, "utf8"));
      assert.equal(
        records.filter((r) => r.result.startsWith("PASS") && r.teardown).length,
        3,
      );
      // Pi skips malformed skill frontmatter; fixture command still succeeds.
      await writeFile(
        path.join(
          scratch,
          "pi-playwright/package/skills/playwright-browser/SKILL.md",
        ),
        "No valid skill frontmatter.\n",
      );
      await writeFile(
        path.join(scratch, "pi-web-access/package/index.js"),
        "export default function(pi){pi.on('session_start',()=>{throw new Error('fixture lifecycle failure')});}",
      );
      await assert.rejects(
        run(process.execPath, args, { timeout: 60000 }),
        (error) => error.code === 1,
      );
      records = JSON.parse(await readFile(resultFile, "utf8"));
      assert.match(
        records.find((r) => r.name === "pi-playwright").result,
        /FAIL missing candidate capabilities: playwright-browser/,
      );
      assert.equal(
        records.find((r) => r.name === "pi-web-access").result,
        "FAIL extension lifecycle",
      );
      assert.equal(records.filter((r) => r.teardown === true).length, 3);
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  },
);
