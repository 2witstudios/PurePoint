import { mkdtemp } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { makePrivate } from "./acl.js";

/**
 * Test helper: a fresh private temp directory. `mkdtemp` is 0700 on Unix; on
 * Windows it inherits TEMP's ACL until given the owner-only one.
 */
export async function privateTemp(prefix) {
  const directory = await mkdtemp(path.join(os.tmpdir(), prefix));
  if (process.platform === "win32") await makePrivate(directory);
  return directory;
}
