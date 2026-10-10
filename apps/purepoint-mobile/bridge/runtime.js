import os from "node:os";
import path from "node:path";
import { access, lstat, rm } from "node:fs/promises";
import { constants } from "node:fs";
import { randomUUID } from "node:crypto";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { serve } from "./network.js";
import { launchArguments, nativeSessions, ensureToken } from "./setup.js";
import { openRuntimeState, writePrivateJSON } from "./runtime-state.js";
import { nativeProviderSetup } from "./native-provider.js";
import { serveAdmin, AdminError } from "./admin.js";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function idle(c) {
  return (
    !c.busy &&
    !c.state.isStreaming &&
    !c.pendingMutations &&
    !c.pendingEnqueue &&
    !c.queue.length &&
    !c.dialogs.size
  );
}
const endpoint = (scheme, host, server) =>
  `${scheme}://${host.includes(":") ? "[" + host + "]" : host}:${server.address().port}/v1`;
/** Wait for the exact owned RPC child, including the legacy close timeout path. */
async function closeOwnedRpc(rpc) {
  const child = rpc.child;
  await rpc.close();
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise((resolve) => {
    const timer = setTimeout(() => child.kill("SIGKILL"), 3000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve(null);
    });
    if (child.exitCode !== null || child.signalCode !== null) {
      clearTimeout(timer);
      resolve(null);
    }
  });
}
/** App-lifetime managed service; never adopts or signals a process from a readiness file. */
export async function runManaged(env = process.env) {
  const directory =
    env.POINT_GUARD_STATE_DIR ??
    path.join(os.homedir(), "Library/Application Support/PurePoint/PointGuard");
  const instanceId = env.POINT_GUARD_INSTANCE_ID ?? randomUUID();
  if (!uuid.test(instanceId))
    throw new Error("Invalid Point Guard launch identity.");
  const state = await openRuntimeState(
    directory,
    env.POINT_GUARD_CLIENT_ID,
    instanceId,
  );
  let rpc, controller, trust, provider, admin, local, remote;
  let stopped = false;
  let restarting = false;
  let persistError = false;
  const descriptorFile = path.join(directory, "admin.json");
  const persist = async () => {
    if (!controller) return;
    const s = controller.state;
    await state.save({
      ...(s.sessionFile ? { selectedSessionPath: s.sessionFile } : {}),
      ...(s.model ? { provider: s.model.provider, model: s.model.id } : {}),
    });
  };
  const shutdown = async () => {
    if (stopped) return;
    stopped = true;
    restarting = true;
    provider?.close();
    controller?.dispose();
    // Listener closure stops new input before the owned child exits. No prompt is replayed.
    await Promise.all([
      local?.shutdown(),
      remote?.shutdown(),
      admin?.shutdown(),
    ]);
    if (rpc) await closeOwnedRpc(rpc);
    await persist().catch(() => {});
    await trust?.close();
    await rm(descriptorFile, { force: true });
    await state.close();
  };
  try {
    await rm(descriptorFile, { force: true });
    await rm(path.join(directory, "error.json"), { force: true });
    const pu = env.POINT_GUARD_PU_PATH;
    if (!pu || !path.isAbsolute(pu))
      throw new Error(
        "Bundled pu CLI path is missing. Reinstall the complete PurePoint app.",
      );
    await access(pu, constants.X_OK);
    const cwd = env.PI_MOBILE_CWD
      ? path.resolve(env.PI_MOBILE_CWD)
      : (state.value.cwd ?? os.homedir());
    if (!(await lstat(cwd)).isDirectory())
      throw new Error(
        "Selected working folder is unavailable. Choose an existing folder in Point Guard setup.",
      );
    await state.save({ cwd });
    const adminToken = await ensureToken(path.join(directory, "admin-token"));
    const chatToken = await ensureToken(
      path.join(directory, "desktop-chat-token"),
    );
    if (adminToken === chatToken)
      throw new Error(
        "Local capabilities collide. Restore separate private owner credentials.",
      );
    provider = await nativeProviderSetup(state.value.desktopClientId);
    const { openTrustStore } = await import("./trust.js");
    trust = await openTrustStore({ directory: path.join(directory, "trust") });
    const childEnv = {
      ...env,
      PATH: [path.dirname(pu), "/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(
        path.delimiter,
      ),
      POINT_GUARD_PU_PATH: pu,
    };
    // Never expose local owner service bootstrap configuration to extensions/tools.
    for (const key of Object.keys(childEnv))
      if (key.startsWith("POINT_GUARD_") && key !== "POINT_GUARD_PU_PATH")
        delete childEnv[key];
    const args = await launchArguments(cwd, {
      ...env,
      PI_MOBILE_SESSION: state.value.selectedSessionPath,
    });
    rpc = new Rpc(process.execPath, args, cwd, childEnv);
    controller = new Controller(rpc, await nativeSessions());
    await controller.refresh();
    if (state.value.provider && state.value.model) {
      await rpc.call("set_model", {
        provider: state.value.provider,
        modelId: state.value.model,
      });
      await controller.refresh();
    }
    const { commands } = await rpc.call("get_commands");
    for (const name of ["skill:pu", "skill:pu-cli"])
      if (!commands.some((c) => c.source === "skill" && c.name === name))
        throw new Error(
          "Packaged Point Guard support is incomplete. Reinstall the complete app.",
        );
    await persist();
    const originalRequest = controller.request.bind(controller);
    controller.request = async (request) => {
      if (
        (restarting || persistError) &&
        !["sync", "sessions", "history"].includes(request.op)
      )
        throw new Error(
          "Point Guard needs an owned restart. Inspect history; uncertain input will not be replayed.",
        );
      const result = await originalRequest(request);
      if (["new", "resume"].includes(request.op)) await persist();
      return result;
    };
    controller.on("snapshot", () => {
      persist().catch(() => {
        persistError = true;
        controller.notice(
          "Session selection could not be saved. Restore private state before restarting.",
        );
      });
    });
    local = await serve(controller, {
      host: "127.0.0.1",
      port: 0,
      localAdmin: { token: chatToken, clientId: state.value.desktopClientId },
    });
    const nativeChatURL = endpoint("ws", "127.0.0.1", local);
    let chatURL = null;
    if (env.PI_MOBILE_HOST) {
      const port = Number(env.PI_MOBILE_PORT ?? 8787);
      if (!Number.isInteger(port) || port < 1 || port > 65535)
        throw new Error("Invalid phone listener port.");
      remote = await serve(controller, {
        host: env.PI_MOBILE_HOST,
        port,
        trust,
        tls: trust.tls,
      });
      chatURL = endpoint("wss", env.PI_MOBILE_HOST, remote);
    }
    const status = () => ({
      phase: controller.error || persistError ? "failed" : "ready",
      instanceId,
      pid: process.pid,
      contractVersion: 1,
      nativeChatURL,
      chatURL,
      hostId: trust.hostId,
      certificateSHA256: trust.certificateSHA256,
      desktopClientId: state.value.desktopClientId,
      sessionId: controller.state.sessionId,
      provider: controller.state.model?.provider,
      model: controller.state.model?.id,
      cwd,
      ...(controller.error
        ? {
            recovery:
              "Pi stopped or transport failed. Inspect history, then restart the owned service. No uncertain prompt will be replayed.",
          }
        : {}),
    });
    const exclusive = async (action) => {
      if (!idle(controller))
        throw new AdminError(
          "busy",
          "Wait for Pi and its queued work to finish before applying setup changes.",
        );
      controller.pendingMutations++;
      const operation = controller.serial.then(action);
      controller.serial = operation.catch(() => {});
      try {
        return await operation;
      } finally {
        controller.pendingMutations--;
      }
    };
    admin = await serveAdmin({
      token: adminToken,
      dispatch: async (r) => {
        switch (r.operation) {
          case "status":
            return status();
          case "providers":
            return provider.list();
          case "auth.start":
            return provider.start(r.provider, r.type);
          case "auth.status":
            return provider.status(r.attemptId);
          case "auth.respond":
            return provider.respond(r.attemptId, r.promptId, r.value);
          case "auth.cancel":
            return provider.cancel(r.attemptId);
          case "model.select":
            return exclusive(async () => {
              provider.cancelActive();
              await rpc.call("set_model", {
                provider: r.provider,
                modelId: r.model,
              });
              await controller.refresh();
              await persist();
              return {};
            });
          case "runtime.configure":
            return exclusive(async () => {
              if (
                typeof r.cwd !== "string" ||
                !path.isAbsolute(r.cwd) ||
                !(await lstat(r.cwd)).isDirectory()
              )
                throw new AdminError(
                  "cwd",
                  "Choose an existing absolute working folder.",
                );
              provider.cancelActive();
              await state.save({ cwd: r.cwd });
              return { restartRequired: true };
            });
          case "runtime.restart":
            return exclusive(async () => {
              await persist();
              provider.cancelActive();
              return { restartRequired: true };
            });
          case "runtime.stop":
            return exclusive(async () => {
              restarting = true;
              setImmediate(() => shutdown().then(() => process.exit(0)));
              return {};
            });
          case "pairing.create":
            if (!chatURL)
              throw new AdminError(
                "unreachable",
                "Connect Tailscale and select this Mac’s tailnet address before pairing.",
              );
            if (r.endpoint !== undefined && r.endpoint !== chatURL)
              throw new AdminError(
                "endpoint",
                "Enrollment must use this service’s actual phone endpoint.",
              );
            return trust.createEnrollment({
              endpoint: chatURL,
              ttlSeconds: r.ttlSeconds,
            });
          case "pairing.status":
            return trust.enrollmentStatus(r.enrollmentId);
          case "pairing.revoke":
            trust.revokeEnrollment(r.enrollmentId);
            return {};
          case "devices.list":
            return { devices: trust.listDevices() };
          case "devices.revoke":
            await trust.revokeDevice(r.deviceId);
            return {};
          case "devices.rotate":
            await trust.rotateDevice(r.deviceId);
            return {};
          default:
            throw new AdminError(
              "operation",
              "Unknown administrative operation.",
            );
        }
      },
    });
    await writePrivateJSON(descriptorFile, {
      schemaVersion: 1,
      ...status(),
      adminURL: admin.url,
    });
    const onSignal = () =>
      shutdown()
        .then(() => process.exit(0))
        .catch(() => process.exit(1));
    process.once("SIGTERM", onSignal);
    process.once("SIGINT", onSignal);
    return { shutdown, status };
  } catch (error) {
    await shutdown();
    // Errors from SDK/network are intentionally not persisted verbatim.
    await writePrivateJSON(path.join(directory, "error.json"), {
      schemaVersion: 1,
      code: "startup_failed",
      message: "Point Guard could not start its packaged service.",
      recovery:
        "Check the selected folder, complete app runtime, private Pi state, and any existing listener/startup lock. Quit the owning app before deliberate recovery.",
    });
    throw error;
  }
}
