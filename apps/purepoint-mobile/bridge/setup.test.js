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
import { discoverPuSkills, ensureToken, loadToken } from "./setup.js";
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
test("owner secrets accept a trailing newline but reject invalid HTTP header characters without replacement", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-pair-header-"));
  const file = path.join(dir, "secret");
  const token = "a".repeat(43);
  try {
    for (const ending of ["\n", "\r\n"]) {
      await writeFile(file, token + ending, { mode: 0o600 });
      assert.equal(await loadToken(file), token);
      assert.equal(await ensureToken(file), token);
    }
    for (const invalid of ["\r", "\n", "\r\n", "\0", "\u0001", "\u0100"]) {
      const contents = token + invalid + "suffix\n";
      await writeFile(file, contents);
      await assert.rejects(ensureToken(file), (error) => {
        assert.match(error.message, /single-line HTTP header/);
        assert.match(error.message, /PI_MOBILE_TOKEN_FILE/);
        assert.equal(error.message.includes(token), false);
        return true;
      });
      assert.equal(await readFile(file, "utf8"), contents);
    }
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
