// The palette's entry points (WIN-7.5), from apps/purepoint-macos/purepoint-macos:
// - `showNewAgentPalette` — AppCommands.swift (File › New Agent…, Ctrl+Shift+N);
// - `openCommandPalette` — SidebarFooter.swift `showCommandPalette` and
//   SidebarOutlineView+Callbacks.swift `onShowCommandPalette` (the sidebar's New and add buttons);
// - `openEmptyTabPalette` — PaneCellView.swift `EmptyTabView.openCommandPalette` (an empty tab, and
//   the tab a split or New Tab creates).
// Each loads the agents hub's definitions (and, for a tab, the files under its root), builds the
// items in the user's order and shows the palette; the choice spawns through the project.
//
// Differences from Swift:
// - The agents added in Settings › Agents (`AgentVariant.addedAgents`) arrive with the settings
//   screen (WIN-8.11); until then the built-ins are Claude, Codex, OpenCode and Terminal.
// - The hub keeps `AgentDefinition` / `SwarmDefinition`; the palette items take the daemon's
//   `AgentDefInfo` / `SwarmDefInfo`, which these are converted back to.
import {
  type CommandPaletteItem,
  type CommandPaletteResult,
  type PaletteFileEntry,
  buildItems,
} from '../../model/commandPalette.ts';
import { allVariants, variantsWithWorktree } from '../../model/agentVariant.ts';
import type { AgentDefinition, SwarmDefinition } from '../../model/agentsHubModels.ts';
import type { SidebarSelection } from '../../model/sidebarItem.ts';
import { isPointGuard, leafIdOfSurface } from '../../model/workspace.ts';
import type { AgentDefInfo, SwarmDefInfo } from '../../protocol';
import { entriesFromListing, fileIndex, remoteListingLimit } from '../../services/fileIndex.ts';
import { type AgentsHubState, agentsHubStateFor } from '../../state/agentsHubState.ts';
import type { AppState } from '../../state/appState.ts';
import { CommandPalettePreferences } from '../../state/commandPalettePreferences.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { browserStorage } from '../../state/storage.ts';
import { requestPaletteOpen, showPalette } from './paletteStore.ts';

/**
 * What the command palette opens for (Swift `onShowCommandPalette`'s project, selection and
 * whether worktree variants and swarms are offered).
 */
export interface CommandPaletteRequest {
  readonly projectId: string;
  readonly selection: SidebarSelection | null;
  readonly includeWorktree: boolean;
}

// MARK: Preferences

let preferences: CommandPalettePreferences | null = null;

/** Swift `settingsState`'s palette preferences, in `localStorage` (memory when unavailable). */
export function palettePreferences(): CommandPalettePreferences {
  if (preferences === null) {
    const memory = new Map<string, string>();
    const storage = browserStorage() ?? {
      getItem: (key: string) => memory.get(key) ?? null,
      setItem: (key: string, value: string) => {
        memory.set(key, value);
      },
    };
    preferences = new CommandPalettePreferences(storage);
  }
  return preferences;
}

// MARK: Items

export function agentDefInfo(def: AgentDefinition): AgentDefInfo {
  return {
    name: def.name,
    agent_type: def.agentType,
    template: def.template,
    inline_prompt: def.inlinePrompt,
    tags: [...def.tags],
    scope: def.scope,
    available_in_command_dialog: def.availableInCommandDialog,
    icon: def.icon,
    command: def.command,
  };
}

export function swarmDefInfo(swarm: SwarmDefinition): SwarmDefInfo {
  return {
    name: swarm.name,
    worktree_count: swarm.worktreeCount,
    worktree_template: swarm.worktreeTemplate,
    roster: swarm.roster.map((item) => ({
      agent_def: item.agentDef,
      role: item.role,
      quantity: item.quantity,
    })),
    include_terminal: swarm.includeTerminal,
    scope: swarm.scope,
  };
}

async function loadHub(hub: AgentsHubState, projectRoots: readonly string[]): Promise<void> {
  try {
    await hub.loadAll(projectRoots);
  } catch (error) {
    // The palette still offers the built-ins.
    console.error('PurePoint: palette definitions failed to load', error);
  }
}

function projectItems(
  hub: AgentsHubState,
  includeWorktree: boolean,
  includeSwarms: boolean,
): CommandPaletteItem[] {
  return buildItems({
    builtInVariants: includeWorktree ? variantsWithWorktree() : allVariants(),
    agents: hub.agents.map(agentDefInfo),
    swarms: includeSwarms ? hub.swarms.map(swarmDefInfo) : [],
    preferredOrder: palettePreferences().commandPaletteOrder,
  });
}

/** Swift `FileIndex.list(root:host:)`: local through the backend, remote through the daemon. */
async function listFiles(
  root: string | null,
  project: ProjectState | null,
): Promise<PaletteFileEntry[]> {
  if (root === null) return [];
  try {
    if (project?.isRemote === true) {
      const response = await project.client.send({
        type: 'fs_list',
        path: root,
        recursive: true,
        max_entries: remoteListingLimit(),
      });
      return response.type === 'fs_list_result' ? entriesFromListing(root, response.entries) : [];
    }
    return await fileIndex(root);
  } catch (error) {
    console.error('PurePoint: palette file index failed', error);
    return [];
  }
}

// MARK: Entry points

function showForProject(
  app: AppState,
  project: ProjectState,
  selection: SidebarSelection | null,
  includeWorktree: boolean,
  includeSwarms: boolean,
): void {
  const hub = agentsHubStateFor(app);
  void requestPaletteOpen(async () => {
    await loadHub(hub, app.libraryProjectRoots);
    showPalette({
      items: projectItems(hub, includeWorktree, includeSwarms),
      usePointGuardNames: palettePreferences().usePointGuardNames,
      onSelect: (result) => {
        project.handlePaletteResult(result, selection, async (root, name) => {
          await hub.runSwarm(root, name);
        });
      },
    });
  });
}

/** Swift `showNewAgentPalette()` (File › New Agent…). */
export function showNewAgentPalette(app: AppState): void {
  const workspace = app.registry?.activeWorkspace ?? null;
  const selection = app.activeSidebarSelection;
  const onDashboard = selection?.kind === 'nav' && selection.item === 'dashboard';
  if ((workspace !== null && isPointGuard(workspace)) || onDashboard) {
    app.newPointGuardTab();
    return;
  }
  const active = app.activeProjectRoot;
  const project = active === null ? (app.projects[0] ?? null) : app.projectState(active);
  if (project === null) return;
  showForProject(app, project, selection, true, true);
}

/** The sidebar's New and add buttons; `null` opens it as New Agent does. */
export function openCommandPalette(app: AppState, request: CommandPaletteRequest | null): void {
  if (request === null) {
    showNewAgentPalette(app);
    return;
  }
  const project = app.projectState(request.projectId);
  if (project === null) return;
  showForProject(app, project, request.selection, request.includeWorktree, request.includeWorktree);
}

/** Shows the tab a palette choice filled, in a focused pane. */
function focusSurface(app: AppState, workspaceId: string, surfaceId: number): void {
  const registry = app.registry;
  const workspace = registry?.workspace(workspaceId) ?? null;
  if (registry === null || workspace === null) return;
  const leafId = leafIdOfSurface(workspace, surfaceId);
  if (leafId === null) return;
  registry.selectTab(workspaceId, surfaceId);
  registry.setFocus(workspaceId, leafId);
}

/** What an empty tab does with the palette's choice (Swift `EmptyTabView.showPalette`). */
export function fillSurface(
  app: AppState,
  workspaceId: string,
  surfaceId: number,
  result: CommandPaletteResult,
): void {
  const registry = app.registry;
  const projectId = registry?.projectRoot(workspaceId) ?? null;
  const project = projectId === null ? null : app.projectState(projectId);
  if (registry === null || project === null) return;
  switch (result.kind) {
    case 'spawnBuiltIn':
      project.spawnAgentForSurface({
        agent: result.variant.id,
        prompt: result.prompt ?? '',
        workspaceId,
        surfaceId,
        name: result.name,
      });
      break;
    case 'spawnAgentDef':
      project.spawnAgentForSurface({
        agent: result.def.agent_type,
        prompt: result.prompt ?? result.def.inline_prompt ?? '',
        workspaceId,
        surfaceId,
        name: result.name,
        command: result.def.command ?? null,
      });
      break;
    case 'runSwarm':
    case 'createWorktree':
      return;
    case 'openFilePane':
      registry.openFile(workspaceId, surfaceId, result.path);
      break;
  }
  focusSurface(app, workspaceId, surfaceId);
}

/** Swift `EmptyTabView.openCommandPalette`: spawn an agent into the tab, or open a file in it. */
export function openEmptyTabPalette(app: AppState, workspaceId: string, surfaceId: number): void {
  const hub = agentsHubStateFor(app);
  const rootPath = app.fileRoot(workspaceId);
  const project = app.projectStateForWorkspace(workspaceId);
  void requestPaletteOpen(async () => {
    const [, files] = await Promise.all([
      loadHub(hub, app.paletteProjectRoots(workspaceId)),
      listFiles(rootPath, project),
    ]);
    showPalette({
      items: buildItems({
        builtInVariants: allVariants(),
        agents: hub.agents.map(agentDefInfo),
        swarms: [],
        includeFiles: true,
        files,
        preferredOrder: palettePreferences().commandPaletteOrder,
      }),
      usePointGuardNames: palettePreferences().usePointGuardNames,
      onSelect: (result) => {
        fillSurface(app, workspaceId, surfaceId, result);
      },
    });
  });
}
