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
import { discoverPuSkills, nativeSessions, ensureToken } from "./setup.js";
test("given first setup should generate a private secret once and reuse it across concurrent starts", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-pair-secret-"));
  const file = path.join(dir, "private/secret");
  try {
    const tokens = await Promise.all([ensureToken(file), ensureToken(file)]);
    assert.equal(tokens[0], tokens[1]);
    assert.match(tokens[0], /^[A-Za-z0-9_-]{43}$/);
    assert.equal((await stat(file)).mode & 0o777, 0o600);
    assert.equal(await ensureToken(file), tokens[0]);
    await writeFile(file, "invalid");
    await assert.rejects(ensureToken(file));
    assert.equal(await readFile(file, "utf8"), "invalid");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
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
