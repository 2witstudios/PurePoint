import { pathToFileURL } from "node:url";
import https from "node:https";
import net from "node:net";
import WebSocket, { WebSocketServer } from "ws";
import { EventEmitter } from "node:events";
import { writeFile, access, mkdir } from "node:fs/promises";
import path from "node:path";
// Optional bridge override exercises an integrated checkout without modifying it.
const bridgeRoot = process.env.PHONE_CONTROL_BRIDGE_ROOT;
if (bridgeRoot && !path.isAbsolute(bridgeRoot)) throw new Error("Absolute verification bridge root required");
const bridgeURL = bridgeRoot ? pathToFileURL(path.join(bridgeRoot, "network.js")) : new URL("../bridge/network.js", import.meta.url);
const { serve } = await import(bridgeURL.href);
const { openTrustStore } = await import(new URL("trust.js", bridgeURL).href);
const { createDaemons } = await import(new URL("daemon.js", bridgeURL).href);
const directory = process.argv[2];
if (!directory?.startsWith("/tmp/pg-machine-network-")) throw new Error("Isolated verification required");
const requests = [];
const fixtures = [];
async function exists(name) { try { await access(path.join(directory, name)); return true; } catch { return false; } }
async function held(name) {
  if (!await exists(`hold-${name}`)) return;
  await writeFile(path.join(directory, `started-${name}`), "started");
  while (!await exists(`release-${name}`)) await new Promise(resolve => setTimeout(resolve, 10));
}
for (const name of ["A", "B"]) {
  const trust = await openTrustStore({ directory: path.join(directory, name) });
  const controller = new EventEmitter();
  let revision = 0;
  controller.request = async r => {
    requests.push({ machine: name, op: r.op, clientId: r.clientId, text: r.text ?? null });
    await writeFile(path.join(directory, "requests.json"), JSON.stringify(requests));
    if (r.op !== "sync") await held(`${name}-${r.op}`);
    if (r.op === "sessions") return { sessions: [{ id: `${name}-session`, title: `${name} history` }] };
    if (r.op === "history") return { sessionId: `${name}-session`, title: name, messages: [{ id: "old", role: "assistant", text: `${name} history` }] };
    if (r.op === "send") return { disposition: "queued" };
    if (r.op === "answer" || r.op === "stop") return {};
    return { type: "snapshot", version: 1, epoch: "same-epoch", revision: ++revision, busy: false, sessionId: `${name}-session`, title: name, messages: [{ id: "same", role: "assistant", text: `${name} transcript` }], tools: [], queue: [], dialogs: [{ id: "same-dialog", method: "confirm", title: `${name} dialog` }], notices: [], canceled: [{ id: "same-canceled", clientId: r.clientId, text: `${name} canceled`, sessionId: `${name}-session` }] };
  };
  // Delay inventory at the real daemon socket boundary, after final bridge dispatch.
  const daemonHome = path.join(directory, `${name}-daemon`);
  await mkdir(daemonHome, { mode: 0o700 });
  const daemonSockets = new Set();
  const daemon = net.createServer(socket => {
    daemonSockets.add(socket);
    socket.on("close", () => daemonSockets.delete(socket));
    socket.on("error", () => {});
    let pending = "";
    socket.on("data", bytes => {
      pending += bytes.toString();
      let end;
      while ((end = pending.indexOf("\n")) >= 0) {
        const request = JSON.parse(pending.slice(0, end)); pending = pending.slice(end + 1);
        void (async () => {
          if (request.type === "health") socket.write(JSON.stringify({ type: "health_report", protocol_version: 10 }) + "\n");
          else if (request.type === "inventory") {
            await held(`${name}-agents`);
            if (!socket.destroyed) socket.write(JSON.stringify({ type: "inventory", complete: true, agents: [] }) + "\n");
          }
        })();
      }
    });
  });
  await new Promise(resolve => daemon.listen(path.join(daemonHome, "daemon.sock"), resolve));
  const server = await serve(controller, { host: "127.0.0.1", port: 0, trust, tls: trust.tls, daemons: createDaemons({ puHome: daemonHome }) });
  // A pinned TLS/WSS relay holds permission *requests* at the network boundary.
  // The final bridge still authenticates and handles permissions itself; no fake
  // chat-controller permissions handler can accidentally bypass that dispatch.
  const relay = https.createServer(trust.tls, (request, response) => {
    const upstream = https.request({ hostname: "127.0.0.1", port: server.address().port, path: request.url, method: request.method, headers: request.headers, rejectUnauthorized: false }, result => {
      response.writeHead(result.statusCode, result.headers); result.pipe(response);
    });
    upstream.on("error", () => { response.writeHead(502); response.end(); });
    request.pipe(upstream);
  });
  const relayWss = new WebSocketServer({ server: relay });
  const upstreams = new Set();
  relayWss.on("connection", (phone, request) => {
    const upstream = new WebSocket(`wss://127.0.0.1:${server.address().port}/v1`, { rejectUnauthorized: false, headers: { Authorization: request.headers.authorization, "X-PointGuard-Client-ID": request.headers["x-pointguard-client-id"] } });
    upstreams.add(upstream);
    const opened = new Promise(resolve => upstream.once("open", resolve));
    phone.on("message", bytes => {
      void (async () => {
        await opened;
        if (JSON.parse(bytes.toString()).op === "permissions") await held(`${name}-permissions`);
        if (upstream.readyState === WebSocket.OPEN) upstream.send(bytes.toString());
      })();
    });
    upstream.on("message", bytes => { if (phone.readyState === WebSocket.OPEN) phone.send(bytes.toString()); });
    upstream.on("close", (code, reason) => { upstreams.delete(upstream); if (phone.readyState === WebSocket.OPEN) phone.close(code === 1006 ? 1011 : code, reason); });
    upstream.on("error", () => phone.close(1011, "Fixture upstream unavailable"));
    phone.on("error", () => {});
    phone.on("close", () => upstream.terminate());
  });
  await new Promise(resolve => relay.listen(0, "127.0.0.1", resolve));
  const endpoint = `wss://127.0.0.1:${relay.address().port}/v1`;
  const enrollment = trust.createEnrollment({ endpoint });
  const device = await trust.enroll({ enrollmentToken: JSON.parse(enrollment.payload).enrollmentToken, name: "Fixture phone" });
  await trust.setTerminals(device.deviceId, "control");
  await writeFile(path.join(directory, `${name}.json`), JSON.stringify({ ...device, endpoint, certificateSHA256: trust.certificateSHA256 }));
  await writeFile(path.join(directory, `${name}-used.json`), enrollment.payload);
  await writeFile(path.join(directory, `${name}-qr.json`), trust.createEnrollment({ endpoint }).payload);
  const originalEnroll = trust.enroll.bind(trust);
  trust.enroll = async r => { await held(`${name}-enrollment`); return originalEnroll(r); };
  fixtures.push({ trust, server, device, controller, async closeRelay() {
    for (const socket of upstreams) socket.terminate();
    for (const socket of relayWss.clients) socket.terminate();
    await new Promise(resolve => relayWss.close(() => relay.close(resolve)));
    for (const socket of daemonSockets) socket.destroy();
    await new Promise(resolve => daemon.close(resolve));
  } });
}
let command = "";
process.stdin.on("data", async bytes => {
  command += bytes;
  if (command.includes("stop\n")) {
    for (const f of fixtures) { await f.closeRelay(); await f.server.shutdown(); await f.trust.close(); }
    process.exit(0);
  }
});
const timer = setInterval(async () => {
  for (const [index, name] of ["A", "B"].entries()) {
    if (await exists(`revoke-${name}`)) { await fixtures[index].trust.revokeDevice(fixtures[index].device.deviceId); }
    if (await exists(`broadcast-${name}`)) fixtures[index].controller.emit("editor", { type: "editor", id: "late", text: `${name} late editor` });
  }
}, 30);
timer.unref();
console.log("ready");
