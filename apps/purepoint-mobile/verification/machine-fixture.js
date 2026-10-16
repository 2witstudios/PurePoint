import { openTrustStore } from "../bridge/trust.js";
import { serve } from "../bridge/network.js";
import { EventEmitter } from "node:events";
import { writeFile, access } from "node:fs/promises";
import path from "node:path";
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
    if (r.op === "permissions") return { terminals: await exists(`control-${name}`) ? "control" : "off" };
    if (r.op === "agents") return { machines: [] };
    if (r.op === "sessions") return { sessions: [{ id: `${name}-session`, title: `${name} history` }] };
    if (r.op === "history") return { sessionId: `${name}-session`, title: name, messages: [{ id: "old", role: "assistant", text: `${name} history` }] };
    if (r.op === "send") return { disposition: "queued" };
    if (r.op === "answer" || r.op === "stop") return {};
    return { type: "snapshot", version: 1, epoch: "same-epoch", revision: ++revision, busy: false, sessionId: `${name}-session`, title: name, messages: [{ id: "same", role: "assistant", text: `${name} transcript` }], tools: [], queue: [], dialogs: [{ id: "same-dialog", method: "confirm", title: `${name} dialog` }], notices: [], canceled: [{ id: "same-canceled", clientId: r.clientId, text: `${name} canceled`, sessionId: `${name}-session` }] };
  };
  const server = await serve(controller, { host: "127.0.0.1", port: 0, trust, tls: trust.tls });
  const endpoint = `wss://127.0.0.1:${server.address().port}/v1`;
  const enrollment = trust.createEnrollment({ endpoint });
  const device = await trust.enroll({ enrollmentToken: JSON.parse(enrollment.payload).enrollmentToken, name: "Fixture phone" });
  await writeFile(path.join(directory, `${name}.json`), JSON.stringify({ ...device, endpoint, certificateSHA256: trust.certificateSHA256 }));
  await writeFile(path.join(directory, `${name}-used.json`), enrollment.payload);
  await writeFile(path.join(directory, `${name}-qr.json`), trust.createEnrollment({ endpoint }).payload);
  const originalEnroll = trust.enroll.bind(trust);
  trust.enroll = async r => { await held(`${name}-enrollment`); return originalEnroll(r); };
  fixtures.push({ trust, server, device, controller });
}
let command = "";
process.stdin.on("data", async bytes => {
  command += bytes;
  if (command.includes("stop\n")) {
    for (const f of fixtures) { await f.server.shutdown(); await f.trust.close(); }
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
