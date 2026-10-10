import path from "node:path";
import { Rpc } from "./rpc.js";
import { Controller } from "./controller.js";
import { serve } from "./network.js";
import { root, ensureToken } from "./setup.js";

async function main() {
  if (!process.argv.includes("--fixture")) {
    const { runManaged } = await import("./runtime.js");
    await runManaged();
    return;
  }
  // Explicit loopback-only developer fixture; never falls back from managed startup.
  if (process.env.PI_MOBILE_HOST && process.env.PI_MOBILE_HOST !== "127.0.0.1")
    throw new Error("Fixtures require loopback.");
  const token = await ensureToken(process.env.PI_MOBILE_TOKEN_FILE);
  const rpc = new Rpc(
    process.execPath,
    [path.join(root, "bridge/fixture.js")],
    root,
    process.env,
  );
  const sessions = {
    list: async () => [],
    path: async () => {
      throw new Error("Fixture session unavailable.");
    },
    history: async () => {
      throw new Error("Fixture history unavailable.");
    },
  };
  const controller = new Controller(rpc, sessions);
  let server;
  try {
    await controller.refresh();
    server = await serve(controller, {
      host: "127.0.0.1",
      port: Number(process.env.PI_MOBILE_PORT ?? 8787),
      localAdmin: { token, clientId: "fixture" },
    });
  } catch (e) {
    controller.dispose();
    await rpc.close();
    throw e;
  }
  const shutdown = async () => {
    controller.dispose();
    await server.shutdown();
    await rpc.close();
    process.exit(0);
  };
  process.once("SIGTERM", shutdown);
  process.once("SIGINT", shutdown);
}
main().catch(() => {
  console.error("Point Guard setup failed. Inspect private setup status.");
  process.exitCode = 1;
});
