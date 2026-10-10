import { readFile } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { writePairingPage, pairingPagePath } from "./pairing.js";
import { privateEntry } from "./trust-tls.js";
async function main() {
  const directory =
    process.env.POINT_GUARD_STATE_DIR ??
    path.join(os.homedir(), "Library/Application Support/PurePoint/PointGuard");
  await privateEntry(directory, true);
  const descriptorFile = path.join(directory, "admin.json");
  const tokenFile = path.join(directory, "admin-token");
  await privateEntry(descriptorFile);
  await privateEntry(tokenFile);
  const descriptor = JSON.parse(await readFile(descriptorFile, "utf8"));
  const url = new URL(descriptor.adminURL);
  if (
    url.protocol !== "http:" ||
    url.hostname !== "127.0.0.1" ||
    url.pathname !== "/admin/v1" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  )
    throw new Error("Invalid local administration endpoint.");
  const response = await fetch(url, {
    method: "POST",
    redirect: "error",
    signal: AbortSignal.timeout(10000),
    headers: {
      Authorization: `Bearer ${(await readFile(tokenFile, "utf8")).trim()}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ operation: "pairing.create" }),
  });
  const data = await response.json();
  if (!response.ok || !data.ok || typeof data.result?.payload !== "string")
    throw new Error("Enrollment unavailable.");
  await writePairingPage(pairingPagePath, data.result.payload);
  console.log(
    `Private short-lived enrollment QR saved to ${pairingPagePath}. Scan once; create a new code if expired.`,
  );
}
main().catch(() => {
  console.error(
    "Could not create enrollment QR. Open PurePoint, connect Tailscale, and use Connect phone. Legacy shared-token QR is no longer supported.",
  );
  process.exitCode = 1;
});
