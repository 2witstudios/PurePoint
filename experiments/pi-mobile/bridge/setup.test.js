import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  writeFile,
  rm,
  symlink,
  readFile,
  stat,
} from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { discoverPuSkills, nativeSessions } from "./setup.js";
test("installed skill discovery follows owner symlinks without a pinned cache path", async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), "pi-skill-discovery-"));
  try {
    const skill = path.join(home, "pointguard/pu");
    await mkdir(skill, { recursive: true });
    await writeFile(
      path.join(skill, "SKILL.md"),
      "---\nname: pu\ndescription: PurePoint\n---",
    );
    await mkdir(path.join(home, ".agents/skills"), { recursive: true });
    await symlink(skill, path.join(home, ".agents/skills/pu"));
    assert.ok(
      (await discoverPuSkills(home)).some((x) => x.endsWith("/pu/SKILL.md")),
    );
  } finally {
    await rm(home, { recursive: true, force: true });
  }
});
