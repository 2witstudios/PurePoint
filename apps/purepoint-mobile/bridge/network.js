import http from "node:http";
import https from "node:https";
import { timingSafeEqual } from "node:crypto";
import { isIP } from "node:net";
import { WebSocketServer, WebSocket } from "ws";
export function allowedHost(host) {
  if (host === "127.0.0.1" || host === "::1") return true;
  if (isIP(host) === 4) {
    const p = host.split(".").map(Number);
    return p[0] === 100 && p[1] >= 64 && p[1] <= 127;
  }
  return isIP(host) === 6 && host.toLowerCase().startsWith("fd7a:115c:a1e0:");
}
function authenticated(header, token) {
  if (typeof header !== "string") return false;
  const actual = Buffer.from(header);
  const expected = Buffer.from(`Bearer ${token}`);
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}
export async function serve(controller, { host, port, token, tls = null }) {
  if (!allowedHost(host))
    throw new Error(
      "Bind to an explicit Tailscale IP or loopback address. Public/wildcard addresses are refused.",
    );
  const handler = (_req, res) => {
    res.writeHead(404);
    res.end();
  };
  const server = tls
    ? https.createServer(tls, handler)
    : http.createServer(handler);
  const wss = new WebSocketServer({
    noServer: true,
    maxPayload: 1024 * 1024,
    perMessageDeflate: false,
  });
  let active = null;
  server.on("upgrade", (req, socket, head) => {
    let code = 401;
    if (req.url !== "/v1" || req.headers.origin) code = 403;
    else if (authenticated(req.headers.authorization, token)) {
      if (active && active.readyState !== WebSocket.CLOSED) code = 409;
      else code = 101;
    }
    if (code !== 101) {
      socket.end(
        `HTTP/1.1 ${code} Rejected\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`,
      );
      return;
    }
    wss.handleUpgrade(req, socket, head, (ws) => {
      active = ws;
      wss.emit("connection", ws);
    });
  });
  function send(ws, data) {
    const serialized = JSON.stringify(data);
    if (
      Buffer.byteLength(serialized) > 4 * 1024 * 1024 ||
      ws.bufferedAmount > 4 * 1024 * 1024
    ) {
      ws.close(1009, "Output backlog; reconnect to refresh");
      return;
    }
    if (ws.readyState === WebSocket.OPEN) ws.send(serialized);
  }
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
    const snapshot = (s) => send(ws, s);
    const editor = (e) => send(ws, e);
    controller.on("snapshot", snapshot);
    controller.on("editor", editor);
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
      controller.off("snapshot", snapshot);
      controller.off("editor", editor);
      if (active === ws) active = null;
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, () => resolve(null));
  });
  return {
    address: () => server.address(),
    shutdown: async () => {
      for (const ws of wss.clients) ws.terminate();
      await new Promise((resolve) =>
        wss.close(() => server.close(() => resolve(null))),
      );
    },
  };
}
