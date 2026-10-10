import http from "node:http";
import https from "node:https";
import { timingSafeEqual } from "node:crypto";
import { WebSocketServer, WebSocket } from "ws";
import { allowedHost } from "./endpoints.js";
export { allowedHost } from "./endpoints.js";
function authenticated(header, token) {
  if (typeof header !== "string") return false;
  const actual = Buffer.from(header);
  const expected = Buffer.from(`Bearer ${token}`);
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}
export async function serve(
  controller,
  { host, port, trust = null, localAdmin = null, tls = null },
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
        ? { clientId: localAdmin.clientId }
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
        identity ? { version: 1, hostId: trust.hostId, ...identity } : null,
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
          error: "Enrollment unavailable. Scan a new Mac QR code.",
        });
    } finally {
      req.setTimeout(0);
      pairingRequests--;
    }
  };
  const server = tls
    ? https.createServer(tls, handler)
    : http.createServer(handler);
  const wss = new WebSocketServer({
    noServer: true,
    maxPayload: 1024 * 1024,
    perMessageDeflate: false,
  });

  const identities = new WeakMap();
  server.on("upgrade", (req, socket, head) => {
    let code = 401;
    if (req.url !== "/v1" || req.headers.origin) code = 403;
    else {
      const identity = principal(req.headers.authorization);
      if (identity) {
        const clientId = req.headers["x-pointguard-client-id"];
        if (clientId !== identity.clientId) code = 403;
        else if (wss.clients.size >= 32) code = 503;
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
      if (principal(identities.get(ws)?.credential))
        sendSerialized(ws, serialized);
  };
  const snapshot = broadcast;
  const editor = broadcast;
  controller.on("snapshot", snapshot);
  controller.on("editor", editor);
  wss.on("connection", (ws) => {
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
      try {
        const r = JSON.parse(bytes.toString());
        id = typeof r.id === "string" ? r.id : "";
        if (!principal(identities.get(ws)?.credential)) {
          ws.close(4001, "Device trust revoked; re-pair deliberately");
          return;
        }
        if (r.clientId !== identities.get(ws).clientId)
          throw new Error("Client identity does not match this connection");
        const data = await controller.request(r);
        send(ws, { type: "receipt", id, ok: true, data });
        if (r.op === "sync") send(ws, data);
      } catch (e) {
        send(ws, { type: "receipt", id, ok: false, error: e.message });
      } finally {
        outstanding--;
      }
    });
    ws.on("error", () => {});
    ws.on("close", () => {
      clearInterval(heartbeat);
    });
  });
  const revoked = (deviceId) => {
    for (const ws of wss.clients)
      if (identities.get(ws)?.deviceId === deviceId)
        ws.close(4001, "Device trust revoked; re-pair deliberately");
  };
  trust?.on("revoked", revoked);
  try {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(port, host, () => resolve(null));
    });
  } catch (error) {
    trust?.off("revoked", revoked);
    controller.off("snapshot", snapshot);
    controller.off("editor", editor);
    throw error;
  }
  return {
    address: () => server.address(),
    shutdown: async () => {
      trust?.off("revoked", revoked);
      controller.off("snapshot", snapshot);
      controller.off("editor", editor);
      for (const ws of wss.clients) ws.terminate();
      await new Promise((resolve) =>
        wss.close(() => server.close(() => resolve(null))),
      );
    },
  };
}
