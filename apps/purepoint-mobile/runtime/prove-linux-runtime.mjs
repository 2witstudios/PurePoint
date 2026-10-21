// Clean-environment proof of a packaged Linux bridge payload (`--layout dir`).
//
//   node prove-linux-runtime.mjs <payload-dir> [<pu>] [--in-place]
//
// Copies the payload out of the checkout (unless --in-place, for a payload
// install.sh already put in ~/.pu/pointguard), then launches it `--managed`
// with an empty HOME and a PATH holding no tools, the way `pu phone` does.
// Proves: manifest and payload-relative paths, private state/admin modes, the
// Unix-socket owner admin, single-use enrollment over the pinned TLS listener,
// per-message grants over the pinned WebSocket, owner token separation,
// revocation with another device still connected, the state lock refusing a second
// bridge, and identity/trust surviving a restart. With a Linux `pu`, it also
// drives `pu phone status/devices/grant/revoke/pair` against the running bridge.
import assert from "node:assert/strict";
import { readFile, mkdtemp, mkdir, rm, cp, stat, lstat, readdir } from "node:fs/promises";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import os from "node:os";
import { randomUUID, createHash } from "node:crypto";
import http from "node:http";
import https from "node:https";
import net from "node:net";
import { createRequire } from "node:module";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const args = process.argv.slice(2);
const inPlace = args.includes("--in-place");
const [sourceArg, puArg] = args.filter((a) => a !== "--in-place");
assert.ok(sourceArg, "usage: prove-linux-runtime.mjs <payload-dir> [<pu>] [--in-place]");
assert.equal(process.platform, "linux", "The Linux payload proof runs on Linux");
const source = path.resolve(sourceArg);
const pu = puArg ? path.resolve(puArg) : null;
// Short base: the admin socket path must stay under the 100-byte limit.
const folder = await mkdtemp(path.join("/tmp", "pg-linux-"));
let child;
try {
  const base = inPlace ? source : path.join(folder, "payload");
  if (!inPlace) await cp(source, base, { recursive: true, verbatimSymlinks: true });
  const manifest = JSON.parse(await readFile(path.join(base, "runtime-manifest.json"), "utf8"));
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.contractVersion, 1);
  assert.equal(manifest.layout, "linux");
  assert.match(manifest.puVersion, /^\d+\.\d+\.\d+$/);
  assert.equal(manifest.piVersion, "1.1.0");
  assert.equal(manifest.libc, "glibc");
  assert.equal(manifest.architecture, { x64: "x64", arm64: "arm64" }[process.arch]);
  assert.equal(manifest.paths.node, "bin/node");
  assert.equal(manifest.paths.lockHelper, "bin/point-guard-lock");
  assert.equal(manifest.paths.pu, undefined, "Linux payloads run the installed pu");
  assert.ok(!JSON.stringify(manifest).includes(".."), "Payload-relative paths only");
  const node = path.join(base, manifest.paths.node);
  const entry = path.join(base, manifest.paths.entry);
  const lockHelper = path.join(base, manifest.paths.lockHelper);
  for (const file of [
    node,
    entry,
    lockHelper,
    path.join(base, manifest.paths.instructions),
    ...manifest.paths.skills.map((s) => path.join(base, s)),
  ]) {
    const info = await lstat(file);
    assert.ok(info.isFile() && !info.isSymbolicLink(), `${file} must be a regular file`);
    assert.equal(info.mode & 0o022, 0, `${file} must not be group/other-writable`);
  }
  // Every entry `pu phone` might execute or load is a plain, owner-writable file.
  const walk = async (dir) => {
    for (const item of await readdir(dir, { withFileTypes: true })) {
      const full = path.join(dir, item.name);
      assert.ok(!item.isSymbolicLink(), `symlink in payload: ${full}`);
      assert.equal((await lstat(full)).mode & 0o022, 0, `writable by others: ${full}`);
      if (item.isDirectory()) await walk(full);
    }
  };
  await walk(base);
  const version = execFileSync(node, ["--version"], { env: {}, encoding: "utf8" }).trim();
  assert.equal(version, `v${manifest.nodeVersion}`);

  const home = path.join(folder, "home");
  await mkdir(home, { mode: 0o700 });
  // Where `pu phone` puts it: ${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard.
  const stateDir = path.join(home, ".local/state/purepoint/point-guard");
  const descriptorFile = path.join(stateDir, "admin.json");
  const free = net.createServer();
  await new Promise((resolve) => free.listen(0, "127.0.0.1", resolve));
  const port = free.address().port;
  await new Promise((resolve) => free.close(resolve));
  // The bridge only checks the pu path is executable; without a Linux pu, the
  // lock helper stands in so the payload alone can be proven.
  const puPath = pu ?? lockHelper;
  const environment = (remote) => ({
    HOME: home,
    PATH: "/no-external-tools",
    POINT_GUARD_STATE_DIR: stateDir,
    POINT_GUARD_PU_PATH: puPath,
    POINT_GUARD_PU_HOME: path.join(home, ".pu"),
    POINT_GUARD_LOCK_HELPER_PATH: lockHelper,
    POINT_GUARD_INSTANCE_ID: randomUUID(),
    PI_SKIP_VERSION_CHECK: "1",
    PI_MOBILE_PORT: String(port),
    // No tailnet in CI: an explicit loopback listener stands in for the
    // discovered tailnet address; discovery itself is unit-tested.
    ...(remote ? { PI_MOBILE_HOST: "127.0.0.1" } : { POINT_GUARD_PHONE: "off" }),
  });
  const puCli = (...argv) =>
    execFileSync(pu, ["phone", ...argv], {
      env: { HOME: home, PATH: "/usr/bin:/bin" },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });

  const launch = async (remote) => {
    const env = environment(remote);
    let output = "";
    child = spawn(node, [entry, "--managed"], { env, cwd: home, stdio: ["ignore", "pipe", "pipe"] });
    child.stdout.on("data", (b) => (output += b));
    child.stderr.on("data", (b) => (output += b));
    child.on("error", () => {});
    let ready;
    for (let i = 0; i < 300 && !ready; i++) {
      if (child.exitCode !== null) {
        let failure;
        try {
          failure = JSON.parse(await readFile(path.join(stateDir, "error.json"), "utf8"));
        } catch {}
        throw new Error(
          `Packaged bridge exited ${child.exitCode}: ${failure?.code ?? "no attributed failure"}; ${failure?.recovery ?? output}`,
        );
      }
      try {
        const d = JSON.parse(await readFile(descriptorFile, "utf8"));
        if (d.pid === child.pid && d.instanceId === env.POINT_GUARD_INSTANCE_ID) ready = d;
      } catch {}
      if (!ready) await pause(100);
    }
    assert.ok(ready, "Packaged bridge readiness missing");
    assert.equal(ready.phase, "ready");
    assert.equal(ready.hostKind, "linux");
    assert.equal(ready.adminURL, undefined, "Linux admin is a Unix socket, never TCP");
    assert.equal(ready.adminSocket, path.join(stateDir, "admin/admin.sock"));
    assert.equal((await stat(stateDir)).mode & 0o777, 0o700);
    assert.equal((await lstat(path.dirname(ready.adminSocket))).mode & 0o777, 0o700);
    assert.equal((await lstat(ready.adminSocket)).mode & 0o777, 0o600);
    assert.ok((await lstat(ready.adminSocket)).isSocket());
    for (const file of ["admin.json", "admin-token", "desktop-chat-token"])
      assert.equal((await stat(path.join(stateDir, file))).mode & 0o777, 0o600, file);
    const token = (await readFile(path.join(stateDir, "admin-token"), "utf8")).trim();
    const chatToken = (await readFile(path.join(stateDir, "desktop-chat-token"), "utf8")).trim();
    assert.notEqual(token, chatToken);
    assert.equal(output.includes(token), false, "Owner token must never be logged");
    const request = (bearer, body) =>
      new Promise((resolve, reject) => {
        const payload = JSON.stringify(body);
        const req = http.request(
          {
            socketPath: ready.adminSocket,
            path: "/admin/v1",
            method: "POST",
            headers: {
              authorization: `Bearer ${bearer}`,
              "content-type": "application/json",
              "content-length": Buffer.byteLength(payload),
            },
          },
          (res) => {
            let text = "";
            res.on("data", (b) => (text += b));
            res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(text) }));
          },
        );
        req.on("error", reject);
        req.end(payload);
      });
    const admin = async (operation, extra = {}) => {
      const response = await request(token, { operation, ...extra });
      assert.equal(response.body.ok, true, JSON.stringify(response.body));
      return response.body.result;
    };
    return { ready, admin, request, token, chatToken, output: () => output };
  };
  const stop = async (context) => {
    const owned = child;
    const exited = once(owned, "exit");
    await context.admin("runtime.stop");
    const [code] = await exited;
    assert.equal(code, 0, "An idle runtime.stop exits 0, so systemd does not restart it");
    child = null;
  };
  const enroll = (qr, name) =>
    new Promise((resolve, reject) => {
      const req = https.request(
        qr.endpoint.replace("wss:", "https:").replace("/v1", "/pair/enroll"),
        { method: "POST", rejectUnauthorized: false, headers: { "content-type": "application/json" } },
        (response) => {
          // The phone pins the certificate hash from the QR, not a CA.
          const cert = response.socket.getPeerCertificate().raw;
          assert.equal(createHash("sha256").update(cert).digest("hex"), qr.certificateSHA256);
          let body = "";
          response.on("data", (b) => (body += b));
          response.on("end", () => resolve({ status: response.statusCode, body }));
        },
      );
      req.on("error", reject);
      req.end(JSON.stringify({ version: 1, enrollmentToken: qr.enrollmentToken, name }));
    });

  // 1. Local only (no phone listener): ready, and pairing refuses.
  const first = await launch(false);
  assert.ok((await first.admin("providers")).providers.some((p) => p.id === "anthropic"));
  await assert.rejects(first.admin("pairing.create"));
  if (pu) {
    const status = JSON.parse(puCli("status", "--json"));
    assert.equal(status.pointGuard.phase, "ready", JSON.stringify(status));
    assert.equal(status.pointGuard.hostId, first.ready.hostId);
  }
  await stop(first);

  // 2. Phone listener: one-time enrollment over pinned TLS, then revocation.
  const second = await launch(true);
  assert.equal(second.ready.hostId, first.ready.hostId);
  assert.equal(second.ready.certificateSHA256, first.ready.certificateSHA256);
  const enrollment = await second.admin("pairing.create");
  const qr = JSON.parse(enrollment.payload);
  assert.equal(qr.certificateSHA256, second.ready.certificateSHA256);
  assert.equal(qr.endpoint, second.ready.chatURL);
  assert.equal(qr.hostId, second.ready.hostId);
  const enrolled = await enroll(qr, "Linux proof device");
  assert.equal(enrolled.status, 200, enrolled.body);
  const { credential } = JSON.parse(enrolled.body);
  assert.ok(credential);
  const replay = await enroll(qr, "Replayed enrollment");
  assert.notEqual(replay.status, 200, "Enrollment tokens are single use");
  const devices = (await second.admin("devices.list")).devices;
  assert.equal(devices.length, 1);
  assert.ok(!JSON.stringify(devices).includes(credential), "No credential material in device lists");
  // A device credential, or the chat token, is never owner authority.
  for (const bearer of [credential, second.chatToken])
    assert.equal((await second.request(bearer, { operation: "status" })).status, 401);
  // The state lock refuses a second bridge; nothing is adopted.
  const collision = spawn(node, [entry, "--managed"], { env: environment(true), cwd: home, stdio: "ignore" });
  assert.equal((await once(collision, "exit"))[0], 1);
  assert.equal((await second.admin("status")).instanceId, second.ready.instanceId);
  // A second device; grants are set by the owner and enforced per message.
  const tablet = JSON.parse(
    (await enroll(JSON.parse((await second.admin("pairing.create")).payload), "Linux proof tablet")).body,
  );
  const deviceId = (await second.admin("devices.list")).devices.find((d) => d.name === "Linux proof device").deviceId;
  const setGrant = async (name, id, terminals, workspace) => {
    if (pu) puCli("grant", name, "--terminals", terminals, "--workspace", workspace);
    else {
      await second.admin("devices.workspaceManagement", { deviceId: id, workspaceManagement: false });
      await second.admin("devices.terminals", { deviceId: id, terminals });
      await second.admin("devices.workspaceManagement", { deviceId: id, workspaceManagement: workspace === "on" });
    }
  };
  await setGrant("Linux proof device", deviceId, "view", "off");
  await setGrant("Linux proof tablet", tablet.deviceId, "control", "on");
  // ws comes from the payload itself, as the bridge loads it.
  const WebSocket = createRequire(entry)("ws");
  const connect = (who) =>
    new Promise((resolve, reject) => {
      const ws = new WebSocket(qr.endpoint, {
        rejectUnauthorized: false,
        headers: { Authorization: `Bearer ${who.credential}`, "X-PointGuard-Client-ID": who.clientId },
      });
      const inbox = [];
      let next = 0;
      ws.on("message", (bytes) => inbox.push(JSON.parse(bytes.toString())));
      ws.once("upgrade", (res) => {
        const cert = res.socket.getPeerCertificate().raw;
        assert.equal(createHash("sha256").update(cert).digest("hex"), qr.certificateSHA256);
      });
      ws.once("open", () => {
        // Errors after open (e.g. a revoked socket closing) end in "close".
        ws.on("error", () => {});
        resolve({
          ws,
          inbox,
          call: (op, fields = {}) =>
            new Promise((done, fail) => {
              const id = `${op}-${++next}`;
              const timer = setTimeout(() => fail(new Error(`no receipt for ${op}`)), 10000);
              const listener = (bytes) => {
                const r = JSON.parse(bytes.toString());
                if (r.type === "receipt" && r.id === id) {
                  clearTimeout(timer);
                  ws.off("message", listener);
                  done(r);
                }
              };
              ws.on("message", listener);
              ws.send(JSON.stringify({ version: 1, id, op, clientId: who.clientId, ...fields }));
            }),
        });
      });
      ws.once("error", reject);
    });
  const viewer = await connect({ credential, clientId: deviceId });
  const manager = await connect(tablet);
  assert.equal((await viewer.call("permissions")).data.chat, "read");
  assert.equal((await manager.call("permissions")).data.chat, "send");
  assert.equal((await viewer.call("send", { text: "never dispatched" })).ok, false, "View cannot send chat");
  // An unknown credential never upgrades.
  await assert.rejects(connect({ credential: "x".repeat(43), clientId: deviceId }));
  if (pu) {
    const listed = JSON.parse(puCli("devices", "--json"));
    const text = JSON.stringify(listed);
    assert.ok(text.includes("Linux proof device"), text);
    assert.ok(!text.includes(credential) && !text.includes(tablet.credential));
    assert.equal(listed.find((d) => d.name === "Linux proof tablet").chat, "send");
    puCli("revoke", "Linux proof device");
  } else {
    await second.admin("devices.revoke", { deviceId });
  }
  const revoked = (await second.admin("devices.list")).devices;
  assert.ok(revoked.find((d) => d.deviceId === deviceId).revokedAt, "Revocation is durable before success");
  // The revoked device's socket closes now; the other device stays connected.
  if (viewer.ws.readyState !== WebSocket.CLOSED) await once(viewer.ws, "close");
  assert.equal(manager.ws.readyState, WebSocket.OPEN);
  assert.equal((await manager.call("permissions")).data.chat, "send");
  // A live downgrade applies to the very next message.
  await setGrant("Linux proof tablet", tablet.deviceId, "off", "off");
  assert.equal((await manager.call("permissions")).data.chat, "off");
  assert.equal((await manager.call("sync")).ok, false, "Off refuses chat reads");
  manager.ws.close();
  if (pu) {
    // `pu phone pair`: stdout is not a terminal, so it prints the paste code.
    const pairing = () => {
      const child = spawn(pu, ["phone", "pair", "--ttl", "60"], {
        env: { HOME: home, PATH: "/usr/bin:/bin" },
        stdio: ["ignore", "pipe", "pipe"],
      });
      let out = "";
      child.stdout.on("data", (b) => (out += b));
      child.stderr.on("data", (b) => (out += b));
      const code = (async () => {
        for (let i = 0; i < 100 && !out.includes("purepoint-pair:{"); i++) await pause(100);
        const line = out.split("\n").find((l) => l.startsWith("purepoint-pair:"));
        assert.ok(line, out);
        return JSON.parse(line.slice("purepoint-pair:".length));
      })();
      return { child, code, out: () => out };
    };
    const paired = pairing();
    const pasted = await paired.code;
    assert.equal(pasted.certificateSHA256, qr.certificateSHA256);
    assert.equal((await enroll(pasted, "Linux proof paste")).status, 200);
    const [exitCode] = await once(paired.child, "exit");
    assert.equal(exitCode, 0, paired.out());
    assert.match(paired.out(), /Paired "Linux proof paste": terminals off, workspace off, chat none/);
    // An SSH drop (SIGHUP) before use revokes the code.
    const dropped = pairing();
    const abandoned = await dropped.code;
    dropped.child.kill("SIGHUP");
    await once(dropped.child, "exit");
    assert.notEqual((await enroll(abandoned, "Too late")).status, 200, "A dropped pairing code is revoked");
    puCli("revoke", "Linux proof paste");
  }
  await stop(second);

  // 3. Restart: identity and trust survive; the revoked device stays revoked.
  const third = await launch(true);
  assert.equal(third.ready.hostId, first.ready.hostId);
  assert.equal(third.ready.certificateSHA256, first.ready.certificateSHA256);
  const after = (await third.admin("devices.list")).devices;
  assert.equal(after.length, pu ? 3 : 2);
  assert.ok(after.find((d) => d.deviceId === deviceId).revokedAt);
  const kept = after.find((d) => d.deviceId === tablet.deviceId);
  assert.ok(!kept.revokedAt && kept.terminals === "off", "Grants persist across restart");
  await stop(third);
  console.log(
    `PASS ${inPlace ? "installed" : "relocated"} ${manifest.target} payload for pu ${manifest.puVersion}: empty HOME/stripped PATH, private Unix admin, pinned single-use enrollment, per-message grants over the pinned WebSocket, owner token separation, revocation with the other device still connected, lock collision, identity across restart${pu ? ", pu phone status/devices/grant/revoke/pair" : ""}.`,
  );
} finally {
  // Only this proof's own child may be signaled.
  if (child && child.exitCode === null) {
    const exited = once(child, "exit");
    child.kill("SIGTERM");
    await exited;
  }
  await rm(folder, { recursive: true, force: true });
}
