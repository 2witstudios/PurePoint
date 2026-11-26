// Port of apps/purepoint-macos/purepoint-macosTests/PointGuardRoutingTests.swift (WIN-8.13): Point
// Guard's folder routing in AppState (tabs, splits, palette roots, hidden folders and the local
// daemon's project list).
//
// UserDefaults is a memory storage holding the same keys as JSON arrays, and "the folder exists" is
// a `pathExists` over a set instead of directories created on disk. Swift's
// `AppState.canonicalPath` resolves `/private/var` symlinks of macOS temporary folders; Windows
// roots have none, so the daemon reports the same strings the app opened.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: () => Promise.resolve(true),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => {}),
}));

const { AppState } = await import('./appState');
const { WorkspaceRegistry } = await import('./workspaceRegistry');
const { ManualScheduler, MemoryWorkspacePersistence } =
  await import('./workspaceRegistry.fixtures');
const { makeAgentModel } = await import('../model/workspaceModel');
const { PROJECT_ROOT, focusedSurface, tabCount } = await import('../model/workspace');
type AppStateInstance = import('./appState').AppState;
type Registry = import('./workspaceRegistry').WorkspaceRegistry;

const POINT_GUARD_FOLDERS = 'PurePointPointGuardFolders';
const OPEN_PROJECTS = 'PurePointOpenProjects';
const SHOWN_POINT_GUARD_FOLDERS = 'PurePointShownPointGuardFolders';

const root = 'C:\\pg';
const project = root + '-project';
const next = root + '-next';

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  };
}

type Storage = ReturnType<typeof memoryStorage>;

const terminal = (id: string) =>
  makeAgentModel({
    id,
    name: 'Terminal',
    agentType: 'terminal',
    status: 'running',
    prompt: '',
    startedAt: '',
  });

function required<T>(value: T | null | undefined): T {
  expect(value).not.toBeNull();
  expect(value).not.toBeUndefined();
  return value as T;
}

let storage: Storage;
let existing: Set<string>;

beforeEach(() => {
  storage = memoryStorage();
  existing = new Set();
});

function makeState(options: { registry?: Registry | null; started?: string[] } = {}) {
  const registry =
    options.registry === undefined
      ? new WorkspaceRegistry({
          persistence: new MemoryWorkspacePersistence(),
          scheduler: new ManualScheduler(),
        })
      : options.registry;
  const state = new AppState({
    registry,
    client: { send: () => Promise.resolve({ type: 'ok' }) },
    startProject: (started) => options.started?.push(started.projectRoot),
    followsLocalDaemon: false,
    storage,
    homeDirectory: 'C:\\Users\\me',
    pathExists: (path) => existing.has(path),
  });
  return { state, registry };
}

/** Swift `withState`: an AppState with a registry whose Pi folder is `root`. */
function withState(): { state: AppStateInstance; registry: Registry } {
  const { state, registry } = makeState();
  state.updatePointGuardRoot(root);
  return { state, registry: required(registry) };
}

function setList(key: string, list: string[]): void {
  storage.setItem(key, JSON.stringify(list));
}

function getList(key: string): unknown {
  const raw = storage.getItem(key);
  return raw === null ? null : JSON.parse(raw);
}

const roots = (state: AppStateInstance) => state.projects.map((p) => p.projectRoot);

describe('Point Guard routing', () => {
  it('routes a new Point Guard tab nowhere before Pi reports its folder', () => {
    const { state, registry } = withState();
    state.updatePointGuardRoot('');
    state.openProject(project);
    expect(state.activeSidebarSelection).toEqual({ kind: 'nav', item: 'dashboard' });
    expect(state.activeProjectRoot).toBe(project);
    state.newPointGuardTab();
    expect(registry.activeWorkspaceId).toBeNull();
    expect(registry.workspacesByProject).toEqual({});
    expect(roots(state)).toEqual([project]);
  });

  it('queries the palette in the Point Guard folder with and without visible projects', () => {
    const { state } = withState();
    const id = required(state.activatePointGuardWorkspace());
    expect(state.paletteProjectRoots(id)).toEqual([root]);
    state.openProject(project);
    expect(state.paletteProjectRoots(id)).toEqual([root, project]);
    state.openProject(root);
    expect(state.paletteProjectRoots(id)).toEqual([root, project]);
  });

  it('restores hidden folders on an argument launch without restoring other visible projects', () => {
    const { state } = withState();
    existing.add(root);
    setList(OPEN_PROJECTS, [next]);
    setList(POINT_GUARD_FOLDERS, [root]);
    state.restoreProjects(project);
    expect(roots(state)).toEqual([project]);
    expect(state.projectState(root)).not.toBeNull();
    expect(state.projectState(next)).toBeNull();
  });

  it('retains saved hidden Point Guard folders when restoring open projects', () => {
    const { state } = withState();
    existing.add(root).add(project);
    setList(OPEN_PROJECTS, [project]);
    setList(POINT_GUARD_FOLDERS, [root]);
    state.restoreProjects();
    expect(roots(state)).toEqual([project]);
    expect(state.projectState(root)).not.toBeNull();
    expect(getList(POINT_GUARD_FOLDERS)).toEqual([root]);
  });

  it('waits for the Pi working folder before creating a workspace', () => {
    const started: string[] = [];
    const { state, registry } = makeState({ started });
    state.updatePointGuardRoot('');
    expect(state.pointGuardRoot).toBeNull();
    expect(state.activatePointGuardWorkspace()).toBeNull();
    state.newPointGuardTab();
    expect(started).toEqual([]);
    expect(registry?.workspacesByProject).toEqual({});
    expect(registry?.activeWorkspaceId).toBeNull();
  });

  it('clears a previously selected coding workspace when the Point Guard folder is unknown', () => {
    const { state, registry } = withState();
    const agent = terminal('ag-coding');
    registry.reconcile(project, [agent], []);
    const id = required(registry.workspaceIdForAgent(agent.id));
    registry.activate(id);
    state.updatePointGuardRoot('');
    expect(state.activatePointGuardWorkspace()).toBeNull();
    expect(registry.activeWorkspaceId).toBeNull();
    expect(registry.workspace(id)).not.toBeNull();
  });

  it('keeps the visible coding project active when restoring or selecting Point Guard', () => {
    const { state, registry } = withState();
    state.openProject(project);
    const id = required(state.activatePointGuardWorkspace());
    state.pendingSelectWorkspaceId = id;
    expect(state.resolvePendingWorkspaceSelection()).toBe(id);
    expect(state.activeProjectRoot).toBe(project);
    state.updateActiveProject({ kind: 'workspace', id });
    expect(state.activeProjectRoot).toBe(project);
    state.updateActiveProject({ kind: 'nav', item: 'dashboard' });
    expect(state.activeProjectRoot).toBe(project);
    expect(registry.projectRoot(id)).toBe(root);
  });

  it('splits in the Pi folder whatever project is selected', () => {
    const { state, registry } = withState();
    state.openProject(project);
    state.activeProjectRoot = project;
    const id = required(state.activatePointGuardWorkspace());
    for (const axis of ['vertical', 'horizontal'] as const) {
      const leaf = required(registry.workspace(id)).focusedLeafId;
      registry.split(id, leaf, axis);
      expect(registry.projectRoot(id)).toBe(root);
      expect(registry.workspace(id)?.container).toEqual(PROJECT_ROOT);
      expect(state.projectState(root)?.projectRoot).toBe(root);
      expect(state.fileRoot(id)).toBe(root);
    }
    expect(roots(state)).toEqual([project]);
    expect(state.activeProjectRoot).toBe(project);
  });

  it('supports Point Guard tabs without an open project and finds their agents', () => {
    const { state, registry } = withState();
    state.newPointGuardTab();
    const workspace = required(registry.activeWorkspace);
    expect(registry.projectRoot(workspace.id)).toBe(root);
    expect(tabCount(workspace)).toBe(2);
    expect(registry.pendingPaletteSurfaceId).toBe(focusedSurface(workspace)?.id);
    expect(state.projects).toEqual([]);
    const owner = required(state.projectState(root));
    const agent = terminal('ag-pg');
    owner.store.setState({ rootAgents: [agent] });
    expect(state.agent(agent.id)?.id).toBe(agent.id);
    expect(state.projectStateForAgent(agent.id)).toBe(owner);
  });

  it('creates the next tab in a changed Pi folder and keeps the previous state', () => {
    const { state, registry } = withState();
    const original = required(state.activatePointGuardWorkspace());
    state.updatePointGuardRoot(next);
    state.newPointGuardTab();
    const nextWorkspace = required(registry.activeWorkspace);
    expect(nextWorkspace.id).not.toBe(original);
    expect(registry.projectRoot(nextWorkspace.id)).toBe(next);
    expect(state.projectState(root)).not.toBeNull();
    expect(registry.workspace(original)).not.toBeNull();
    state.updatePointGuardRoot('');
    expect(state.pointGuardRoot).toBeNull();
  });

  it('retains the state of a configured folder removed before its workspace loads', () => {
    const { state, registry } = withState();
    state.openProject(root);
    const owner = required(state.projectState(root));
    expect(registry.workspacesByProject[root]).toBeUndefined();
    expect(state.closeProject(root)).toBe(false);
    expect(state.projectState(root)).toBe(owner);
    expect(getList(POINT_GUARD_FOLDERS)).toEqual([root]);
  });

  it('retains the state of a historical visible folder removed before its layout restores', () => {
    const { state, registry } = withState();
    existing.add(root);
    setList(OPEN_PROJECTS, [root]);
    setList(POINT_GUARD_FOLDERS, [root]);
    state.restoreProjects();
    state.updatePointGuardRoot(next);
    const owner = required(state.projectState(root));
    expect(registry.workspacesByProject[root]).toBeUndefined();
    expect(state.closeProject(root)).toBe(false);
    expect(state.projectState(root)).toBe(owner);
  });

  it('keeps the owner of a removed Point Guard project and selects another visible project', () => {
    const { state } = withState();
    required(state.activatePointGuardWorkspace());
    const owner = required(state.projectState(root));
    state.openProject(root);
    state.openProject(project);
    state.activeProjectRoot = root;
    expect(state.closeProject(root)).toBe(false);
    expect(state.projectState(root)).toBe(owner);
    expect(state.activeProjectRoot).toBe(project);
    expect(state.closeProject(project)).toBe(true);
    expect(state.activeProjectRoot).toBeNull();
  });

  it('keeps the same agent owner when the Pi folder is opened and closed as a project', () => {
    const { state } = withState();
    required(state.activatePointGuardWorkspace());
    const owner = required(state.projectState(root));
    state.openProject(root);
    expect(state.projects[0]).toBe(owner);
    expect(state.closeProject(root)).toBe(false);
    expect(state.activeProjectRoot).toBeNull();
    expect(state.projects).toEqual([]);
    expect(state.projectState(root)).toBe(owner);
  });
});

describe("Point Guard routing: the local daemon's projects decide the sidebar", () => {
  it('opens registered projects and closes ones removed elsewhere', () => {
    const { state } = withState();
    existing.add(project).add(next);
    state.applyLocalProjects([next, project]);
    expect(new Set(roots(state))).toEqual(new Set([project, next]));
    // Removed on another device, or with `pu projects remove`.
    state.applyLocalProjects([project]);
    expect(roots(state)).toEqual([project]);
  });

  it('keeps a project opened before the daemon lists it', () => {
    const { state } = withState();
    existing.add(project);
    // Its `init` has not reached the daemon yet: absent from a list it was never in.
    state.openProject(project);
    state.applyLocalProjects([]);
    expect(roots(state)).toEqual([project]);
    // It is the same project once the daemon lists it.
    state.applyLocalProjects([project]);
    expect(roots(state)).toEqual([project]);
  });

  it('keeps the Point Guard folder out of the sidebar unless opened', () => {
    const { state } = withState();
    existing.add(root);
    storage.removeItem(OPEN_PROJECTS);
    setList(POINT_GUARD_FOLDERS, [root]);
    storage.removeItem(SHOWN_POINT_GUARD_FOLDERS);
    state.restoreProjects();
    state.applyLocalProjects([root]);
    expect(state.projects).toEqual([]);
    expect(state.projectState(root)).not.toBeNull();
    state.openProject(root);
    expect(getList(SHOWN_POINT_GUARD_FOLDERS)).toEqual([root]);
  });

  it('imports the legacy sidebar list once, then deletes it', () => {
    const { state } = withState();
    existing.add(project);
    setList(OPEN_PROJECTS, [project]);
    state.restoreProjects();
    expect(roots(state)).toEqual([project]);
    expect(getList(OPEN_PROJECTS)).toBeNull();
  });

  it('keeps the suspended agents of projects joining after launch paused', () => {
    const { state } = withState();
    existing.add(project).add(next);
    storage.removeItem(OPEN_PROJECTS);
    state.restoreProjects();
    // Last session's sidebar: this app suspended its agents on quit and resumes them.
    state.applyLocalProjects([project]);
    expect(state.projectState(project)?.keepsSuspendedAgentsOnNextOpen).toBe(false);
    // Added later, by another device or the CLI.
    state.applyLocalProjects([next, project]);
    expect(state.projectState(next)?.keepsSuspendedAgentsOnNextOpen).toBe(true);
  });

  it('still resumes last session when the legacy entry is on an unmounted volume', () => {
    const { state } = withState();
    existing.add(project);
    setList(OPEN_PROJECTS, [root + '-unmounted']);
    state.restoreProjects();
    expect(getList(OPEN_PROJECTS)).toEqual([root + '-unmounted']);
    state.applyLocalProjects([project]);
    expect(state.projectState(project)?.keepsSuspendedAgentsOnNextOpen).toBe(false);
  });

  it('registers a Point Guard folder removed elsewhere again', () => {
    const started: string[] = [];
    existing.add(root);
    const { state } = makeState({ registry: null, started });
    state.updatePointGuardRoot(root);
    state.openProject(root);
    state.applyLocalProjects([root]);
    started.length = 0;
    state.applyLocalProjects([]);
    expect(started).toEqual([root]);
    expect(roots(state)).toEqual([root]);
    // The agents the removal suspended stay paused.
    expect(state.projectState(root)?.keepsSuspendedAgentsOnNextOpen).toBe(true);
  });
});
