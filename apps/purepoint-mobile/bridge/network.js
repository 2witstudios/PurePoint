import { terminalConnection, terminalOperation } from "./terminals.js";
import http from "node:http";
import https from "node:https";
import { timingSafeEqual } from "node:crypto";
import { WebSocketServer, WebSocket } from "ws";
import { allowedHost } from "./endpoints.js";
export { allowedHost } from "./endpoints.js";
const chatLevels = ["off", "read", "send"];
/** Every chat op and the chat access it needs. Anything else never reaches the controller. */
const chatOperations = new Map([
  ["sync", "read"],
  ["sessions", "read"],
  ["history", "read"],
  ["send", "send"],
  ["stop", "send"],
  ["new", "send"],
  ["resume", "send"],
  ["answer", "send"],
]);
/**
 * Point Guard chat follows the existing device grants. Point Guard has a shell and
 * the `pu` CLI, so sending is full control: it needs Control and Workspace management.
 * Reading needs View or Control. The loopback desktop principal is the owner.
 */
export function chatLevel(identity) {
  if (!identity) return "off";
  if (identity.owner === true) return "send";
  if (identity.terminals === "control" && identity.workspaceManagement === true)
    return "send";
  return ["view", "control"].includes(identity.terminals) ? "read" : "off";
}
const chatAllowed = (granted, needed) =>
  chatLevels.includes(granted) &&
  chatLevels.indexOf(granted) >= chatLevels.indexOf(needed);
function chatGate(granted, op) {
  const needed = chatOperations.get(op);
  if (!needed) throw new Error("Unsupported bridge operation");
  if (chatAllowed(granted, needed)) return;
  throw new Error(
    granted === "read"
      ? "Point Guard chat is read-only on this device. Sending needs Agents and terminals: Control and Workspace management. Change this on the host this device is paired with."
      : "Point Guard chat needs Agents and terminals: View or Control on this device. Change this on the host this device is paired with.",
  );
}
function authenticated(header, token) {
  if (typeof header !== "string") return false;
  const actual = Buffer.from(header);
  const expected = Buffer.from(`Bearer ${token}`);
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}
export async function serve(
  controller,
  {
    host,
    port,
    trust = null,
    localAdmin = null,
    tls = null,
    daemons = undefined,
    terminalQueueLimit = undefined,
    hostInfo = undefined,
    allowPeer = undefined,
  },
) {
  if (!allowedHost(host))
    throw new Error(
      "Bind to an explicit Tailscale IP or loopback address. Public/wildcard addresses are refused.",
    );
  if (Boolean(trust) === Boolean(localAdmin))
    throw new Error(
      "Configure exactly one remote device trust or local desktop chat principal; legacy shared-token authorization is disabled.",
    );
  if (
    localAdmin &&
    (!["127.0.0.1", "::1"].includes(host) ||
      typeof localAdmin.token !== "string" ||
      localAdmin.token.length < 32 ||
      !/^[A-Za-z0-9_-]{1,100}$/.test(localAdmin.clientId))
  )
    throw new Error(
      "Local desktop chat requires loopback and a fixed private principal.",
    );
  if (trust && !tls)
    throw new Error(
      "Remote device trust requires pinned TLS; plaintext authorization is refused.",
    );
  const principal = (header) => {
    if (localAdmin)
      return authenticated(header, localAdmin.token)
        ? { clientId: localAdmin.clientId, owner: true }
        : null;
    return typeof header === "string" && header.startsWith("Bearer ")
      ? trust.authorize(header.slice(7))
      : null;
  };
  let pairingRequests = 0;
  const handler = async (req, res) => {
    const respond = (code, data = null) => {
      res.writeHead(code, {
        "Content-Type": "application/json",
        "Cache-Control": "no-store",
      });
      res.end(data ? JSON.stringify(data) : "");
    };
    if (req.headers.origin) {
      respond(403);
      return;
    }
    if (!trust || !["/pair/enroll", "/pair/verify"].includes(req.url)) {
      respond(404);
      return;
    }
    if (req.url === "/pair/verify") {
      if (req.method !== "GET") {
        respond(405);
        return;
      }
      const identity = principal(req.headers.authorization);
      respond(
        identity ? 200 : 401,
        identity
          ? {
              version: 1,
              hostId: trust.hostId,
              ...(hostInfo
                ? { hostKind: hostInfo.hostKind, hostLabel: hostInfo.hostLabel }
                : {}),
              ...identity,
            }
          : null,
      );
      return;
    }
    if (req.method !== "POST") {
      respond(405);
      return;
    }
    if (pairingRequests >= 16) {
      respond(503);
      return;
    }
    pairingRequests++;
    req.setTimeout(5000, () => req.destroy());
    try {
      let size = 0;
      const chunks = [];
      for await (const chunk of req) {
        size += chunk.length;
        if (size > 8192) {
          respond(413);
          req.destroy();
          return;
        }
        chunks.push(chunk);
      }
      const body = JSON.parse(Buffer.concat(chunks).toString());
      if (body.version !== 1) {
        respond(400);
        return;
      }
      const enrolled = await trust.enroll({
        enrollmentToken: body.enrollmentToken,
        name: body.name,
      });
      respond(200, enrolled);
    } catch {
      if (!res.headersSent && !res.destroyed)
        respond(401, {
          error: "Enrollment unavailable. Create a new pairing code on the host.",
        });
    } finally {
      req.setTimeout(0);
      pairingRequests--;
    }
  };
  const server = tls
    ? https.createServer(tls, handler)
    : http.createServer(handler);
  // Pre-authentication limits: bounded sockets and absolute header/request
  // deadlines, so unauthenticated peers cannot hold the listener.
  server.maxConnections = 64;
  server.headersTimeout = 10000;
  server.requestTimeout = 10000;
  server.keepAliveTimeout = 5000;
  if (allowPeer)
    // Raw TCP accept, before any TLS handshake or request parsing.
    server.on("connection", (/** @type {import("node:net").Socket} */ socket) => {
      if (!allowPeer(socket.remoteAddress ?? "")) socket.destroy();
    });
  const wss = new WebSocketServer({
    noServer: true,
    maxPayload: 1024 * 1024,
    perMessageDeflate: false,
  });

  const identities = new WeakMap();
  const terminals = new WeakMap();
  server.on("upgrade", (req, socket, head) => {
    let code = 401;
    if (req.url !== "/v1" || req.headers.origin) code = 403;
    else {
      const identity = principal(req.headers.authorization);
      if (identity) {
        const clientId = req.headers["x-pointguard-client-id"];
        if (clientId !== identity.clientId) code = 403;
        else if (wss.clients.size >= 32) code = 503;
        else if (
          identity.deviceId &&
          [...wss.clients].filter(
            (ws) => identities.get(ws)?.deviceId === identity.deviceId,
          ).length >= 4
        )
          code = 503;
        else code = 101;
      }
    }
    if (code !== 101) {
      socket.end(
        `HTTP/1.1 ${code} Rejected\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`,
      );
      return;
    }
    wss.handleUpgrade(req, socket, head, (ws) => {
      identities.set(ws, {
        ...principal(req.headers.authorization),
        credential: req.headers.authorization,
      });
      wss.emit("connection", ws);
    });
  });
  function send(ws, data) {
    sendSerialized(ws, JSON.stringify(data));
  }
  function sendSerialized(ws, serialized) {
    if (
      Buffer.byteLength(serialized) > 4 * 1024 * 1024 ||
      ws.bufferedAmount > 4 * 1024 * 1024
    ) {
      ws.close(1009, "Output backlog; reconnect to refresh");
      return;
    }
    if (ws.readyState === WebSocket.OPEN) ws.send(serialized);
  }
  // One listener per authoritative event, independent of the number of views.
  const broadcast = (record) => {
    const serialized = JSON.stringify(record);
    for (const ws of wss.clients)
      if (
        chatAllowed(
          chatLevel(principal(identities.get(ws)?.credential)),
          "read",
        )
      )
        sendSerialized(ws, serialized);
  };
  const snapshot = broadcast;
  const editor = broadcast;
  controller.on("snapshot", snapshot);
  controller.on("editor", editor);
  const workspaceLedger = new Map();
  wss.on("connection", (ws) => {
    const identity = identities.get(ws);
    const terminal = terminalConnection({
      ws,
      identity,
      permission: () => principal(identity.credential)?.terminals ?? "off",
      management: () =>
        principal(identity.credential)?.workspaceManagement === true,
      chat: () => chatLevel(principal(identity.credential)),
      ledger: workspaceLedger,
      send: (data) => send(ws, data),
      daemons,
      queueLimit: terminalQueueLimit,
    });
    terminals.set(ws, terminal);
    if (trust) terminal.notify();
    const terminalRequestIds = new Set();
    const activeTerminalRequestIds = new Set();
    let outstanding = 0;
    let alive = true;
    ws.on("pong", () => {
      alive = true;
    });
    const heartbeat = setInterval(() => {
      if (!alive) {
        ws.terminate();
        return;
      }
      alive = false;
      ws.ping();
    }, 20000);

    ws.on("message", async (bytes) => {
      if (outstanding >= 16) {
        ws.close(1008, "Too many requests");
        return;
      }
      outstanding++;
      let id = "";
      let terminalRequest = false;
      try {
        const r = JSON.parse(bytes.toString());
        id = typeof r.id === "string" ? r.id : "";
        const current = principal(identities.get(ws)?.credential);
        if (!current) {
          ws.close(4001, "Device trust revoked; re-pair deliberately");
          return;
        }
        if (r.clientId !== identities.get(ws).clientId)
          throw new Error("Client identity does not match this connection");
        if (terminalOperation(r.op)) {
          if (r.version !== 1 || !id || id.length > 128)
            throw new Error("Terminal request version and ID required");
          if (terminalRequestIds.has(id) || activeTerminalRequestIds.has(id))
            throw new Error(
              "Duplicate terminal request; uncertain input is never replayed",
            );
          if (terminalRequestIds.size >= 4096)
            terminalRequestIds.delete(terminalRequestIds.values().next().value);
          terminalRequestIds.add(id);
          activeTerminalRequestIds.add(id);
          terminalRequest = true;
        } else chatGate(chatLevel(current), r.op);
        // Re-read live grants: queued chat actions run only if still permitted.
        const permitted = () =>
          chatAllowed(
            chatLevel(principal(identities.get(ws)?.credential)),
            chatOperations.get(r.op),
          );
        const data = await (terminalOperation(r.op)
          ? terminal.request(r)
          : controller.request(r, { allowed: permitted }));
        // A downgrade during the request withholds results it no longer grants.
        if (!terminalOperation(r.op) && !permitted())
          throw new Error("Point Guard chat permission changed");
        send(ws, { type: "receipt", id, ok: true, data });
        if (r.op === "sync") send(ws, data);
      } catch (e) {
        send(ws, { type: "receipt", id, ok: false, error: e.message });
      } finally {
        if (terminalRequest) activeTerminalRequestIds.delete(id);
        outstanding--;
      }
    });
    ws.on("error", () => {});
    ws.on("close", () => {
      terminal.dispose();
      clearInterval(heartbeat);
    });
  });
  const revoked = (deviceId) => {
    for (const ws of wss.clients)
      if (identities.get(ws)?.deviceId === deviceId) {
        terminals.get(ws)?.dispose();
        ws.close(4001, "Device trust revoked; re-pair deliberately");
      }
  };
  const permissions = (deviceId) => {
    for (const ws of wss.clients)
      if (identities.get(ws)?.deviceId === deviceId)
        terminals.get(ws)?.changed();
  };
  trust?.on("permissions", permissions);
  trust?.on("revoked", revoked);
  try {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(port, host, () => resolve(null));
    });
  } catch (error) {
    trust?.off("permissions", permissions);
    trust?.off("revoked", revoked);
    controller.off("snapshot", snapshot);
    controller.off("editor", editor);
    throw error;
  }
  return {
    address: () => server.address(),
    shutdown: async () => {
      trust?.off("permissions", permissions);
      trust?.off("revoked", revoked);
      controller.off("snapshot", snapshot);
      controller.off("editor", editor);
      for (const ws of wss.clients) {
        terminals.get(ws)?.dispose();
        ws.terminate();
      }
      await new Promise((resolve) =>
        wss.close(() => server.close(() => resolve(null))),
      );
    },
  };
}
