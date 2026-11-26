import os from "node:os";
import path from "node:path";
import { pipeNameForSocket } from "./pipe-name.js";

/** Host differences live here; the rest of the bridge is platform-neutral. */
export function platformDefaults({
  platform = process.platform,
  env = process.env,
  home = os.homedir(),
} = {}) {
  if (platform === "linux") {
    const state =
      env.XDG_STATE_HOME && path.isAbsolute(env.XDG_STATE_HOME)
        ? env.XDG_STATE_HOME
        : path.join(home, ".local/state");
    return {
      hostKind: "linux",
      stateDirectory: path.join(state, "purepoint/point-guard"),
      adminTransport: "unix",
      discoverTailnet: true,
    };
  }
  if (platform === "win32") {
    const local =
      env.LOCALAPPDATA && path.win32.isAbsolute(env.LOCALAPPDATA)
        ? env.LOCALAPPDATA
        : path.win32.join(home, "AppData", "Local");
    return {
      hostKind: "windows",
      // Made private with a protected owner-only ACL (acl.js), not a mode.
      stateDirectory: path.win32.join(local, "PurePoint", "point-guard"),
      // Token-guarded named pipe; see ownerPipeName.
      adminTransport: "pipe",
      // Like the Mac, the app discovers the address and passes PI_MOBILE_HOST.
      discoverTailnet: false,
    };
  }
  return {
    hostKind: "mac",
    stateDirectory: path.join(
      home,
      "Library/Application Support/PurePoint/PointGuard",
    ),
    adminTransport: "tcp",
    // The Mac app discovers the address and passes PI_MOBILE_HOST.
    discoverTailnet: false,
  };
}

/** Short, display-only host label: first DNS label, conservative characters. */
export function hostLabel(hostname = os.hostname(), hostKind = "mac") {
  const label = String(hostname)
    .split(".")[0]
    .replace(/[^A-Za-z0-9-]/g, "")
    .slice(0, 63);
  return (
    label ||
    (hostKind === "linux"
      ? "linux-host"
      : hostKind === "windows"
        ? "windows-pc"
        : "mac")
  );
}

/** Host-specific recovery copy for owner-facing errors. */
export function hostCopy(hostKind) {
  if (hostKind === "windows")
    return {
      host: "this PC",
      unreachable:
        "Connect Tailscale and select this PC’s tailnet address before pairing.",
      reinstall: "Reinstall the complete PurePoint app.",
    };
  return hostKind === "linux"
    ? {
        host: "this host",
        unreachable:
          "No tailnet address. Connect Tailscale (`tailscale up`), then run `pu phone status`.",
        reinstall: "Reinstall phone access with `pu phone install`.",
      }
    : {
        host: "this Mac",
        unreachable:
          "Connect Tailscale and select this Mac’s tailnet address before pairing.",
        reinstall: "Reinstall the complete PurePoint app.",
      };
}

/**
 * Windows owner channel: `\\.\pipe\purepoint-<hash16>-pointguard-owner`,
 * hashed (WINAPP-003) over `<state>\admin.sock` so each state directory gets
 * its own pipe. Pipe names are global, so the bearer token stays required.
 */
export function ownerPipeName(stateDirectory) {
  return pipeNameForSocket(
    path.win32.join(stateDirectory, "admin.sock"),
    "pointguard-owner",
  );
}
