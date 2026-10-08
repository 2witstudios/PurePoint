import path from "node:path";
import { readFile } from "node:fs/promises";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { serve } from "./network.js";
import { root, launchArguments, nativeSessions, loadToken } from "./setup.js";
async function main() {
  const fixture = process.argv.includes("--fixture");
  const env = process.env;
  const cwd = env.PI_MOBILE_CWD ? path.resolve(env.PI_MOBILE_CWD) : null;
  if (!fixture && !cwd)
    throw new Error(
      "Set PI_MOBILE_CWD to the explicit folder Pi should work in.",
    );
  const token = await loadToken(env.PI_MOBILE_TOKEN_FILE);
  const host = env.PI_MOBILE_HOST;
  if (!host)
    throw new Error(
      "Set PI_MOBILE_HOST to this Mac’s explicit Tailscale IP (or 127.0.0.1 for simulator).",
    );
  const port = Number(env.PI_MOBILE_PORT ?? 8787);
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("PI_MOBILE_PORT must be 1–65535.");
  const args = fixture
    ? [path.join(root, "bridge/fixture.js")]
    : await launchArguments(cwd, env);
  const rpc = new Rpc(process.execPath, args, cwd ?? root, env);
  const sessions = fixture
    ? {
        list: async () => [
          {
            id: "fixture-history",
            name: "A previous thought",
            modified: new Date("2026-01-01"),
          },
        ],
        path: async () => "fixture-history",
        history: async () => ({
          sessionId: "fixture-history",
          title: "A previous thought",
          messages: [
            { id: "fixture-old", role: "user", text: "Where should we begin?" },
            {
              id: "fixture-reply",
              role: "assistant",
              text: "With one clear idea.",
            },
          ],
        }),
      }
    : await nativeSessions();
  const controller = new Controller(rpc, sessions);
  let server;
  try {
    await controller.refresh();
    if (!fixture) {
      const { commands } = await rpc.call("get_commands");
      if (
        !commands.some(
          (c) =>
            c.source === "skill" &&
            ["skill:pu", "skill:pu-cli"].includes(c.name),
        )
      )
        throw new Error(
          "Pi did not load the pu skill. Check its frontmatter and native resource diagnostics locally.",
        );
      if (!controller.state.model)
        controller.notice(
          "Configure a provider/model with local Pi before sending a message.",
        );
    }
    const tls =
      env.PI_MOBILE_TLS_CERT && env.PI_MOBILE_TLS_KEY
        ? {
            cert: await readFile(env.PI_MOBILE_TLS_CERT),
            key: await readFile(env.PI_MOBILE_TLS_KEY),
          }
        : null;
    server = await serve(controller, { host, port, token, tls });
    console.log(
      `Pi Mobile ${fixture ? "fixture" : "bridge"} listening on ${tls ? "wss" : "ws"}://${host.includes(":") ? "[" + host + "]" : host}:${port}/v1. One controller; Pi stays running when the phone disconnects.`,
    );
  } catch (e) {
    controller.dispose();
    await rpc.close();
    throw e;
  }
  let exiting = false;
  const shutdown = async () => {
    if (exiting) return;
    exiting = true;
    controller.dispose();
    await server.shutdown();
    await rpc.close();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}
main().catch((error) => {
  console.error(`Setup failed: ${error.message}`);
  process.exitCode = 1;
});
