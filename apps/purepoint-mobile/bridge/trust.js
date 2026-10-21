import { EventEmitter } from "node:events";
import { mkdir, open, readFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import {
  randomBytes,
  randomUUID,
  createHash,
  timingSafeEqual,
} from "node:crypto";
import { ensureHostTLS, privateEntry } from "./trust-tls.js";
import { pairingEndpoint } from "./endpoints.js";
import { acquirePrivateLock } from "./lock-helper.js";
const opaque = () => randomBytes(32).toString("base64url");
const digest = (value) => createHash("sha256").update(value).digest("hex");
const validToken = (value) =>
  typeof value === "string" && /^[A-Za-z0-9_-]{43}$/.test(value);
const uuid = (value) =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
    value,
  );
function same(left, right) {
  const a = Buffer.from(left, "hex");
  const b = Buffer.from(right, "hex");
  return a.length === b.length && timingSafeEqual(a, b);
}
async function publish(file, value, assertHeld) {
  assertHeld();
  const temp = file + "." + randomUUID() + ".tmp";
  let handle;
  try {
    handle = await open(temp, "wx", 0o600);
    await handle.writeFile(JSON.stringify(value));
    await handle.sync();
    await handle.close();
    handle = null;
    assertHeld();
    await rename(temp, file);
    const directory = await open(path.dirname(file), "r");
    try {
      await directory.sync();
      assertHeld();
    } finally {
      await directory.close();
    }
  } finally {
    await handle?.close();
    await rm(temp, { force: true });
  }
}
function validate(state, pin) {
  if (
    state?.schemaVersion !== 1 ||
    !uuid(state.hostId) ||
    state.certificateSHA256 !== pin ||
    !Array.isArray(state.devices) ||
    state.devices.length > 256
  )
    throw new Error(
      "Point Guard trust schema or host identity mismatch. Restore original state; do not downgrade or silently reset.",
    );
  const ids = new Set();
  const hashes = new Set();
  for (const device of state.devices) {
    if (
      !uuid(device.deviceId) ||
      device.clientId !== device.deviceId ||
      ids.has(device.deviceId) ||
      !/^[a-f0-9]{64}$/.test(device.credentialHash) ||
      hashes.has(device.credentialHash) ||
      typeof device.name !== "string" ||
      !device.name.length ||
      device.name.length > 80 ||
      !Number.isSafeInteger(device.createdAt) ||
      (device.revokedAt != null && !Number.isSafeInteger(device.revokedAt)) ||
      (device.workspaceManagement !== undefined &&
        typeof device.workspaceManagement !== "boolean") ||
      (device.terminals !== undefined &&
        !["off", "view", "control"].includes(device.terminals))
    )
      throw new Error(
        "Point Guard device trust is malformed. Restore original private state.",
      );
    ids.add(device.deviceId);
    hashes.add(device.credentialHash);
  }
}
/** One private durable writer. Unknown/legacy state never becomes remote authorization. */
export async function openTrustStore({
  directory,
  now = Date.now,
  helperPath,
}) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await privateEntry(directory, true);
  let lockFailure;
  let notifyLoss;
  const lock = await acquirePrivateLock({
    file: path.join(directory, "writer.lock"),
    helperPath,
    onLost(error) {
      lockFailure = error;
      notifyLoss?.(error);
    },
  });
  const assertHeld = () => {
    if (lockFailure) throw lockFailure;
    lock.assertHeld();
  };
  try {
    assertHeld();
    const tls = await ensureHostTLS(directory, assertHeld);
    assertHeld();
    const file = path.join(directory, "trust.json");
    let state;
    try {
      const info = await privateEntry(file);
      if (info.size > 256 * 1024)
        throw new Error("Point Guard trust state exceeds its limit.");
      state = JSON.parse(await readFile(file, "utf8"));
    } catch (e) {
      if (e.code !== "ENOENT") throw e;
      if (!tls.created)
        throw new Error(
          "Point Guard host trust is missing beside existing TLS identity. Restore original trust or deliberately reset and re-pair.",
        );
      state = {
        schemaVersion: 1,
        hostId: randomUUID(),
        certificateSHA256: tls.certificateSHA256,
        devices: [],
      };
      await publish(file, state, assertHeld);
    }
    assertHeld();
    validate(state, tls.certificateSHA256);
    /** @type {EventEmitter & Record<string, any>} */
    const trust = new EventEmitter();
    const enrollments = new Map();
    const permissionFences = new Map(
      state.devices.map((device) => [
        device.deviceId,
        device.terminals ?? "off",
      ]),
    );
    const managementFences = new Map(
      state.devices.map((d) => [d.deviceId, d.workspaceManagement === true]),
    );
    const managementVersions = new Map();
    const revocations = new Set();
    const permissionVersions = new Map();
    let tail = Promise.resolve();
    let closed = false;
    let closing = false;
    let closingTask;
    notifyLoss = (error) => {
      enrollments.clear();
      trust.emit("lockLost", error);
    };
    const active = () => {
      assertHeld();
      if (closed || closing)
        throw new Error("Point Guard trust store is closed.");
    };
    const serialized = (fn) => {
      const work = tail.then(() => {
        active();
        return fn();
      });
      tail = work.catch(() => {});
      return work;
    };
    Object.assign(trust, {
      hostId: state.hostId,
      certificateSHA256: tls.certificateSHA256,
      tls: { cert: tls.cert, key: tls.key },
      createEnrollment({ endpoint, ttlSeconds = 120 }) {
        active();
        const url = pairingEndpoint(endpoint);
        // Endpoint validation is shared with the pairing encoder; caller adapter binds actual configured endpoint.
        if (!Number.isInteger(ttlSeconds) || ttlSeconds < 1 || ttlSeconds > 300)
          throw new Error(
            "Pairing requires WSS /v1 and an enrollment lifetime of 1–300 seconds.",
          );
        for (const [id, entry] of enrollments)
          if (entry.expiresAt + 300000 < now()) enrollments.delete(id);
        if (enrollments.size >= 64)
          throw new Error("Too many pending enrollments. Wait for expiry.");
        const enrollmentId = randomUUID();
        const token = opaque();
        const expiresAt = now() + ttlSeconds * 1000;
        enrollments.set(enrollmentId, {
          hash: digest(token),
          expiresAt,
          status: "pending",
        });
        return {
          enrollmentId,
          expiresAt,
          payload: JSON.stringify({
            type: "pi-mobile-pairing",
            version: 1,
            endpoint: url.href,
            hostId: state.hostId,
            certificateSHA256: state.certificateSHA256,
            enrollmentToken: token,
            expiresAt,
          }),
        };
      },
      enrollmentStatus(enrollmentId) {
        active();
        const entry = enrollments.get(enrollmentId);
        if (!entry) return { enrollmentId, status: "unknown" };
        if (entry.status === "pending" && entry.expiresAt <= now())
          entry.status = "expired";
        return {
          enrollmentId,
          status: entry.status,
          expiresAt: entry.expiresAt,
          ...(entry.deviceId ? { deviceId: entry.deviceId } : {}),
        };
      },
      revokeEnrollment(enrollmentId) {
        active();
        const entry = enrollments.get(enrollmentId);
        if (entry?.status === "pending") entry.status = "revoked";
      },
      authorize(credential) {
        if (
          closed ||
          closing ||
          lockFailure ||
          !lock.held ||
          !validToken(credential)
        )
          return null;
        const hash = digest(credential);
        const device = state.devices.find(
          (d) =>
            same(hash, d.credentialHash) &&
            d.revokedAt == null &&
            !revocations.has(d.deviceId),
        );
        return device
          ? {
              deviceId: device.deviceId,
              clientId: device.clientId,
              workspaceManagement:
                managementFences.get(device.deviceId) === true,
              terminals:
                permissionFences.get(device.deviceId) ??
                device.terminals ??
                "off",
            }
          : null;
      },
      enroll({ enrollmentToken, name }) {
        return serialized(async () => {
          if (
            !validToken(enrollmentToken) ||
            typeof name !== "string" ||
            !name.trim() ||
            name.length > 80 ||
            /[\x00-\x1f\x7f]/.test(name)
          )
            throw new Error("Invalid device enrollment.");
          const hash = digest(enrollmentToken);
          const entry = [...enrollments.values()].find((e) =>
            same(hash, e.hash),
          );
          if (!entry || entry.status !== "pending" || entry.expiresAt <= now())
            throw new Error(
              "Enrollment expired, revoked or already used. Create a new pairing code on the host.",
            );
          if (state.devices.length >= 256)
            throw new Error(
              "Device trust is full. Deliberate trust maintenance required.",
            );
          // Consume before awaiting disk: failed/uncertain enrollment must obtain a fresh QR, never replay.
          entry.status = "consumed";
          const credential = opaque();
          const deviceId = randomUUID();
          const next = {
            ...state,
            devices: [
              ...state.devices,
              {
                deviceId,
                clientId: deviceId,
                name: name.trim(),
                createdAt: now(),
                credentialHash: digest(credential),
              },
            ],
          };
          await publish(file, next, assertHeld);
          assertHeld();
          state = next;
          // Reported to the owner only once the device is durable.
          entry.deviceId = deviceId;
          permissionFences.set(deviceId, "off");
          return {
            version: 1,
            hostId: state.hostId,
            deviceId,
            clientId: deviceId,
            credential,
          };
        });
      },
      listDevices() {
        active();
        return state.devices.map(({ credentialHash, ...device }) => ({
          ...device,
          workspaceManagement: managementFences.get(device.deviceId) === true,
          terminals:
            permissionFences.get(device.deviceId) ?? device.terminals ?? "off",
        }));
      },
      renameDevice(deviceId, name) {
        if (
          typeof name !== "string" ||
          !name.length ||
          name.length > 80 ||
          /[\x00-\x1f\x7f]/.test(name)
        )
          return Promise.reject(new Error("Invalid device name"));
        return serialized(async () => {
          if (
            !state.devices.some(
              (d) => d.deviceId === deviceId && d.revokedAt == null,
            )
          )
            throw new Error("Unknown paired device");
          const next = {
            ...state,
            devices: state.devices.map((d) =>
              d.deviceId === deviceId ? { ...d, name } : d,
            ),
          };
          await publish(file, next, assertHeld);
          assertHeld();
          state = next;
          return {};
        });
      },
      setWorkspaceManagement(deviceId, workspaceManagement) {
        if (typeof workspaceManagement !== "boolean")
          return Promise.reject(new Error("Invalid workspace permission"));
        try {
          active();
        } catch (error) {
          return Promise.reject(error);
        }
        if (
          !state.devices.some(
            (d) => d.deviceId === deviceId && d.revokedAt == null,
          )
        )
          return Promise.reject(new Error("Unknown paired device"));
        const version = (managementVersions.get(deviceId) ?? 0) + 1;
        managementVersions.set(deviceId, version);
        if (!workspaceManagement) {
          managementFences.set(deviceId, false);
          trust.emit("permissions", deviceId);
        }
        return serialized(async () => {
          const next = {
            ...state,
            devices: state.devices.map((d) =>
              d.deviceId === deviceId ? { ...d, workspaceManagement } : d,
            ),
          };
          await publish(file, next, assertHeld);
          assertHeld();
          state = next;
          if (
            managementVersions.get(deviceId) === version &&
            !revocations.has(deviceId)
          ) {
            managementFences.set(deviceId, workspaceManagement);
            trust.emit("permissions", deviceId);
          }
          return {};
        });
      },
      setTerminals(deviceId, terminals) {
        if (!["off", "view", "control"].includes(terminals))
          return Promise.reject(new Error("Invalid terminal permission"));
        // Fence synchronously, before durable IO or earlier queued administration.
        try {
          active();
        } catch (error) {
          return Promise.reject(error);
        }
        const device = state.devices.find(
          (d) => d.deviceId === deviceId && d.revokedAt == null,
        );
        if (!device) return Promise.reject(new Error("Unknown paired device"));
        const previous =
          permissionFences.get(deviceId) ?? device.terminals ?? "off";
        const version = (permissionVersions.get(deviceId) ?? 0) + 1;
        permissionVersions.set(deviceId, version);
        const downgrade =
          ["off", "view", "control"].indexOf(terminals) <
          ["off", "view", "control"].indexOf(previous);
        if (downgrade) {
          permissionFences.set(deviceId, terminals);
          state = {
            ...state,
            devices: state.devices.map((d) =>
              d.deviceId === deviceId ? { ...d, terminals } : d,
            ),
          };
          trust.emit("permissions", deviceId);
        }
        return serialized(async () => {
          const next = {
            ...state,
            devices: state.devices.map((d) =>
              d.deviceId === deviceId ? { ...d, terminals } : d,
            ),
          };
          await publish(file, next, assertHeld);
          assertHeld();
          state = next;
          if (
            permissionVersions.get(deviceId) === version &&
            !revocations.has(deviceId)
          ) {
            permissionFences.set(deviceId, terminals);
            if (!downgrade) trust.emit("permissions", deviceId);
          }
          return {};
        });
      },
      revokeDevice(deviceId) {
        try {
          active();
        } catch (error) {
          return Promise.reject(error);
        }
        if (!state.devices.some((d) => d.deviceId === deviceId))
          return Promise.reject(new Error("Unknown paired device."));
        revocations.add(deviceId);
        state = {
          ...state,
          devices: state.devices.map((d) =>
            d.deviceId === deviceId ? { ...d, revokedAt: now() } : d,
          ),
        };
        trust.emit("revoked", deviceId);
        return serialized(async () => {
          if (!state.devices.some((d) => d.deviceId === deviceId))
            throw new Error("Unknown paired device.");
          const next = {
            ...state,
            devices: state.devices.map((d) =>
              d.deviceId === deviceId ? { ...d, revokedAt: now() } : d,
            ),
          };
          await publish(file, next, assertHeld);
          assertHeld();
          state = next;
        });
      },
      rotateDevice(deviceId) {
        return trust.revokeDevice(deviceId);
      },
      close() {
        if (closingTask) return closingTask;
        closing = true;
        closingTask = (async () => {
          await tail;
          closed = true;
          enrollments.clear();
          // Retained parent FD keeps kernel ownership until all submitted IO drains.
          await lock.release();
        })();
        return closingTask;
      },
    });
    return trust;
  } catch (e) {
    await lock.release();
    throw e;
  }
}
