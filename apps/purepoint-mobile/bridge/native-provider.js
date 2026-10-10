import os from "node:os";
import path from "node:path";
import { readFile, mkdir, lstat } from "node:fs/promises";
import { AuthStorage } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
import { ModelRuntime } from "@earendil-works/pi-coding-agent";
import { privatePath } from "./runtime-state.js";
import { ProviderSetup } from "./provider.js";
// Pi may create this directory with the process umask. Read/traverse access
// does not expose private auth.json; write access could replace credentials.
async function nativeDirectory(dir) {
  try {
    await mkdir(dir, { recursive: true, mode: 0o700 });
    const s = await lstat(dir);
    if (
      s.isSymbolicLink() ||
      !s.isDirectory() ||
      (s.mode & 0o022) !== 0 ||
      (typeof process.getuid === "function" && s.uid !== process.getuid())
    )
      throw new Error("Unsafe native directory.");
  } catch {
    throw Object.assign(new Error("Native Pi directory is not safe to use."), {
      code: "native_directory_unsafe",
    });
  }
}
/** Preserve canonical native Pi credentials, rejecting insecure/corrupt existing files. */
export async function nativeProviderSetup(deviceId) {
  const dir =
    process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), ".pi/agent");
  await nativeDirectory(dir);
  const authPath = path.join(dir, "auth.json");
  try {
    const s = await privatePath(authPath);
    if (s.size > 1024 * 1024) throw new Error("Oversized native auth state.");
    const data = JSON.parse(await readFile(authPath, "utf8"));
    if (!data || Array.isArray(data) || typeof data !== "object")
      throw new Error("Corrupt native auth state.");
  } catch (e) {
    if (e.code !== "ENOENT")
      throw new Error(
        "Native Pi credentials are corrupt or not private. Restore permissions or a known backup before setup.",
      );
  }
  const credentials = AuthStorage.create(authPath);
  const runtime = await ModelRuntime.create({
    credentials,
    refreshOnCreate: false,
    allowModelNetwork: false,
  });
  return new ProviderSetup({
    credentials,
    providers: runtime.getProviders(),
    models: runtime.getModels(),
    deviceId: () => deviceId,
  });
}
