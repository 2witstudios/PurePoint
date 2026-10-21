import os from "node:os";
import { access } from "node:fs/promises";
import { constants } from "node:fs";
import { execFile } from "node:child_process";
import { isIP } from "node:net";

/** Fixed absolute locations only; PATH is never searched. */
export const TAILSCALE_PATHS = [
  "/usr/bin/tailscale",
  "/usr/sbin/tailscale",
  "/usr/local/bin/tailscale",
  "/snap/bin/tailscale",
];

export function tailnetIPv4(address) {
  if (isIP(address) !== 4) return false;
  const [a, b] = address.split(".").map(Number);
  return a === 100 && b >= 64 && b <= 127;
}

/** Tailnet IPv4 addresses held by live, non-loopback interfaces. */
export function interfaceCandidates(interfaces = os.networkInterfaces()) {
  const found = new Set();
  for (const entries of Object.values(interfaces))
    for (const entry of entries ?? [])
      if (
        !entry.internal &&
        String(entry.family).replace("IPv", "") === "4" &&
        tailnetIPv4(entry.address)
      )
        found.add(entry.address);
  return found;
}

async function executable(paths) {
  for (const candidate of paths) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return null;
}

/** Bounded, read-only tailscale query with a scrubbed environment. */
export function runTailscale(binary, args, { timeout = 3000 } = {}) {
  return new Promise((resolve) => {
    execFile(
      binary,
      args,
      {
        timeout,
        maxBuffer: 65536,
        env: { PATH: "/usr/bin:/bin", HOME: os.homedir(), TERM: "dumb" },
        windowsHide: true,
      },
      (error, stdout) => resolve(error ? null : String(stdout)),
    );
  });
}

/**
 * Decide how the phone reaches this host.
 * - interface: an interface holds the address `tailscale ip -4` reports; bind it.
 * - serve: userspace networking (no interface holds it); bind loopback and
 *   require a `tailscale serve` TCP passthrough to exactly that port.
 * - unavailable: anything ambiguous; never guess.
 */
export async function discoverTailnet({
  interfaces = os.networkInterfaces(),
  paths = TAILSCALE_PATHS,
  run = runTailscale,
} = {}) {
  const binary = await executable(paths);
  if (!binary) return { mode: "unavailable", reason: "tailscale_missing" };
  const output = await run(binary, ["ip", "-4"]);
  const values = (output ?? "").split(/\s+/).filter(Boolean);
  if (values.length !== 1 || !tailnetIPv4(values[0]))
    return { mode: "unavailable", reason: "tailscale_down" };
  const reported = values[0];
  const candidates = interfaceCandidates(interfaces);
  if (candidates.has(reported))
    return { mode: "interface", address: reported, binary };
  if (candidates.size === 0)
    return { mode: "serve", address: reported, binary };
  return { mode: "unavailable", reason: "tailnet_mismatch" };
}

/**
 * True only when `tailscale serve` forwards tailnet TCP `port` to exactly
 * loopback `port` without terminating TLS, so the phone still sees the
 * pinned bridge certificate end to end.
 */
export function servePassthrough(statusJSON, port) {
  let status;
  try {
    status = JSON.parse(statusJSON);
  } catch {
    return false;
  }
  const handler = status?.TCP?.[String(port)];
  return (
    !!handler &&
    typeof handler === "object" &&
    [`127.0.0.1:${port}`, `localhost:${port}`].includes(handler.TCPForward) &&
    !handler.TerminateTLS &&
    !handler.HTTPS &&
    !handler.HTTP
  );
}

export async function serveConfigured(binary, port, run = runTailscale) {
  const output = await run(binary, ["serve", "status", "--json"]);
  return output !== null && servePassthrough(output, port);
}

export function serveCommands(port) {
  return [
    `tailscale serve --bg --tcp ${port} tcp://127.0.0.1:${port}`,
    "(if that is refused: sudo tailscale set --operator=$USER)",
  ];
}
