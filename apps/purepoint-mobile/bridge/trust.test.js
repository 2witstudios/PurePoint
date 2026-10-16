import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, readFile, writeFile, stat, open, chmod, symlink } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { openTrustStore } from "./trust.js";
import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
const endpoint = "wss://100.100.1.2:8787/v1";
async function fixture(fn) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-trust-"));
  let now = Date.now();
  let store;
  try {
    store = await openTrustStore({ directory, now: () => now });
    await fn(store, directory, (value) => {
      now += value;
    });
  } finally {
    await store?.close();
    await rm(directory, { recursive: true, force: true });
  }
}
test("short-lived enrollment is single use under concurrent attempts; credential stays out of durable state", async () =>
  fixture(async (store, directory) => {
    const enrollment = store.createEnrollment({ endpoint });
    const code = JSON.parse(enrollment.payload);
    assert.equal(code.version, 1);
    assert.equal(code.certificateSHA256, store.certificateSHA256);
    assert.equal(code.secret, undefined);
    const outcomes = await Promise.allSettled([
      store.enroll({ enrollmentToken: code.enrollmentToken, name: "Phone" }),
      store.enroll({ enrollmentToken: code.enrollmentToken, name: "Imposter" }),
    ]);
    assert.equal(outcomes.filter((x) => x.status === "fulfilled").length, 1);
    const device = outcomes.find((x) => x.status === "fulfilled").value;
    assert.equal(store.authorize(device.credential).clientId, device.deviceId);
    assert.equal(
      store.enrollmentStatus(enrollment.enrollmentId).status,
      "consumed",
    );
    const disk = await readFile(path.join(directory, "trust.json"), "utf8");
    assert.ok(
      !disk.includes(device.credential) && !disk.includes(code.enrollmentToken),
    );
    assert.equal(
      (await stat(path.join(directory, "trust.json"))).mode & 0o777,
      0o600,
    );
  }));
test("expiry and explicit enrollment revocation reject without creating devices", async () =>
  fixture(async (store, _directory, advance) => {
    const expired = store.createEnrollment({ endpoint, ttlSeconds: 1 });
    advance(1001);
    await assert.rejects(
      store.enroll({
        enrollmentToken: JSON.parse(expired.payload).enrollmentToken,
        name: "Phone",
      }),
    );
    assert.equal(
      store.enrollmentStatus(expired.enrollmentId).status,
      "expired",
    );
    const revoked = store.createEnrollment({ endpoint });
    store.revokeEnrollment(revoked.enrollmentId);
    await assert.rejects(
      store.enroll({
        enrollmentToken: JSON.parse(revoked.payload).enrollmentToken,
        name: "Phone",
      }),
    );
    assert.equal(store.listDevices().length, 0);
  }));
test("trust survives restart while pending enrollments do not; rotating one device preserves another", async () =>
  fixture(async (store, directory) => {
    const enroll = async (name) =>
      store.enroll({
        enrollmentToken: JSON.parse(
          store.createEnrollment({ endpoint }).payload,
        ).enrollmentToken,
        name,
      });
    const phone = await enroll("Phone");
    const other = await enroll("Other");
    const pending = JSON.parse(store.createEnrollment({ endpoint }).payload);
    const identity = [store.hostId, store.certificateSHA256];
    await store.close();
    const restarted = await openTrustStore({ directory });
    try {
      assert.deepEqual(
        [restarted.hostId, restarted.certificateSHA256],
        identity,
      );
      assert.equal(
        restarted.authorize(phone.credential).deviceId,
        phone.deviceId,
      );
      await assert.rejects(
        restarted.enroll({
          enrollmentToken: pending.enrollmentToken,
          name: "Phone",
        }),
      );
      await restarted.rotateDevice(phone.deviceId);
      assert.equal(restarted.authorize(phone.credential), null);
      assert.equal(
        restarted.authorize(other.credential).deviceId,
        other.deviceId,
      );
      await restarted.revokeDevice(other.deviceId);
      assert.equal(restarted.authorize(other.credential), null);
      assert.equal(
        restarted.authorize("legacy-shared-token-0000000000000000"),
        null,
      );
    } finally {
      await restarted.close();
    }
  }));
test("unknown schema, identity mismatch and partial identity fail closed without overwriting state", async () =>
  fixture(async (store, directory) => {
    await assert.rejects(openTrustStore({ directory }), { code: "lock_busy" });
    await store.close();
    const file = path.join(directory, "trust.json");
    const original = JSON.parse(await readFile(file, "utf8"));
    const unknown = JSON.stringify({ ...original, schemaVersion: 99 });
    await writeFile(file, unknown, { mode: 0o600 });
    await assert.rejects(openTrustStore({ directory }));
    assert.equal(await readFile(file, "utf8"), unknown);
    await writeFile(
      file,
      JSON.stringify({ ...original, certificateSHA256: "0".repeat(64) }),
    );
    await assert.rejects(openTrustStore({ directory }));
    await writeFile(file, JSON.stringify(original));
    await rm(path.join(directory, "identity-key.pem"));
    await assert.rejects(openTrustStore({ directory }));
  }));

test("lost durable host ID never regenerates from surviving certificate files", async () =>
  fixture(async (store, directory) => {
    await store.close();
    const cert = await readFile(path.join(directory, "identity-cert.pem"));
    await rm(path.join(directory, "trust.json"));
    await assert.rejects(openTrustStore({ directory }), /missing|restore/i);
    assert.deepEqual(
      await readFile(path.join(directory, "identity-cert.pem")),
      cert,
    );
  }));
test("insecure existing files and public/legacy endpoints never downgrade trust", async () =>
  fixture(async (store, directory) => {
    assert.throws(() =>
      store.createEnrollment({ endpoint: "wss://example.com/v1" }),
    );
    assert.throws(() =>
      store.createEnrollment({ endpoint: "ws://100.100.1.2/v1" }),
    );
    await store.close();
    const { chmod } = await import("node:fs/promises");
    await chmod(path.join(directory, "trust.json"), 0o644);
    await assert.rejects(openTrustStore({ directory }), /private|owner/i);
  }));

test("helper crash fences trust while retained kernel ownership excludes writers until submitted IO drains", async (t) => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-trust-holder-crash-"));
  let store;
  let resume;
  let heldChild;
  const spawn = childProcess.spawn;
  t.mock.method(childProcess, "spawn", (...args) => {
    const child = spawn(...args);
    if (args[0] === process.env.POINT_GUARD_LOCK_HELPER_PATH) heldChild = child;
    return child;
  });
  syncBuiltinESMExports();
  try {
    store = await openTrustStore({ directory });
    assert.ok(heldChild?.pid, "fixture directly owns the spawned helper child");
    const holder = heldChild;
    const phone = await store.enroll({ enrollmentToken: JSON.parse(store.createEnrollment({ endpoint }).payload).enrollmentToken, name: "Saved phone" });
    const original = await readFile(path.join(directory, "trust.json"));
    const certificate = await readFile(path.join(directory, "identity-cert.pem"));
    const inode = (await stat(path.join(directory, "writer.lock"))).ino;
    const probe = await open(path.join(directory, "probe"), "w", 0o600);
    const prototype = Object.getPrototypeOf(probe);
    await probe.close();
    const sync = prototype.sync;
    let entered;
    const entering = new Promise(resolve => { entered = resolve; });
    const gate = new Promise(resolve => { resume = resolve; });
    let blocked = false;
    t.mock.method(prototype, "sync", async function (...args) {
      if (!blocked) { blocked = true; entered(); await gate; }
      return sync.apply(this, args);
    });
    const outcome = store.revokeDevice(phone.deviceId).then(() => null, error => error);
    await entering;
    const loss = once(store, "lockLost");
    holder.kill("SIGKILL"); // Only this fixture's directly-owned helper child.
    const [error] = await loss;
    assert.equal(error.code, "lock_lost");
    assert.equal(store.authorize(phone.credential), null);
    assert.throws(() => store.createEnrollment({ endpoint }), { code: "lock_lost" });
    const queued = store.revokeDevice(phone.deviceId).then(() => null, error => error);
    await assert.rejects(openTrustStore({ directory }), { code: "lock_busy" });
    const closing = store.close();
    await assert.rejects(openTrustStore({ directory }), { code: "lock_busy" });
    resume();
    assert.equal((await outcome).code, "lock_lost");
    assert.equal((await queued).code, "lock_lost");
    await closing;
    const reopened = await openTrustStore({ directory });
    try {
      assert.equal(reopened.authorize(phone.credential).deviceId, phone.deviceId);
      assert.deepEqual(await readFile(path.join(directory, "trust.json")), original);
      assert.deepEqual(await readFile(path.join(directory, "identity-cert.pem")), certificate);
      assert.equal((await stat(path.join(directory, "writer.lock"))).ino, inode);
    } finally { await reopened.close(); }
  } finally {
    resume?.();
    t.mock.restoreAll();
    syncBuiltinESMExports();
    await store?.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("isolated bridge crash releases inherited helper ownership and preserves same inode and durable phone trust", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-trust-bridge-crash-"));
  const source = `import { openTrustStore } from ${JSON.stringify(new URL("./trust.js", import.meta.url).href)};
    const store = await openTrustStore({directory:process.env.TRUST_FIXTURE_DIRECTORY});
    const phone = await store.enroll({enrollmentToken:JSON.parse(store.createEnrollment({endpoint:${JSON.stringify(endpoint)}}).payload).enrollmentToken,name:"Phone"});
    process.send({phone,hostId:store.hostId,certificateSHA256:store.certificateSHA256});
    setInterval(()=>{},1000);`;
  const child = childProcess.spawn(process.execPath, ["--input-type=module", "-e", source], {
    env: { ...process.env, TRUST_FIXTURE_DIRECTORY: directory }, stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  let restarted;
  try {
    const [ready] = await Promise.race([once(child, "message"), once(child, "exit").then(() => { throw new Error("fixture bridge exited before ready"); }), delay(10000, undefined, { ref: false }).then(() => { throw new Error("fixture bridge readiness timed out"); })]);
    const before = await readFile(path.join(directory, "trust.json"));
    const cert = await readFile(path.join(directory, "identity-cert.pem"));
    const inode = (await stat(path.join(directory, "writer.lock"))).ino;
    await assert.rejects(openTrustStore({ directory }), { code: "lock_busy" });
    const exited = once(child, "exit");
    child.kill("SIGKILL"); // Only the fixture bridge spawned above; never production/session processes.
    await exited;
    for (let attempt = 0; attempt < 50; attempt++) {
      try { restarted = await openTrustStore({ directory }); break; }
      catch (error) { if (error.code !== "lock_busy" || attempt === 49) throw error; await delay(20); }
    }
    assert.equal(restarted.hostId, ready.hostId);
    assert.equal(restarted.certificateSHA256, ready.certificateSHA256);
    assert.equal(restarted.authorize(ready.phone.credential).deviceId, ready.phone.deviceId);
    assert.deepEqual(await readFile(path.join(directory, "trust.json")), before);
    assert.deepEqual(await readFile(path.join(directory, "identity-cert.pem")), cert);
    assert.equal((await stat(path.join(directory, "writer.lock"))).ino, inode);
  } finally {
    if (child.exitCode === null && child.signalCode === null) { const exited = once(child, "exit"); child.kill("SIGKILL"); await exited; }
    await restarted?.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("permanent private old PID marker is inert; public or symlink lock refuses without repair", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-trust-lock-migration-"));
  const file = path.join(directory, "writer.lock");
  let store;
  try {
    await writeFile(file, "1", { mode: 0o600 });
    const inode = (await stat(file)).ino;
    store = await openTrustStore({ directory });
    await store.close();
    assert.equal(await readFile(file, "utf8"), "1");
    assert.equal((await stat(file)).ino, inode);
    await chmod(file, 0o644);
    await assert.rejects(openTrustStore({ directory }), { code: "lock_private" });
    assert.equal((await stat(file)).mode & 0o777, 0o644);
    await rm(file); // Fixture-only replacement to construct a malicious symlink, never recovery.
    await symlink(path.join(directory, "trust.json"), file);
    const original = await readFile(path.join(directory, "trust.json"));
    await assert.rejects(openTrustStore({ directory }), { code: "lock_private" });
    assert.deepEqual(await readFile(path.join(directory, "trust.json")), original);
  } finally { await store?.close(); await rm(directory, { recursive: true, force: true }); }
});

test("terminal access defaults off for existing devices and explicit grants persist across restart", async () =>
  fixture(async (store, directory) => {
    const phone = await store.enroll({
      enrollmentToken: JSON.parse(store.createEnrollment({ endpoint }).payload)
        .enrollmentToken,
      name: "Existing phone",
    });
    const legacy = JSON.parse(
      await readFile(path.join(directory, "trust.json"), "utf8"),
    );
    assert.equal(legacy.devices[0].terminals, undefined);
    await store.close();
    const existing = await openTrustStore({ directory });
    try {
      assert.equal(existing.authorize(phone.credential).terminals, "off");
      assert.equal(existing.listDevices()[0].terminals, "off");
    } finally { await existing.close(); }
    store = await openTrustStore({ directory });
    assert.equal(store.authorize(phone.credential).terminals, "off");
    assert.equal(store.listDevices()[0].terminals, "off");
    await store.setTerminals(phone.deviceId, "control");
    assert.equal(store.authorize(phone.credential).terminals, "control");
    const downgrade = store.setTerminals(phone.deviceId, "view");
    assert.equal(store.authorize(phone.credential).terminals, "view");
    await downgrade;
    await assert.rejects(
      store.setTerminals(phone.deviceId, "admin"),
      /permission/,
    );
    await store.close();
    const reopened = await openTrustStore({ directory });
    try {
      assert.equal(reopened.authorize(phone.credential).terminals, "view");
    } finally {
      await reopened.close();
    }
  }));

test("newer downgrade fences queued upgrades before durable IO completes", async (t) =>
  fixture(async (store, directory) => {
    const phone = await store.enroll({
      enrollmentToken: JSON.parse(store.createEnrollment({ endpoint }).payload)
        .enrollmentToken,
      name: "Phone",
    });
    const probe = await open(path.join(directory, "probe"), "w", 0o600);
    const prototype = Object.getPrototypeOf(probe);
    await probe.close();
    const originalSync = prototype.sync;
    let release, entered;
    const gate = new Promise((resolve) => {
      release = resolve;
    });
    const ready = new Promise((resolve) => {
      entered = resolve;
    });
    let blocked = false;
    t.mock.method(prototype, "sync", async function (...args) {
      if (!blocked) {
        blocked = true;
        entered();
        await gate;
      }
      return originalSync.apply(this, args);
    });
    const first = store.setTerminals(phone.deviceId, "control");
    await ready;
    const last = store.setTerminals(phone.deviceId, "off");
    assert.equal(store.authorize(phone.credential).terminals, "off");
    release();
    await first;
    assert.equal(
      store.authorize(phone.credential).terminals,
      "off",
      "older successful grant cannot resurrect permission",
    );
    await last;
    t.mock.restoreAll();
    assert.equal(
      JSON.parse(await readFile(path.join(directory, "trust.json"), "utf8"))
        .devices[0].terminals,
      "off",
    );
  }));

test("workspace management is default-off for old control grants, persists explicitly and fences queued upgrade", async () =>
  fixture(async (store, directory) => {
    const device = await store.enroll({
      enrollmentToken: JSON.parse(store.createEnrollment({ endpoint }).payload)
        .enrollmentToken,
      name: "Phone",
    });
    await store.setTerminals(device.deviceId, "control");
    assert.equal(store.authorize(device.credential).workspaceManagement, false);
    await assert.rejects(store.setWorkspaceManagement(device.deviceId, "true"));
    await store.setWorkspaceManagement(device.deviceId, true);
    assert.equal(store.listDevices()[0].workspaceManagement, true);
    await store.close();
    const restarted = await openTrustStore({ directory });
    try {
      assert.equal(
        restarted.authorize(device.credential).workspaceManagement,
        true,
      );
      const upgrade = restarted.setWorkspaceManagement(device.deviceId, true);
      const downgrade = restarted.setWorkspaceManagement(
        device.deviceId,
        false,
      );
      assert.equal(
        restarted.authorize(device.credential).workspaceManagement,
        false,
      );
      await Promise.all([upgrade, downgrade]);
      assert.equal(
        restarted.authorize(device.credential).workspaceManagement,
        false,
      );
    } finally {
      await restarted.close();
    }
  }));
