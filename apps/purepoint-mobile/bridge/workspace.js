import { daemonRequest } from "./daemon.js";

export const workspaceFeatures = Object.freeze({
  version: 1,
  inventory: true,
  management: true,
  operationStatus: true,
});
export const workspaceOperations = [
  "workspace.catalog",
  "workspace.operation",
  "workspace.spawn",
  "workspace.createWorktree",
  "workspace.terminal",
  "workspace.rename",
  "workspace.stop",
];
const fail = (code, message) => {
  throw new Error(`workspace_${code}: ${message}`);
};
const bounded = (value, max) =>
  typeof value === "string" &&
  value.trim().length > 0 &&
  Buffer.byteLength(value) <= max &&
  !/[\x00-\x1f\x7f]/.test(value);
const promptText = (value) =>
  typeof value === "string" &&
  value.trim().length > 0 &&
  Buffer.byteLength(value) <= 65536 &&
  !/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(value);
// Spawn instructions are optional: blank means no instructions.
const optionalPromptText = (value) =>
  typeof value === "string" &&
  Buffer.byteLength(value) <= 65536 &&
  (value.trim().length === 0 || promptText(value));
// Config map keys are opaque strings, not process or filesystem identifiers.
const configKey = (value) =>
  typeof value === "string" &&
  value.length > 0 &&
  Buffer.byteLength(value) <= 128 &&
  !/[\x00-\x1f\x7f-\x9f]/.test(value);
const identifier = (value) =>
  typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
const validOperation = (value) =>
  typeof value === "string" && /^[A-Za-z0-9_-]{8,128}$/.test(value);

/** Metadata failures preserve the independent agents projection. */
export async function projectMetadata(machine, options = {}) {
  const query = async (kind) => {
    const reply = await daemonRequest(
      machine.socket,
      { type: "inventory", kind },
      options,
    );
    if (
      reply.type !== "inventory" ||
      !Array.isArray(reply[kind]) ||
      reply[kind].length > 4096
    )
      fail("unsupported", "Bounded project metadata unavailable");
    return reply;
  };
  const [projects, worktrees] = await Promise.allSettled([
    query("projects"),
    query("worktrees"),
  ]);
  const complete =
    projects.status === "fulfilled" &&
    worktrees.status === "fulfilled" &&
    projects.value.complete === true &&
    worktrees.value.complete === true;
  return {
    metadataStatus: complete ? "complete" : "partial",
    metadataError: complete
      ? null
      : "Project metadata incomplete or unsupported; refresh to retry",
    projects:
      projects.status === "fulfilled"
        ? projects.value.projects.map((p) => ({
            id: p.project_root,
            name: p.name,
            available: p.available === true,
            error: p.error ?? null,
            counts: p.counts,
          }))
        : [],
    worktrees:
      worktrees.status === "fulfilled"
        ? worktrees.value.worktrees.map((w) => ({
            projectId: w.project_root,
            id: w.id,
            name: w.name,
            path: w.path,
            branch: w.branch,
            status: w.status,
            agentIds: w.agent_ids,
          }))
        : [],
  };
}

const fields = {
  "workspace.spawn": [
    "target",
    "worktreeId",
    "name",
    "base",
    "agentType",
    "presetId",
    "prompt",
    "planMode",
  ],
  "workspace.createWorktree": ["name", "base"],
  "workspace.terminal": ["target", "worktreeId"],
  "workspace.rename": ["agentId", "name"],
  "workspace.stop": ["agentId", "confirm"],
};
const fingerprint = (request) =>
  JSON.stringify(
    Object.keys(request)
      .filter((k) => !["id", "clientId", "version"].includes(k))
      .sort()
      .map((k) => [k, request[k]]),
  );

/** Shared process ledger is device-scoped; socket disposal never authorizes replay. */
export function workspaceConnection({
  identity,
  permission,
  management,
  ledger,
  daemons,
}) {
  let disposed = false;
  const pending = new Set();
  const allowed = (mutation) => {
    if (
      disposed ||
      !["view", "control"].includes(permission()) ||
      (mutation && (permission() !== "control" || !management()))
    )
      fail("permission", "Current device grant does not allow this operation");
  };
  const visible = (record) => {
    allowed(false);
    return permission() === "control" && management()
      ? record
      : {
          operationId: record.operationId,
          state: record.state,
          message:
            "Workspace management permission removed; mutation may already have committed",
        };
  };
  const resolve = async (r, signal, check) => {
    if (!identifier(r.machine) || !bounded(r.projectId, 4096))
      fail("invalid", "Exact machine and registered project required");
    const machine = await daemons.resolve(r.machine);
    check();
    const metadata = await projectMetadata(machine, { signal });
    check();
    if (metadata.metadataStatus !== "complete")
      fail("unsupported", "Complete authoritative project metadata required");
    const project = metadata.projects.find((p) => p.id === r.projectId);
    if (!project || !project.available)
      fail("target", "Registered available project required");
    return { machine, metadata, project };
  };
  const catalog = async (machine, projectId, signal, check) => {
    const config = await daemonRequest(
      machine.socket,
      { type: "get_config", project_root: projectId },
      { signal },
    );
    check();
    if (
      config.type !== "config_report" ||
      !Array.isArray(config.agents) ||
      config.agents.length > 256 ||
      typeof config.terminal_available !== "boolean" ||
      config.agents.some(
        (a) => !configKey(a.id) || typeof a.available !== "boolean",
      )
    )
      fail("unsupported", "Daemon lacks canonical workspace catalog");
    const agents = config.agents.map((a) => ({
      id: a.id,
      name: a.name,
      available: a.available,
    }));
    let presets = [],
      complete = true;
    try {
      const defs = await daemonRequest(
        machine.socket,
        { type: "list_agent_defs", project_root: projectId },
        { signal },
      );
      check();
      if (
        defs.type !== "agent_def_list" ||
        !Array.isArray(defs.agent_defs) ||
        defs.agent_defs.length > 256
      )
        throw new Error("Unsupported presets");
      presets = defs.agent_defs
        .filter(
          (d) =>
            !d.command &&
            !d.template &&
            promptText(d.inline_prompt) &&
            identifier(d.name) &&
            agents.some((a) => a.id === d.agent_type && a.available),
        )
        .map((d) => ({
          id: d.name,
          name: d.name,
          agentType: d.agent_type,
          prompt: d.inline_prompt,
        }));
    } catch {
      complete = false;
    }
    check();
    return {
      agents,
      presets,
      terminalAvailable: config.terminal_available,
      complete,
    };
  };
  const request = async (r) => {
    allowed(false);
    const key = `${identity.deviceId ?? identity.clientId}:${r.operationId}`;
    if (r.op === "workspace.operation") {
      if (!validOperation(r.operationId))
        fail("invalid", "Operation ID required");
      return visible(
        ledger.get(key)?.result ?? {
          operationId: r.operationId,
          state: "unknown",
          message:
            "No process-lifetime record; do not resend. Refresh inventory and review before a new launch.",
        },
      );
    }
    const mutation = r.op !== "workspace.catalog";
    allowed(mutation);
    let entry;
    if (mutation) {
      if (!validOperation(r.operationId))
        fail("invalid", "Operation ID requires 8–128 safe ASCII characters");
      const accepted = new Set([
        "version",
        "id",
        "clientId",
        "op",
        "operationId",
        "machine",
        "projectId",
        ...(fields[r.op] ?? []),
      ]);
      if (!fields[r.op] || Object.keys(r).some((k) => !accepted.has(k)))
        fail("invalid", "Unsupported operation fields");
      const payload = fingerprint(r);
      const existing = ledger.get(key);
      if (existing) {
        if (existing.payload !== payload)
          fail("duplicate", "Operation ID belongs to a different form");
        if (existing.result.state === "pending")
          fail(
            "duplicate",
            "Operation in flight; query workspace.operation with the same operationId",
          );
        return visible(existing.result);
      }
      if (ledger.size >= 2048)
        fail("capacity", "Operation ledger full; no mutation dispatched");
      entry = {
        payload,
        result: { operationId: r.operationId, state: "pending" },
      };
      ledger.set(key, entry);
    }
    const abort = new AbortController();
    pending.add(abort);
    let dispatched = false;
    const check = () => {
      allowed(mutation);
      if (abort.signal.aborted) fail("permission", "Operation fenced");
    };
    try {
      const { machine, metadata, project } = await resolve(
        r,
        abort.signal,
        check,
      );
      if (!mutation)
        return await catalog(machine, project.id, abort.signal, check);
      if (r.name !== undefined && !bounded(r.name, 80))
        fail("invalid", "Name requires 1–80 UTF-8 bytes");
      if (
        r.base !== undefined &&
        (!bounded(r.base, 128) ||
          !/^[A-Za-z0-9][A-Za-z0-9_./-]*$/.test(r.base) ||
          r.base.includes(".."))
      )
        fail("invalid", "Base requires a bounded git reference");
      let ipc;
      if (["workspace.rename", "workspace.stop"].includes(r.op)) {
        if (!identifier(r.agentId)) fail("invalid", "Exact agent required");
        const { inventory } = await daemons.inspect(machine, {
          signal: abort.signal,
        });
        check();
        if (
          inventory.complete !== true ||
          !inventory.agents.some(
            (a) => a.id === r.agentId && a.project_root === project.id,
          )
        )
          fail(
            "target",
            "Agent does not belong to this project on this machine",
          );
        if (r.op === "workspace.rename") {
          if (!bounded(r.name, 80)) fail("invalid", "Name required");
          ipc = {
            type: "rename",
            project_root: project.id,
            agent_id: r.agentId,
            name: r.name,
          };
        } else {
          if (r.confirm !== true)
            fail("invalid", "Explicit stop confirmation required");
          ipc = {
            type: "kill",
            project_root: project.id,
            target: { agent: r.agentId },
            exclude: [],
          };
        }
      } else if (r.op === "workspace.createWorktree") {
        if (!bounded(r.name, 80)) fail("invalid", "Worktree name required");
        ipc = {
          type: "create_worktree",
          project_root: project.id,
          name: r.name,
          ...(r.base ? { base: r.base } : {}),
        };
      } else {
        if (
          ![
            "root",
            "worktree",
            ...(r.op === "workspace.spawn" ? ["newWorktree"] : []),
          ].includes(r.target)
        )
          fail("invalid", "Explicit launch target required");
        if (r.target === "worktree") {
          if (
            !identifier(r.worktreeId) ||
            !metadata.worktrees.some(
              (w) =>
                w.projectId === project.id &&
                w.id === r.worktreeId &&
                w.status === "active",
            )
          )
            fail("target", "Exact active project worktree required");
          if (r.base !== undefined)
            fail("invalid", "Existing worktree forbids base");
        } else if (
          r.worktreeId !== undefined ||
          (r.target === "root" && r.base !== undefined)
        )
          fail("invalid", "Root/new target cannot include existing worktree");
        if (r.target === "newWorktree" && !bounded(r.name, 80))
          fail("invalid", "New worktree name required");
        const choices = await catalog(machine, project.id, abort.signal, check);
        let prompt = "",
          agent = "terminal";
        if (r.op === "workspace.spawn") {
          if (
            !configKey(r.agentType) ||
            !choices.agents.some((a) => a.id === r.agentType && a.available)
          )
            fail("unsupported", "Agent unavailable");
          if (
            !optionalPromptText(r.prompt) ||
            (r.planMode !== undefined && typeof r.planMode !== "boolean")
          )
            fail(
              "invalid",
              "Bounded instructions and boolean plan mode required",
            );
          const instructions = r.prompt.trim().length === 0 ? "" : r.prompt;
          prompt = instructions;
          agent = r.agentType;
          if (r.presetId !== undefined) {
            const preset = choices.presets.find(
              (p) => p.id === r.presetId && p.agentType === agent,
            );
            if (!preset)
              fail("target", "Preset unavailable for selected agent");
            prompt = instructions
              ? `${preset.prompt}\n\n${instructions}`
              : preset.prompt;
          }
          if (Buffer.byteLength(prompt) > 65536)
            fail("invalid", "Expanded preset prompt exceeds 65536 UTF-8 bytes");
          // Plan mode needs an effective prompt; the daemon rejects it otherwise.
          if (r.planMode === true && prompt.length === 0)
            fail("invalid", "Plan mode requires instructions or a preset");
        } else if (!choices.terminalAvailable)
          fail("unsupported", "Terminal unavailable");
        ipc = {
          type: "spawn",
          project_root: project.id,
          agent,
          prompt,
          root: r.target === "root",
          ...(r.target === "worktree" ? { worktree: r.worktreeId } : {}),
          ...(r.name ? { name: r.name } : {}),
          ...(r.base ? { base: r.base } : {}),
          plan_mode: r.planMode === true,
        };
      }
      check();
      dispatched = true;
      const reply = await daemonRequest(machine.socket, ipc, {
        signal: abort.signal,
        // Spawn can await screen-confirmed prompt delivery after committing the agent.
        // These are bridge wait caps, not bounds on daemon Git/filesystem work.
        timeout: r.op === "workspace.spawn" ? 360000 : 65000,
      });
      check();
      const expected = {
        "workspace.spawn": "spawn_result",
        "workspace.terminal": "spawn_result",
        "workspace.createWorktree": "create_worktree_result",
        "workspace.rename": "rename_result",
        "workspace.stop": "kill_result",
      }[r.op];
      if (reply.type !== expected) throw new Error("Unexpected daemon receipt");
      entry.result = {
        operationId: r.operationId,
        state: "succeeded",
        detail: {
          ...(reply.agent_id
            ? { agentId: reply.agent_id }
            : r.agentId
              ? { agentId: r.agentId }
              : {}),
          ...([
            "workspace.spawn",
            "workspace.terminal",
            "workspace.createWorktree",
          ].includes(r.op)
            ? { worktreeId: reply.worktree_id ?? null }
            : {}),
        },
      };
    } catch (error) {
      if (!mutation) throw error;
      entry.result = {
        operationId: r.operationId,
        state: dispatched ? "uncertain" : "failed",
        message: dispatched
          ? "Mutation may have committed. Refresh operation status and authoritative inventory; never automatically retry."
          : error.message,
      };
    } finally {
      pending.delete(abort);
    }
    return visible(entry.result);
  };
  return {
    request,
    changed() {
      for (const abort of pending) abort.abort();
    },
    dispose() {
      disposed = true;
      for (const abort of pending) abort.abort();
    },
  };
}
