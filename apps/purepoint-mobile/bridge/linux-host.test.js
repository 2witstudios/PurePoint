import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, lstat, mkdir, symlink, writeFile } from "node:fs/promises";
import { X509Certificate, createPrivateKey, createPublicKey } from "node:crypto";
import http from "node:http";
import path from "node:path";
import os from "node:os";
import { mintHostIdentity } from "./identity.js";
import { platformDefaults, hostLabel } from "./platform.js";
import {
  discoverTailnet,
  interfaceCandidates,
  servePassthrough,
} from "./tailnet.js";
import { startRemote } from "./remote.js";
import { serveAdmin } from "./admin.js";

const iface = (address) => ({ address, family: "IPv4", internal: false });

test("minted identity is a valid self-signed P-256 certificate matching its key", () => {
  const { cert, key } = mintHostIdentity();
  const x509 = new X509Certificate(cert);
  assert.equal(x509.subject, "CN=Point Guard");
  assert.equal(x509.issuer, "CN=Point Guard");
  assert.ok(x509.verify(x509.publicKey));
  assert.ok(x509.checkIssued(x509));
  assert.equal(x509.publicKey.asymmetricKeyDetails.namedCurve, "prime256v1");
  const derived = createPublicKey(createPrivateKey(key)).export({ type: "spki", format: "der" });
  assert.ok(derived.equals(x509.publicKey.export({ type: "spki", format: "der" })));
  const days = (Date.parse(x509.validTo) - Date.parse(x509.validFrom)) / 86400000;
  assert.ok(days > 3649 && days < 3651);
  assert.ok(Date.parse(x509.validFrom) <= Date.now());
});

test("minted identities never repeat keys or serials", () => {
  const a = new X509Certificate(mintHostIdentity().cert);
  const b = new X509Certificate(mintHostIdentity().cert);
  assert.notEqual(a.serialNumber, b.serialNumber);
  assert.notEqual(a.fingerprint256, b.fingerprint256);
});

test("given validity past 2049, should encode GeneralizedTime that parses", () => {
  const { cert } = mintHostIdentity({ now: new Date("2045-01-01T00:00:00Z") });
  assert.equal(new Date(new X509Certificate(cert).validTo).getUTCFullYear(), 2054);
});

test("platform defaults: XDG state on Linux, Application Support on the Mac", () => {
  const linux = platformDefaults({ platform: "linux", env: {}, home: "/home/u" });
  assert.equal(linux.stateDirectory, "/home/u/.local/state/purepoint/point-guard");
  assert.equal(linux.adminTransport, "unix");
  assert.equal(linux.hostKind, "linux");
  assert.equal(
    platformDefaults({ platform: "linux", env: { XDG_STATE_HOME: "/s" }, home: "/home/u" }).stateDirectory,
    "/s/purepoint/point-guard",
  );
  // A relative XDG value is ignored, never resolved against cwd.
  assert.equal(
    platformDefaults({ platform: "linux", env: { XDG_STATE_HOME: "s" }, home: "/home/u" }).stateDirectory,
    "/home/u/.local/state/purepoint/point-guard",
  );
  const mac = platformDefaults({ platform: "darwin", env: {}, home: "/Users/u" });
  assert.equal(mac.stateDirectory, "/Users/u/Library/Application Support/PurePoint/PointGuard");
  assert.equal(mac.adminTransport, "tcp");
  assert.equal(mac.discoverTailnet, false);
});

test("host label is the first DNS label with only conservative characters", () => {
  assert.equal(hostLabel("build-box.tail1234.ts.net", "linux"), "build-box");
  assert.equal(hostLabel("<script>", "linux"), "script");
  assert.equal(hostLabel("…", "linux"), "linux-host");
});

test("interface candidates include only live tailnet IPv4 addresses", () => {
  const found = interfaceCandidates({
    lo: [{ address: "127.0.0.1", family: "IPv4", internal: true }],
    eth0: [iface("10.0.0.2"), iface("100.63.0.1")],
    tailscale0: [iface("100.101.1.2"), { address: "fd7a:115c:a1e0::1", family: "IPv6", internal: false }],
  });
  assert.deepEqual([...found], ["100.101.1.2"]);
});

test("discovery binds only when Tailscale and the interfaces agree", async () => {
  const paths = [process.execPath];
  const run = (output) => async () => output;
  assert.deepEqual(
    await discoverTailnet({ paths: ["/nonexistent/tailscale"], interfaces: {}, run: run("") }),
    { mode: "unavailable", reason: "tailscale_missing" },
  );
  assert.equal((await discoverTailnet({ paths, interfaces: {}, run: run(null) })).reason, "tailscale_down");
  assert.equal(
    (await discoverTailnet({ paths, interfaces: {}, run: run("100.64.0.1\n100.64.0.2\n") })).reason,
    "tailscale_down",
  );
  assert.equal((await discoverTailnet({ paths, interfaces: {}, run: run("8.8.8.8\n") })).reason, "tailscale_down");
  const agree = await discoverTailnet({
    paths,
    interfaces: { tailscale0: [iface("100.101.1.2")] },
    run: run("100.101.1.2\n"),
  });
  assert.equal(agree.mode, "interface");
  assert.equal(agree.address, "100.101.1.2");
  assert.equal(
    (await discoverTailnet({ paths, interfaces: { tailscale0: [iface("100.101.1.3")] }, run: run("100.101.1.2\n") })).reason,
    "tailnet_mismatch",
  );
  const userspace = await discoverTailnet({ paths, interfaces: { eth0: [iface("10.0.0.2")] }, run: run("100.101.1.2\n") });
  assert.equal(userspace.mode, "serve");
});

test("serve passthrough requires TCP forwarding to exactly the loopback port without TLS termination", () => {
  const ok = JSON.stringify({ TCP: { 8787: { TCPForward: "127.0.0.1:8787" } } });
  assert.equal(servePassthrough(ok, 8787), true);
  assert.equal(servePassthrough(JSON.stringify({ TCP: { 8787: { TCPForward: "localhost:8787" } } }), 8787), true);
  assert.equal(servePassthrough(ok, 9000), false);
  assert.equal(servePassthrough(JSON.stringify({ TCP: { 8787: { TCPForward: "127.0.0.1:22" } } }), 8787), false);
  assert.equal(servePassthrough(JSON.stringify({ TCP: { 8787: { TCPForward: "10.0.0.5:8787" } } }), 8787), false);
  assert.equal(
    servePassthrough(JSON.stringify({ TCP: { 8787: { TCPForward: "127.0.0.1:8787", TerminateTLS: "host.ts.net" } } }), 8787),
    false,
  );
  assert.equal(servePassthrough(JSON.stringify({ TCP: { 8787: { HTTPS: true } } }), 8787), false);
  assert.equal(servePassthrough("not json", 8787), false);
});

function fakeTimers() {
  const pending = [];
  return {
    setTimer: (fn, ms) => {
      const t = { fn, ms };
      pending.push(t);
      return t;
    },
    clearTimer: (t) => {
      const i = pending.indexOf(t);
      if (i >= 0) pending.splice(i, 1);
    },
    next: () => pending.shift(),
    pending,
  };
}

test("remote manager waits for Tailscale, late-binds, and rebinds on address change", async () => {
  const timers = fakeTimers();
  const answers = [
    { mode: "unavailable", reason: "tailscale_down" },
    { mode: "interface", address: "100.64.0.9", binary: "/t" },
    { mode: "interface", address: "100.64.0.9", binary: "/t" },
    { mode: "interface", address: "100.64.0.10", binary: "/t" },
  ];
  const binds = [];
  const closed = [];
  const remote = await startRemote({
    port: 8787,
    peerAllowed: (mode) => mode,
    discover: async () => answers.shift(),
    listen: async (host, peer) => {
      binds.push([host, peer]);
      return { shutdown: async () => closed.push(host) };
    },
    ...timers,
  });
  assert.equal(remote.chatURL(), null);
  assert.equal(remote.status().code, "tailscale_down");
  assert.ok(remote.status().recovery.includes("tailscale up"));
  assert.equal(timers.pending[0].ms, 5000);
  await timers.next().fn();
  assert.equal(remote.chatURL(), "wss://100.64.0.9:8787/v1");
  assert.deepEqual(binds, [["100.64.0.9", "interface"]]);
  assert.equal(timers.pending[0].ms, 60000);
  await timers.next().fn();
  assert.equal(binds.length, 1, "same address does not rebind");
  await timers.next().fn();
  assert.deepEqual(closed, ["100.64.0.9"]);
  assert.equal(remote.chatURL(), "wss://100.64.0.10:8787/v1");
  await remote.shutdown();
  assert.deepEqual(closed, ["100.64.0.9", "100.64.0.10"]);
  assert.equal(timers.pending.length, 0);
});

test("remote manager in userspace mode advertises only after serve passthrough is verified", async () => {
  const timers = fakeTimers();
  let configured = false;
  const binds = [];
  const remote = await startRemote({
    port: 8787,
    peerAllowed: (mode) => mode,
    discover: async () => ({ mode: "serve", address: "100.64.0.9", binary: "/t" }),
    serveCheck: async () => configured,
    listen: async (host, peer) => {
      binds.push([host, peer]);
      return { shutdown: async () => {} };
    },
    ...timers,
  });
  assert.deepEqual(binds, [["127.0.0.1", "serve"]]);
  assert.equal(remote.chatURL(), null);
  assert.equal(remote.status().code, "serve_required");
  assert.ok(remote.status().commands[0].startsWith("tailscale serve --bg --tcp 8787 tcp://127.0.0.1:8787"));
  configured = true;
  await timers.next().fn();
  assert.equal(remote.chatURL(), "wss://100.64.0.9:8787/v1");
  assert.equal(binds.length, 1);
  await remote.shutdown();
});

test("remote manager closes the listener when Tailscale disappears and backs off to 60s", async () => {
  const timers = fakeTimers();
  const answers = [{ mode: "interface", address: "100.64.0.9", binary: "/t" }];
  let closed = 0;
  const remote = await startRemote({
    port: 8787,
    peerAllowed: () => () => true,
    discover: async () => answers.shift() ?? { mode: "unavailable", reason: "tailscale_down" },
    listen: async () => ({ shutdown: async () => closed++ }),
    ...timers,
  });
  await timers.next().fn();
  assert.equal(closed, 1);
  assert.equal(remote.chatURL(), null);
  const delays = [];
  for (let i = 0; i < 6; i++) {
    delays.push(timers.pending[0].ms);
    await timers.next().fn();
  }
  assert.deepEqual(delays, [5000, 10000, 20000, 40000, 60000, 60000]);
  await remote.shutdown();
});

test("remote manager reports bind failure without throwing", async () => {
  const timers = fakeTimers();
  const remote = await startRemote({
    port: 8787,
    peerAllowed: () => () => true,
    discover: async () => ({ mode: "interface", address: "100.64.0.9" }),
    listen: async () => {
      throw Object.assign(new Error("in use"), { code: "EADDRINUSE" });
    },
    ...timers,
  });
  assert.equal(remote.status().code, "bind_failed");
  assert.equal(remote.chatURL(), null);
  await remote.shutdown();
});

function request(socketPath, { token, body = { operation: "status" }, headers = {} }) {
  return new Promise((resolve, reject) => {
    const data = JSON.stringify(body);
    const req = http.request(
      {
        socketPath,
        path: "/admin/v1",
        method: "POST",
        headers: {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(data),
          ...(token ? { authorization: `Bearer ${token}` } : {}),
          ...headers,
        },
      },
      (res) => {
        let text = "";
        res.on("data", (c) => (text += c));
        res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(text) }));
      },
    );
    req.on("error", reject);
    req.end(data);
  });
}

test("owner admin over a Unix socket: private dir, 0600 socket under a permissive umask, token still required", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "pg-admin-"));
  const socketPath = path.join(root, "admin", "admin.sock");
  const previous = process.umask(0o000);
  let admin;
  try {
    // A stale admin dir (even with foreign content) is replaced, never reused.
    await mkdir(path.join(root, "admin"), { mode: 0o777 });
    await writeFile(path.join(root, "admin", "planted"), "x");
    const token = "t".repeat(43);
    admin = await serveAdmin({ token, dispatch: async () => ({ ok: 1 }), socketPath });
    process.umask(previous);
    assert.equal((await lstat(path.join(root, "admin"))).mode & 0o777, 0o700);
    assert.equal((await lstat(socketPath)).mode & 0o777, 0o600);
    await assert.rejects(lstat(path.join(root, "admin", "planted")));
    assert.equal(admin.socket, socketPath);
    assert.equal(admin.url, undefined);
    assert.equal((await request(socketPath, {})).status, 401);
    assert.equal((await request(socketPath, { token: "x".repeat(43) })).status, 401);
    assert.equal((await request(socketPath, { token, headers: { origin: "http://e" } })).status, 403);
    const ok = await request(socketPath, { token });
    assert.equal(ok.status, 200);
    assert.deepEqual(ok.body.result, { ok: 1 });
    await admin.shutdown();
    admin = null;
    await assert.rejects(lstat(path.join(root, "admin")));
  } finally {
    process.umask(previous);
    await admin?.shutdown();
    await rm(root, { recursive: true, force: true });
  }
});

test("owner admin refuses a symlinked admin directory target", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "pg-admin-"));
  const elsewhere = await mkdtemp(path.join(os.tmpdir(), "pg-elsewhere-"));
  try {
    // rm removes the symlink itself (not the target), then a fresh dir is made.
    await symlink(elsewhere, path.join(root, "admin"));
    const admin = await serveAdmin({
      token: "t".repeat(43),
      dispatch: async () => ({}),
      socketPath: path.join(root, "admin", "admin.sock"),
    });
    const info = await lstat(path.join(root, "admin"));
    assert.ok(info.isDirectory() && !info.isSymbolicLink());
    await lstat(elsewhere);
    await admin.shutdown();
  } finally {
    await rm(root, { recursive: true, force: true });
    await rm(elsewhere, { recursive: true, force: true });
  }
});

test("owner admin socket path must be absolute and fit sun_path", async () => {
  await assert.rejects(
    serveAdmin({ token: "t".repeat(43), dispatch: async () => ({}), socketPath: "relative/admin.sock" }),
    /absolute and short/,
  );
  await assert.rejects(
    serveAdmin({ token: "t".repeat(43), dispatch: async () => ({}), socketPath: "/" + "a".repeat(120) + "/admin.sock" }),
    /absolute and short/,
  );
});
