import assert from "node:assert/strict";
import { readFile, mkdtemp, mkdir, rm, cp, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import os from "node:os";
import { randomUUID, createHash } from "node:crypto";
import https from "node:https";
import net from "node:net";
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const source = path.resolve(process.argv[2]);
const folder = await mkdtemp(path.join(os.tmpdir(), "pointguard-relocated-"));
let child;
try {
  const app = path.join(folder, "Relocated PurePoint.app");
  await cp(source, app, { recursive: true });
  const base = path.join(app, "Contents/Resources/PointGuard");
  const manifest = JSON.parse(
    await readFile(path.join(base, "runtime-manifest.json"), "utf8"),
  );
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.contractVersion, 1);
  assert.equal(manifest.piVersion, "1.1.0");
  const node = path.resolve(base, manifest.paths.node),
    entry = path.resolve(base, manifest.paths.entry),
    pu = path.resolve(base, manifest.paths.pu);
  for (const file of [
    node,
    entry,
    pu,
    path.resolve(base, manifest.paths.instructions),
    ...manifest.paths.skills.map((s) => path.resolve(base, s)),
  ])
    assert.ok((await stat(file)).isFile());
  const home = path.join(folder, "empty-home");
  await mkdir(home, { mode: 0o700 });
  const selected = path.join(folder, "selected-cwd");
  await mkdir(selected, { mode: 0o700 });
  const stateDir = path.join(
    home,
    "Library/Application Support/PurePoint/PointGuard",
  );
  const free = net.createServer();
  await new Promise((resolve) => free.listen(0, "127.0.0.1", resolve));
  const port = free.address().port;
  await new Promise((resolve) => free.close(resolve));
  const descriptorFile = path.join(stateDir, "admin.json");
  const launch = async (cwd, remote = false) => {
    const instanceId = randomUUID();
    let output = "";
    child = spawn(node, [entry, "--managed"], {
      env: {
        HOME: home,
        PATH: "/no-external-tools",
        POINT_GUARD_STATE_DIR: stateDir,
        POINT_GUARD_PU_PATH: pu,
        POINT_GUARD_INSTANCE_ID: instanceId,
        PI_SKIP_VERSION_CHECK: "1",
        ...(cwd ? { PI_MOBILE_CWD: cwd } : {}),
        ...(remote
          ? { PI_MOBILE_HOST: "127.0.0.1", PI_MOBILE_PORT: String(port) }
          : {}),
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", (b) => {
      output += b.toString();
    });
    child.stderr.on("data", (b) => {
      output += b.toString();
    });
    child.on("error", () => {});
    let ready;
    for (let i = 0; i < 200; i++) {
      if (child.exitCode !== null)
        throw new Error(`Packaged service exited ${child.exitCode}: ${output}`);
      try {
        const d = JSON.parse(await readFile(descriptorFile, "utf8"));
        if (d.pid === child.pid && d.instanceId === instanceId) {
          ready = d;
          break;
        }
      } catch {}
      await pause(100);
    }
    assert.ok(ready, "Packaged service readiness missing");
    assert.equal(ready.phase, "ready");
    assert.equal(ready.cwd, cwd ?? home);
    assert.equal(ready.instanceId, instanceId);
    assert.equal((await stat(descriptorFile)).mode & 0o777, 0o600);
    const token = (
      await readFile(path.join(stateDir, "admin-token"), "utf8")
    ).trim();
    const chatToken = (
      await readFile(path.join(stateDir, "desktop-chat-token"), "utf8")
    ).trim();
    assert.notEqual(token, chatToken);
    assert.equal(output.includes(token), false);
    assert.equal(output.includes(chatToken), false);
    const admin = async (operation, args = {}) => {
      const response = await fetch(ready.adminURL, {
        method: "POST",
        headers: {
          authorization: `Bearer ${token}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({ operation, ...args }),
      });
      const body = await response.json();
      assert.equal(body.ok, true, JSON.stringify(body));
      return body.result;
    };
    return { ready, admin, token, chatToken };
  };
  const stop = async (context) => {
    const owned = child;
    const exited = once(owned, "exit");
    await context.admin("runtime.stop");
    const [code] = await exited;
    assert.equal(code, 0);
    await assert.rejects(readFile(path.join(stateDir, "runtime.lock")));
    child = null;
  };
  // Defaults do not depend on cwd of invoking process, repo files or external tools.
  const first = await launch();
  const providers = await first.admin("providers");
  assert.ok(
    providers.providers.some(
      (p) => p.id === "anthropic" && p.oauth && p.apiKey,
    ),
  );
  const session = first.ready.sessionId;
  assert.ok(session);
  await stop(first);
  const second = await launch(selected, true);
  assert.equal(second.ready.sessionId, session);
  // Exercise the actual pinned SDK API-key prompt; no model request or billable prompt.
  const attempt = await second.admin("auth.start", {
    provider: "anthropic",
    type: "api_key",
  });
  let auth;
  for (let i = 0; i < 100; i++) {
    auth = await second.admin("auth.status", { attemptId: attempt.attemptId });
    if (auth.prompt) break;
    await pause(25);
  }
  assert.equal(auth.prompt.type, "secret");
  await second.admin("auth.respond", {
    attemptId: attempt.attemptId,
    promptId: auth.prompt.id,
    value: "artifact-proof-noncredential",
  });
  for (let i = 0; i < 100; i++) {
    auth = await second.admin("auth.status", { attemptId: attempt.attemptId });
    if (auth.status !== "pending") break;
    await pause(25);
  }
  assert.equal(auth.status, "complete");
  assert.equal(auth.restartRequired, true);
  const enrollment = await second.admin("pairing.create");
  const qr = JSON.parse(enrollment.payload);
  assert.equal(qr.certificateSHA256, second.ready.certificateSHA256);
  assert.equal(qr.endpoint, second.ready.chatURL);
  const enrolled = await new Promise((resolve, reject) => {
    const request = https.request(
      qr.endpoint.replace("wss:", "https:").replace("/v1", "/pair/enroll"),
      {
        method: "POST",
        rejectUnauthorized: false,
        headers: { "content-type": "application/json" },
      },
      (response) => {
        const cert = response.socket.getPeerCertificate().raw;
        assert.equal(
          createHash("sha256").update(cert).digest("hex"),
          qr.certificateSHA256,
        );
        let body = "";
        response.on("data", (b) => (body += b));
        response.on("end", () => {
          assert.equal(response.statusCode, 200);
          resolve(JSON.parse(body));
        });
      },
    );
    request.on("error", reject);
    request.end(
      JSON.stringify({
        version: 1,
        enrollmentToken: qr.enrollmentToken,
        name: "Artifact proof device",
      }),
    );
  });
  assert.ok(enrolled.credential);
  assert.equal((await second.admin("devices.list")).devices.length, 1);
  const authFile = path.join(home, ".pi/agent/auth.json");
  const authBytes = await readFile(authFile);
  const trustFile = path.join(stateDir, "trust/trust.json");
  const trustBytes = await readFile(trustFile);
  const runtimeState = JSON.parse(
    await readFile(path.join(stateDir, "runtime.json"), "utf8"),
  );
  await stop(second);
  // Replace the entire artifact after awaiting owned exit; durable state lives outside it.
  await rm(app, { recursive: true });
  await cp(source, app, { recursive: true });
  const third = await launch(selected, true);
  assert.equal(third.ready.sessionId, session);
  assert.equal(third.ready.desktopClientId, runtimeState.desktopClientId);
  assert.equal(third.ready.hostId, second.ready.hostId);
  assert.equal(third.ready.certificateSHA256, second.ready.certificateSHA256);
  assert.deepEqual(await readFile(authFile), authBytes);
  assert.deepEqual(await readFile(trustFile), trustBytes);
  assert.equal((await third.admin("devices.list")).devices.length, 1);
  const bad = await fetch(third.ready.adminURL, {
    method: "POST",
    headers: {
      authorization: `Bearer ${enrolled.credential}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ operation: "providers" }),
  });
  assert.equal(bad.status, 401);
  const collision = spawn(node, [entry, "--managed"], {
    env: {
      HOME: home,
      PATH: "/no-external-tools",
      POINT_GUARD_STATE_DIR: stateDir,
      POINT_GUARD_PU_PATH: pu,
      POINT_GUARD_INSTANCE_ID: randomUUID(),
    },
    stdio: "ignore",
  });
  assert.equal((await once(collision, "exit"))[0], 1);
  assert.equal(
    (await third.admin("status")).instanceId,
    third.ready.instanceId,
  );
  await stop(third);
  console.log(
    `PASS clean relocated ${manifest.architecture} package: empty HOME/stripped PATH, SDK login, native session, separate authority, pinned trust, owned replacement, collision/no adoption.`,
  );
} finally {
  // Only this proof's own child may be signaled; never production or unowned PID.
  if (child && child.exitCode === null) {
    const exited = once(child, "exit");
    child.kill("SIGTERM");
    await exited;
  }
  await rm(folder, { recursive: true, force: true });
}
