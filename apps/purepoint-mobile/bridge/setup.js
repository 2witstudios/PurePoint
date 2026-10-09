import {
  readdir,
  readFile,
  stat,
  access,
  mkdir,
  writeFile,
  link,
  rm,
} from "node:fs/promises";
import { randomBytes, randomUUID } from "node:crypto";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";
import { Projection, boundedRows } from "./core.js";
export const root = fileURLToPath(new URL("../", import.meta.url));
export const defaultTokenFile = path.join(
  os.homedir(),
  ".config/pi-mobile/pairing-secret",
);
export async function ensureToken(file = defaultTokenFile) {
  try {
    return await loadToken(file);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = file + "." + randomUUID() + ".tmp";
  try {
    await writeFile(temporary, randomBytes(32).toString("base64url") + "\n", {
      mode: 0o600,
      flag: "wx",
    });
    // Publish only the fully written file; never replace another start's credential.
    try {
      await link(temporary, file);
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
    }
  } finally {
    await rm(temporary, { force: true });
  }
  return loadToken(file);
}
export const cli = path.join(
  root,
  "node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js",
);
export async function discoverPuSkills(home = os.homedir()) {
  const found = [];
  const roots = [
    path.join(home, ".agents/skills"),
    path.join(home, ".codex/skills"),
    path.join(home, ".codex/plugins/cache/purepoint"),
  ];
  const visited = new Set();
  let scanned = 0;
  async function walk(dir, depth) {
    if (depth > 8 || scanned++ > 4000 || visited.has(dir)) return;
    visited.add(dir);
    let children;
    try {
      children = await readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const child of children) {
      const full = path.join(dir, child.name);
      if (
        child.name === "SKILL.md" &&
        ["pu", "pu-cli"].includes(path.basename(dir))
      ) {
        found.push(full);
        continue;
      }
      if (child.isDirectory() || child.isSymbolicLink())
        await walk(full, depth + 1);
    }
  }
  for (const dir of roots) await walk(dir, 0);
  const unique = [...new Set(found)];
  return ["pu", "pu-cli"]
    .map(
      (name) =>
        unique
          .filter((x) => path.basename(path.dirname(x)) === name)
          .sort((a, b) => {
            const priority = (x) =>
              x.includes("/.agents/")
                ? 0
                : x.includes("/.codex/skills/")
                  ? 1
                  : 2;
            return (
              priority(a) - priority(b) ||
              b.localeCompare(a, undefined, { numeric: true })
            );
          })[0],
    )
    .filter(Boolean);
}
export async function launchArguments(cwd, env) {
  await access(cli);
  if (!(await stat(cwd)).isDirectory())
    throw new Error("PI_MOBILE_CWD must be an existing directory.");
  const skills = env.PI_MOBILE_PU_SKILL
    ? [path.resolve(env.PI_MOBILE_PU_SKILL)]
    : await discoverPuSkills();
  if (!skills.length)
    throw new Error(
      "No installed pu skill/reference found. Install Point Guard pu skills or set PI_MOBILE_PU_SKILL to its SKILL.md.",
    );
  for (const skill of skills) await access(skill);
  const args = [
    cli,
    "--mode",
    "rpc",
    ...skills.flatMap((s) => ["--skill", s]),
    "--append-system-prompt",
    path.join(root, "docs/point-guard.md"),
  ];
  if (env.PI_MOBILE_SESSION) args.push("--session", env.PI_MOBILE_SESSION);
  return args;
}
export async function nativeSessions() {
  const { SessionManager } = await import("@earendil-works/pi-coding-agent");
  let catalog = [];
  async function list() {
    catalog = (await SessionManager.listAll()).slice(0, 1000);
    return catalog;
  }
  async function session(id) {
    let info = catalog.find((s) => s.id === id);
    if (!info) {
      await list();
      info = catalog.find((s) => s.id === id);
    }
    if (!info)
      throw new Error(
        "Conversation no longer exists. Refresh the conversation list.",
      );
    return info;
  }
  return {
    list,
    path: async (id) => (await session(id)).path,
    history: async (id) => {
      const info = await session(id);
      if ((await stat(info.path)).size > 16 * 1024 * 1024)
        throw new Error(
          "This conversation is too large for mobile history. Resume it or inspect it in local Pi.",
        );
      const records = (await readFile(info.path, "utf8"))
        .split("\n")
        .filter((x) => x.trim())
        .map((x) => JSON.parse(x));
      // inMemory applies native migrations/leaf semantics without changing the on-disk file.
      const manager = SessionManager.inMemory(info.cwd, undefined, records);
      const p = new Projection();
      p.load(manager.getEntries(), manager.getLeafId());
      return {
        sessionId: info.id,
        title: info.name || info.firstMessage || "Conversation",
        messages: boundedRows(p.messages, 1024 * 1024),
      };
    },
  };
}
export async function loadToken(file) {
  if (!file)
    throw new Error(
      "Set PI_MOBILE_TOKEN_FILE to an owner-created secret file (at least 32 characters, chmod 600).",
    );
  const info = await stat(file);
  if ((info.mode & 0o077) !== 0 || info.size > 4096)
    throw new Error(
      "Pairing secret must be a private file (chmod 600), at most 4 KiB.",
    );
  const token = (await readFile(file, "utf8")).trim();
  if (token.length < 32)
    throw new Error("Pairing secret must contain at least 32 characters.");
  return token;
}
