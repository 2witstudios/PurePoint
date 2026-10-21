import { createServer } from "node:http";
import { timingSafeEqual } from "node:crypto";
import { chmod, lstat, mkdir, rm } from "node:fs/promises";
import path from "node:path";
/** Safe public errors must be constructed explicitly; raw SDK errors are never forwarded. */
export class AdminError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}
/** Local owner route; phones cannot reach this administrative listener. */
export function setDeviceTerminals(trust, { deviceId, terminals }) {
  if (
    typeof deviceId !== "string" ||
    !deviceId ||
    !["off", "view", "control"].includes(terminals)
  )
    throw new AdminError(
      "terminal_permission",
      "Select Off, View or Control for a paired device.",
    );
  return trust.setTerminals(deviceId, terminals);
}
function matches(actual, expected) {
  const a = Buffer.from(actual ?? "");
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}
/**
 * Create the owner admin socket directory fresh: 0700, owned by this user,
 * never a symlink. Caller holds the exclusive state lock.
 */
export async function prepareAdminSocket(socketPath) {
  const directory = path.dirname(socketPath);
  if (!path.isAbsolute(socketPath) || Buffer.byteLength(socketPath) > 100)
    throw new AdminError(
      "admin_socket",
      "Owner admin socket path must be absolute and short.",
    );
  await rm(directory, { recursive: true, force: true });
  await mkdir(directory, { mode: 0o700 });
  // mkdir honors umask; set the mode explicitly, then verify.
  await chmod(directory, 0o700);
  const info = await lstat(directory);
  if (
    info.isSymbolicLink() ||
    !info.isDirectory() ||
    (info.mode & 0o777) !== 0o700 ||
    (typeof process.getuid === "function" && info.uid !== process.getuid())
  )
    throw new AdminError(
      "admin_socket",
      "Owner admin directory is not private. Restore private state.",
    );
}
/**
 * Owner-only HTTP authority: a loopback ephemeral TCP listener (Mac, where the
 * app holds the token) or a Unix socket in a private directory (Linux).
 */
export async function serveAdmin({ token, dispatch, socketPath = undefined }) {
  const server = createServer(async (req, res) => {
    const send = (status, value) => {
      res.writeHead(status, {
        "content-type": "application/json",
        "cache-control": "no-store",
        connection: "close",
      });
      res.end(JSON.stringify(value));
    };
    const fail = (status, code, message) =>
      send(status, { ok: false, error: { code, message } });
    if (req.headers.origin !== undefined)
      return fail(403, "origin", "Browser requests are not allowed.");
    if (req.method !== "POST") return fail(405, "method", "Use POST.");
    if (req.url !== "/admin/v1")
      return fail(404, "route", "Unknown administrative route.");
    if (!matches(req.headers.authorization, `Bearer ${token}`))
      return fail(401, "authorization", "Owner authentication required.");
    if (req.headers["content-type"] !== "application/json")
      return fail(415, "content_type", "Use JSON.");
    const size = Number(req.headers["content-length"]);
    if (size > 65536)
      return fail(413, "size", "Administrative request too large.");
    try {
      let bytes = 0;
      const chunks = [];
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 65536)
          return fail(413, "size", "Administrative request too large.");
        chunks.push(chunk);
      }
      let request;
      try {
        request = JSON.parse(Buffer.concat(chunks).toString());
      } catch {
        return fail(400, "json", "Invalid JSON.");
      }
      if (
        !request ||
        typeof request.operation !== "string" ||
        Array.isArray(request)
      )
        return fail(400, "operation", "Administrative operation required.");
      send(200, { ok: true, result: await dispatch(request) });
    } catch (e) {
      fail(
        400,
        e instanceof AdminError ? e.code : "operation_failed",
        e instanceof AdminError
          ? e.message
          : "Operation failed. Inspect setup status and retry.",
      );
    }
  });
  server.requestTimeout = 10000;
  server.headersTimeout = 10000;
  server.maxConnections = 16;
  if (socketPath) await prepareAdminSocket(socketPath);
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    const ready = () => {
      server.off("error", reject);
      resolve(null);
    };
    if (socketPath) server.listen(socketPath, ready);
    else server.listen(0, "127.0.0.1", ready);
  });
  if (socketPath) {
    // Socket mode is explicit, independent of umask; the 0700 parent covers
    // the window between bind and chmod.
    await chmod(socketPath, 0o600);
    return {
      socket: socketPath,
      shutdown: () =>
        new Promise((resolve) => {
          server.close(() =>
            rm(path.dirname(socketPath), { recursive: true, force: true })
              .catch(() => {})
              .then(() => resolve(null)),
          );
          server.closeAllConnections();
        }),
    };
  }
  return {
    url: `http://127.0.0.1:${/** @type {import("node:net").AddressInfo} */ (server.address()).port}/admin/v1`,
    shutdown: () =>
      new Promise((resolve) => {
        server.close(() => resolve(null));
        server.closeAllConnections();
      }),
  };
}

export function setDeviceWorkspaceManagement(
  trust,
  { deviceId, workspaceManagement },
) {
  if (
    typeof deviceId !== "string" ||
    !deviceId ||
    typeof workspaceManagement !== "boolean"
  )
    throw new AdminError(
      "workspace_permission",
      "Select workspace management for a paired device.",
    );
  return trust.setWorkspaceManagement(deviceId, workspaceManagement);
}

export function renameDevice(trust, { deviceId, name }) {
  if (
    typeof deviceId !== "string" ||
    !deviceId ||
    typeof name !== "string" ||
    !name.trim() ||
    name.length > 80 ||
    /[\x00-\x1f\x7f]/.test(name)
  )
    throw new AdminError(
      "device_name",
      "Use a device name of 1–80 printable characters.",
    );
  return trust.renameDevice(deviceId, name.trim());
}
