import os from "node:os";
import path from "node:path";

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
  return label || (hostKind === "linux" ? "linux-host" : "mac");
}

/** Host-specific recovery copy for owner-facing errors. */
export function hostCopy(hostKind) {
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
