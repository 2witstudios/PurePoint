import { readFile, realpath, access } from "node:fs/promises";
import { constants } from "node:fs";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";
import {
  getAgentDir,
  DefaultPackageManager,
  SettingsManager,
  loadSkills,
} from "@earendil-works/pi-coding-agent";
import { seedPersonal, personalSections } from "../lib/personal.js";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const personalDirectory = () =>
  process.env.POINT_GUARD_PERSONAL_DIR ??
  path.join(
    os.homedir(),
    "Library/Application Support/PurePoint/PointGuard/personal",
  );
const workflows = () =>
  process.env.POINT_GUARD_WORKFLOW_SOURCE ??
  path.resolve(packageRoot, "../pointguard-workflows");
async function executable(name) {
  for (const folder of (process.env.PATH ?? "")
    .split(path.delimiter)
    .filter(Boolean)) {
    const candidate = path.join(folder, name);
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {
      /* absent from managed PATH */
    }
  }
  return null;
}

/**
 * Pi 1.1.0 factory: no watchers, processes, prompt replacement or custom TUI.
 * @param {import("@earendil-works/pi-coding-agent").ExtensionAPI} pi
 */
export default function pointguard(pi) {
  const prepare = () =>
    seedPersonal(personalDirectory(), path.join(packageRoot, "defaults"));
  pi.on("session_start", prepare);
  pi.on("before_agent_start", async (event) => {
    await prepare();
    Object.assign(
      event.systemPromptOptions.sections,
      await personalSections(personalDirectory()),
      {
        pointguard_operating: await readFile(
          path.resolve(packageRoot, "../../docs/point-guard.md"),
          "utf8",
        ),
      },
    );
  });
  pi.registerCommand("pointguard", {
    description:
      "Point Guard personal files, package/capability diagnostics, and native reload",
    handler: async (args, ctx) => {
      const action = args.trim() || "info";
      if (action === "reload") {
        await ctx.waitForIdle();
        await prepare();
        ctx.ui.notify(
          "Reloading Point Guard through Pi; current personal files apply at the next run boundary.",
          "info",
        );
        await ctx.reload();
        return; // The old extension/context is invalid after reload.
      }
      if (action !== "info") {
        ctx.ui.notify("Use /pointguard info or /pointguard reload.", "warning");
        return;
      }
      await prepare();
      const source = await realpath(workflows());
      const manifest = JSON.parse(
        await readFile(path.join(source, "package.json"), "utf8"),
      );
      let provenance;
      try {
        const pinned = JSON.parse(
          await readFile(path.join(source, "source-manifest.json"), "utf8"),
        );
        provenance = {
          repository: pinned.repository,
          commit: pinned.commit,
          files: Object.keys(pinned.files ?? {}).length,
        };
      } catch {
        provenance =
          "No source-manifest.json; explicitly selected local package.";
      }
      const dependencies = Object.fromEntries(
        await Promise.all(
          [
            "pu",
            "pagespace",
            "gh",
            "node",
            "agent-browser",
            "playwright",
            "qmd",
          ].map(async (name) => [name, await executable(name)]),
        ),
      );
      // This is the resolved native catalog, not a second discovery registry.
      const skills = ctx.getSystemPromptOptions().skills ?? [];
      // Native diagnostic resolution only: never install missing packages or run
      // their factories. Current live winners come first, preserving native precedence.
      const agentDir = getAgentDir();
      const manager = new DefaultPackageManager({
        cwd: ctx.cwd,
        agentDir,
        settingsManager: SettingsManager.create(ctx.cwd, agentDir, {
          projectTrusted: ctx.isProjectTrusted(),
        }),
      });
      const configured = await manager.resolve(async () => "skip");
      const managed = await manager.resolveExtensionSources(
        [packageRoot, source],
        { temporary: true },
      );
      const nativeDiagnostics = loadSkills({
        cwd: ctx.cwd,
        agentDir,
        includeDefaults: false,
        skillPaths: [
          ...skills.map((skill) => skill.filePath),
          ...managed.skills
            .filter((skill) => skill.enabled)
            .map((skill) => skill.path),
          ...configured.skills
            .filter((skill) => skill.enabled)
            .map((skill) => skill.path),
        ],
      }).diagnostics;
      const report = {
        personalDirectory: personalDirectory(),
        nodeExecutable: process.execPath,
        files: ["SOUL.md", "USER.md"].map((name) =>
          path.join(personalDirectory(), name),
        ),
        workflowPackage: {
          source,
          name: manifest.name,
          version: manifest.version,
          provenance,
        },
        activeTools: pi.getActiveTools(),
        registeredTools: pi
          .getAllTools()
          .map(({ name, exposure, namespace }) => ({
            name,
            exposure,
            namespace: namespace?.name,
          })),
        skills: skills.map(({ name, filePath, sourceInfo }) => ({
          name,
          filePath,
          sourceInfo,
        })),
        dependencies,
        nativeManagedDiagnostics: nativeDiagnostics,
        diagnosticsScope:
          "Live native catalog winners compared with current native configured and managed resource paths using Pi APIs. Missing packages are skipped, never installed; reload to activate edits.",
        limitations:
          "A skill is guidance, not a capability. Missing CLI entries are unavailable on this PATH. MCP/browser/desktop tools depend on explicit native configuration and host permissions. Pi owns duplicate-name resolution; nativeManagedDiagnostics reports conflicts against managed packages. No credentials are inspected.",
      };
      pi.sendMessage(
        {
          customType: "pointguard-info",
          content: JSON.stringify(report, null, 2),
          display: true,
        },
        { triggerTurn: false },
      );
    },
  });
}
