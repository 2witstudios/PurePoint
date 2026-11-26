import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

/**
 * Windows owner-only state (WINAPP-008): a protected DACL with no inherited
 * entries, full control for the current user and SYSTEM. This is the Windows
 * meaning of a 0700 directory; files created inside inherit it.
 *
 * System tools run by absolute path, without a shell, so neither `PATH`
 * (Git's `whoami`, a planted `icacls`) nor shell quoting is involved.
 */
const run = promisify(execFile);
const SYSTEM_SID = "S-1-5-18";
/** SDDL aliases icacls prints for well-known SIDs we accept. */
const ALLOWED_ALIASES = new Map([
  ["SY", SYSTEM_SID],
  ["BA", "S-1-5-32-544"], // Administrators already control the machine.
]);

export class AclError extends Error {
  constructor(message, details) {
    super(message);
    this.code = "acl_private";
    this.details = details;
    // CI runners differ from desktops (elevated admin accounts): show what
    // was refused so a failure is diagnosable from the log.
    if (details && process.env.CI)
      console.error("acl refused:", JSON.stringify(details));
  }
}

function systemTool(name, env = process.env) {
  const root = env.SystemRoot ?? env.windir ?? "C:\\Windows";
  return path.win32.join(root, "System32", name);
}

const tool = async (name, args) =>
  (
    await run(systemTool(name), args, {
      encoding: "utf8",
      windowsHide: true,
      timeout: 15000,
      maxBuffer: 1024 * 1024,
    })
  ).stdout;

let cachedSid;
/** The current user's SID from `whoami /user` (locale-independent). */
export async function currentUserSid() {
  if (cachedSid) return cachedSid;
  const out = await tool("whoami.exe", ["/user", "/fo", "csv", "/nh"]);
  const sid = out.trim().split(",").pop()?.replaceAll('"', "");
  if (!sid || !/^S-1-\d+(-\d+)+$/.test(sid))
    throw new AclError("Cannot identify the current Windows user.");
  return (cachedSid = sid);
}

/**
 * Parse the DACL line `icacls <path> /save` writes (UTF-16LE: the name, then
 * `D:<flags>(A;<ace flags>;<rights>;;;<sid>)…`).
 */
export function parseSavedAcl(text) {
  const line = String(text)
    .replace(/^\uFEFF/, "")
    .split(/\r?\n/)
    .find((l) => l.startsWith("D:"));
  if (!line) throw new AclError("Unreadable Windows ACL.");
  const header = /^D:([A-Z]*)/.exec(line);
  const flags = header?.[1] ?? "";
  const aces = [...line.matchAll(/\(([^)]*)\)/g)].map((m) => {
    const [type, aceFlags, rights, , , sid] = m[1].split(";");
    return {
      type,
      flags: aceFlags,
      rights,
      sid: ALLOWED_ALIASES.get(sid) ?? sid,
    };
  });
  return { protected: flags.includes("P"), aces };
}

export async function readAcl(target) {
  const scratch = await mkdtemp(path.join(os.tmpdir(), "pp-acl-"));
  try {
    const file = path.join(scratch, "acl.txt");
    await tool("icacls.exe", [target, "/save", file]);
    return parseSavedAcl(await readFile(file, "utf16le"));
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

/**
 * Owner-only check: every entry is an allow for the user, SYSTEM or
 * Administrators. A directory must be protected with no inherited entries; a
 * file may inherit from its (checked) private parent.
 */
export function isPrivateAcl(acl, userSid, { directory = false } = {}) {
  if (directory && !acl.protected) return false;
  if (!acl.aces.some((a) => a.sid === userSid)) return false;
  return acl.aces.every(
    (a) =>
      a.type === "A" &&
      [userSid, SYSTEM_SID, "S-1-5-32-544"].includes(a.sid) &&
      !(directory && a.flags.includes("ID")),
  );
}

export async function assertPrivate(target, { directory = false } = {}) {
  const [acl, sid] = await Promise.all([readAcl(target), currentUserSid()]);
  if (!isPrivateAcl(acl, sid, { directory }))
    throw new AclError(
      "Point Guard state must be private and owned by this user.",
      { acl, userSid: sid },
    );
  return acl;
}

/**
 * Replace the ACL with `/inheritance:r /grant:r user + SYSTEM`, then verify.
 * `(OI)(CI)` makes files and subdirectories created later inherit it.
 */
export async function makePrivate(target, { directory = true } = {}) {
  const sid = await currentUserSid();
  const inherit = directory ? "(OI)(CI)" : "";
  await tool("icacls.exe", [
    target,
    "/inheritance:r",
    "/grant:r",
    `*${sid}:${inherit}F`,
    `*${SYSTEM_SID}:${inherit}F`,
  ]);
  return assertPrivate(target, { directory });
}
