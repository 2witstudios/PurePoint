import { discoverTailnet, serveConfigured, serveCommands } from "./tailnet.js";

const RETRY_MIN = 5000;
const RETRY_MAX = 60000;
const RECHECK = 60000;
const unavailable = {
  tailscale_missing: [
    "Tailscale is not installed at a standard location.",
    "Install Tailscale and run `tailscale up`; phone access starts automatically.",
  ],
  tailscale_down: [
    "Tailscale is not connected.",
    "Run `tailscale up`; phone access starts automatically.",
  ],
  tailnet_mismatch: [
    "Tailscale and this host's interfaces disagree about the tailnet address.",
    "Check `tailscale ip -4` and the interface addresses; phone access waits until they agree.",
  ],
  bind_failed: [
    "The phone listener could not bind its tailnet address.",
    "Check that nothing else uses the phone port; phone access retries automatically.",
  ],
};

/**
 * Headless remote listener lifecycle. Discovers the tailnet, binds only an
 * address both Tailscale and the interfaces agree on (or loopback behind a
 * verified `tailscale serve` passthrough), retries with backoff and rebinds
 * when the address changes. Local admin and chat never depend on it.
 */
export async function startRemote({
  listen,
  port,
  peerAllowed,
  discover = discoverTailnet,
  serveCheck = serveConfigured,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
}) {
  let current = null; // { mode, host, server }
  let advertised = null;
  let status = { state: "starting" };
  let stopped = false;
  let timer = null;
  let delay = RETRY_MIN / 2;
  let running = Promise.resolve();

  const close = async () => {
    const server = current?.server;
    current = null;
    advertised = null;
    await server?.shutdown();
  };
  const waiting = (code) => {
    const [message, recovery] = unavailable[code];
    status = { state: "unavailable", code, message, recovery };
  };
  const bind = async (mode, host) => {
    if (current?.mode === mode && current.host === host) return true;
    await close();
    try {
      current = { mode, host, server: await listen(host, peerAllowed(mode)) };
      return true;
    } catch {
      waiting("bind_failed");
      return false;
    }
  };
  const tick = async () => {
    const found = await discover();
    if (stopped) return false;
    if (found.mode === "unavailable") {
      await close();
      waiting(found.reason);
      return false;
    }
    if (found.mode === "interface") {
      if (!(await bind("interface", found.address))) return false;
      advertised = `wss://${found.address}:${port}/v1`;
      status = { state: "listening", mode: "interface", address: found.address };
      return true;
    }
    // Userspace networking: loopback listener, reachable only through serve.
    if (!(await bind("serve", "127.0.0.1"))) return false;
    if (!(await serveCheck(found.binary, port))) {
      advertised = null;
      status = {
        state: "unavailable",
        code: "serve_required",
        mode: "serve",
        message: `Tailscale runs in userspace mode; tailnet port ${port} is not forwarded to Point Guard.`,
        recovery: `Run: ${serveCommands(port).join(" ")}`,
        commands: serveCommands(port),
      };
      return false;
    }
    advertised = `wss://${found.address}:${port}/v1`;
    status = { state: "listening", mode: "serve", address: found.address };
    return true;
  };
  const schedule = (ready) => {
    if (stopped) return;
    // After a healthy period the next failure retries at RETRY_MIN again.
    delay = ready ? RETRY_MIN / 2 : Math.min(delay * 2, RETRY_MAX);
    timer = setTimer(run, ready ? RECHECK : delay);
    timer?.unref?.();
  };
  const run = () => {
    running = running
      .then(tick)
      .catch(() => false)
      .then((ready) => schedule(ready));
    return running;
  };
  await run();
  return {
    chatURL: () => advertised,
    status: () => status,
    refresh: () => {
      clearTimer(timer);
      return run();
    },
    shutdown: async () => {
      stopped = true;
      clearTimer(timer);
      await running.catch(() => {});
      await close();
    },
  };
}
