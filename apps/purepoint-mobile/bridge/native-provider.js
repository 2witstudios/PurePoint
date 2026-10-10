import os from "node:os";
import path from "node:path";
import { readFile } from "node:fs/promises";
import { AuthStorage } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
import { ModelRuntime } from "@earendil-works/pi-coding-agent";
import { privateDirectory, privatePath } from "./runtime-state.js";
import { ProviderSetup } from "./provider.js";
/** Preserve canonical native Pi credentials, rejecting insecure/corrupt existing files. */
export async function nativeProviderSetup(deviceId) {
  const dir =
    process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), ".pi/agent");
  await privateDirectory(dir);
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
