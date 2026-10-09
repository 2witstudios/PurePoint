import { AgentSession } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js";
import { installQueueCorrelation } from "./native-queue.js";

installQueueCorrelation(AgentSession);
await import("../node_modules/@earendil-works/pi-coding-agent/dist/cli.js");
