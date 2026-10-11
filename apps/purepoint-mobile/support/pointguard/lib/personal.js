import { constants } from "node:fs";
import { mkdir, lstat, open, readFile, link, rm } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import path from "node:path";

export const personalLimit = 32 * 1024;
function validate(info, directory = false) {
  if ((directory ? !info.isDirectory() : !info.isFile()) ||
      (info.mode & 0o077) !== 0 ||
      (typeof process.getuid === "function" && info.uid !== process.getuid()))
    throw new Error("Point Guard personal files must be private, regular and owned by this user; restore permissions deliberately.");
  if (!directory && info.size > personalLimit)
    throw new Error("Point Guard personal files must be at most 32 KiB; shorten the file deliberately.");
}
export async function readPersonal(file) {
  const handle = await open(file, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    validate(await handle.stat());
    // Bounded read also handles a concurrent editor growing the file after stat.
    const buffer = Buffer.alloc(personalLimit + 1);
    let total = 0;
    while (total < buffer.length) {
      const { bytesRead } = await handle.read(buffer, total, buffer.length - total, null);
      if (!bytesRead) break;
      total += bytesRead;
    }
    if (total > personalLimit) throw new Error("Point Guard personal files must be at most 32 KiB.");
    return buffer.subarray(0, total).toString("utf8");
  } finally { await handle.close(); }
}
/** Publish complete private seeds without replacing an existing owner edit. */
export async function seedPersonal(directory, defaults) {
  if (!path.isAbsolute(directory)) throw new Error("Point Guard personal directory must be absolute.");
  await mkdir(directory, { recursive: true, mode: 0o700 });
  validate(await lstat(directory), true);
  for (const name of ["SOUL.md", "USER.md"]) {
    const file = path.join(directory, name);
    try { await readPersonal(file); continue; }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    const temporary = path.join(directory, `.${name}.${randomUUID()}.tmp`);
    try {
      const content = await readFile(path.join(defaults, name), "utf8");
      const handle = await open(temporary, "wx", 0o600);
      try { await handle.writeFile(content); } finally { await handle.close(); }
      try { await link(temporary, file); }
      catch (error) { if (error.code !== "EEXIST") throw error; }
    } finally { await rm(temporary, { force: true }); }
    await readPersonal(file);
  }
}
export async function personalSections(directory) {
  validate(await lstat(directory), true);
  const [soul, user] = await Promise.all([readPersonal(path.join(directory, "SOUL.md")), readPersonal(path.join(directory, "USER.md"))]);
  return { pointguard_soul: soul, pointguard_user: user };
}
