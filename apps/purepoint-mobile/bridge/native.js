import path from "node:path";
import { accessSync, constants } from "node:fs";

// The app supplies an explicit helper. Never resolve the distributed CLI from developer PATH.
if (process.env.POINT_GUARD_PU_PATH) {
  const pu = process.env.POINT_GUARD_PU_PATH;
  if (!path.isAbsolute(pu))
    throw new Error("Bundled pu path must be absolute.");
  accessSync(pu, constants.X_OK);
  process.env.PATH = [path.dirname(pu), process.env.PATH ?? ""].join(
    path.delimiter,
  );
}

import { AgentSession } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js";
import { installQueueCorrelation } from "./native-queue.js";

installQueueCorrelation(AgentSession);
await import("../node_modules/@earendil-works/pi-coding-agent/dist/cli.js");
