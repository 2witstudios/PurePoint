import { openTrustStore } from "../bridge/trust.js";
import { serve } from "../bridge/network.js";
import { EventEmitter } from "node:events";
import { writeFile, readFile } from "node:fs/promises";
import path from "node:path";
const directory = process.argv[2];
if (!directory?.startsWith("/tmp/pg-mobile-trust-"))
  throw new Error("Isolated verification directory required");
const trust = await openTrustStore({
  directory: path.join(directory, "trust"),
});
const controller = new EventEmitter();
let requests = 0;
let prompts = 0;
controller.request = async (r) => {
  requests++;
  if (r.op === "send") prompts++;
  return {
    type: "snapshot",
    version: 1,
    epoch: "test",
    revision: requests,
    busy: false,
    sessionId: "fixture",
    title: "Trust check",
    messages: [],
    tools: [],
    queue: [],
    dialogs: [],
    notices: [],
  };
};
const server = await serve(controller, {
  host: "127.0.0.1",
  port: 0,
  trust,
  tls: trust.tls,
});
const endpoint = `wss://127.0.0.1:${server.address().port}/v1`;
const enrollment = trust.createEnrollment({ endpoint });
const device = await trust.enroll({
  enrollmentToken: JSON.parse(enrollment.payload).enrollmentToken,
  name: "Verification phone",
});
const record = {
  ...device,
  endpoint,
  certificateSHA256: trust.certificateSHA256,
};
await writeFile(path.join(directory, "record.json"), JSON.stringify(record), {
  mode: 0o600,
});
let commands = "";
process.stdin.on("data", async (bytes) => {
  commands += bytes;
  if (commands.includes("revoke\n")) {
    commands = commands.replace("revoke\n", "");
    await trust.revokeDevice(device.deviceId);
    console.log("revoked");
  }
  if (commands.includes("count\n")) {
    commands = commands.replace("count\n", "");
    console.log(`requests=${requests};prompts=${prompts}`);
  }
  if (commands.includes("stop\n")) {
    await server.shutdown();
    await trust.close();
    process.exit(0);
  }
});
let revoking = false;
const timer = setInterval(async () => {
  if (revoking) return;
  try {
    await readFile(path.join(directory, "revoke"));
    revoking = true;
    await trust.revokeDevice(device.deviceId);
    await writeFile(path.join(directory, "revoked"), "done");
  } catch {}
}, 20);
timer.unref();
console.log("ready");
