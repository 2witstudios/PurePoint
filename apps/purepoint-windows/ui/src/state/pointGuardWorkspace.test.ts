// Port of apps/purepoint-macos/purepoint-macosTests/PointGuardWorkspaceTests.swift (WIN-8.13): the
// Point Guard workspace in the registry (chat tabs, splits, persistence and identity re-keying of
// copied, renamed and newer layout documents).
//
// The registry runs on the app's `CachedWorkspacePersistence` over a memory host standing in for
// the Tauri `fs_*_text` commands, so the documents are the real `.pu/workspaces.json` text. Swift
// reads the file synchronously on first use; here `persistence.open(root)` reads it first, as
// ProjectState does when a project starts. A copied or renamed project directory is the same
// document at another root's path. Swift's `WorkspacePersistence.save`/`load` are
// `saveWorkspaces`/`loadWorkspaces` over a memory `WorkspaceFileIO`.
import { describe, expect, it } from 'vitest';
import {
  POINT_GUARD_CONTENT,
  PROJECT_ROOT,
  adoptingWorkspace,
  agentContent,
  fileContent,
  focusedSurface,
  isPointGuard,
  liveAgent,
  newTab,
  paneCount,
  pointGuardId,
  pointGuardWorkspace,
  reconcileWorkspaces,
  splitPane,
  tabCount,
  workspaceAgentIds,
  workspaceSurfaces,
  worktreeContainer,
  type Workspace,
} from '../model/workspace';
import { makeAgentModel } from '../model/workspaceModel';
import {
  CURRENT_DOCUMENT_VERSION,
  LayoutWriteConflictError,
  documentText,
  isNewerThanSupported,
  loadWorkspaces,
  memoryWorkspaceFileIO,
  saveWorkspaces,
  workspaceFilePath,
  workspacesFromText,
  type HostFileText,
  type LayoutHostIO,
} from '../model/workspacePersistence';
import { CachedWorkspacePersistence } from './cachedWorkspacePersistence';
import { ManualScheduler } from './workspaceRegistry.fixtures';
import { WorkspaceRegistry } from './workspaceRegistry';

const root = 'C:\\pg';

/** Files with modification times; every write gets a new one. Writes can be made to fail. */
class MemoryHost implements LayoutHostIO {
  readonly files = new Map<string, { text: string; modifiedMs: number }>();
  private clock = 1_000;
  /** Thrown by every write while set (Swift's `CocoaError` from the write closure). */
  writeError: Error | null = null;

  put(path: string, text: string): void {
    this.clock += 1;
    this.files.set(path, { text, modifiedMs: this.clock });
  }

  text(projectRoot: string): string | null {
    return this.files.get(workspaceFilePath(projectRoot))?.text ?? null;
  }

  stat(path: string) {
    const file = this.files.get(path);
    return Promise.resolve(file === undefined ? null : { modifiedMs: file.modifiedMs });
  }

  read(path: string, maxBytes: number): Promise<HostFileText | null> {
    const file = this.files.get(path);
    if (file === undefined) return Promise.resolve(null);
    return Promise.resolve({
      text: file.text.slice(0, maxBytes),
      modifiedMs: file.modifiedMs,
      truncated: file.text.length > maxBytes,
    });
  }

  write(path: string, text: string, expected: number | null): Promise<number> {
    if (this.writeError !== null) return Promise.reject(this.writeError);
    if ((this.files.get(path)?.modifiedMs ?? null) !== expected) {
      return Promise.reject(new LayoutWriteConflictError());
    }
    this.put(path, text);
    return Promise.resolve(this.clock);
  }
}

function setup(host = new MemoryHost()) {
  const persistence = new CachedWorkspacePersistence({ host });
  const registry = new WorkspaceRegistry({ persistence, scheduler: new ManualScheduler() });
  return { host, persistence, registry };
}

/** Swift `withRegistry`: a registry with `root`'s layout read. */
async function withRegistry(host = new MemoryHost()) {
  const context = setup(host);
  await context.persistence.open(root);
  return context;
}

const terminal = (id: string) =>
  makeAgentModel({
    id,
    name: id,
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

function ws(registry: WorkspaceRegistry, id: string): Workspace {
  return required(registry.workspace(id));
}

const contents = (workspace: Workspace) =>
  workspaceSurfaces(workspace).map(({ surface }) => surface.content);

describe('Point Guard workspace', () => {
  it('lets chat split and share tabs with root agents', async () => {
    const { registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    registry.split(id, 0, 'vertical');
    const target = required(focusedSurface(ws(registry, id))).id;
    registry.reserveSurface(root, id, target);
    registry.fulfillReservation(root, id, target, 'ag-terminal');
    registry.reconcile(root, [terminal('ag-terminal')], []);
    const workspace = ws(registry, id);
    expect(workspace.container).toEqual(PROJECT_ROOT);
    expect(paneCount(workspace)).toBe(2);
    expect(contents(workspace)).toEqual([POINT_GUARD_CONTENT, agentContent('ag-terminal')]);
    expect(registry.workspaceIdForAgent('ag-terminal')).toBe(id);
    expect(registry.workspacesByProject[root]).toHaveLength(1);
  });

  it('keeps the focused chat and the existing layout when Point Guard is visited', async () => {
    const { registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    const terminalTab = required(registry.newTab(id, 0, agentContent('ag-terminal')));
    registry.selectTab(id, 0);
    registry.activate(id);
    expect(focusedSurface(required(registry.activeWorkspace))?.content).toEqual(
      POINT_GUARD_CONTENT,
    );
    expect(registry.ensurePointGuardWorkspace(root)).toBe(id);
    expect(tabCount(ws(registry, id))).toBe(2);
    registry.selectTab(id, terminalTab);
    expect(focusedSurface(required(registry.activeWorkspace))?.content).toEqual(
      agentContent('ag-terminal'),
    );
  });

  it('returns to chat when the last tab closes, without reusing surface ids', async () => {
    const { registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    registry.activeWorkspaceId = id;
    registry.closeTab(id, 0);
    const workspace = ws(registry, id);
    expect(focusedSurface(workspace)?.content).toEqual(POINT_GUARD_CONTENT);
    expect(focusedSurface(workspace)?.id).not.toBe(0);
    expect(registry.activeWorkspaceId).toBe(id);
  });

  it('neither removes chat for a dead terminal nor adopts it as an agent', async () => {
    const { registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    registry.newTab(id, 0, agentContent('ag-dead'));
    registry.reconcile(root, [], []);
    expect(contents(ws(registry, id))).toEqual([POINT_GUARD_CONTENT]);
  });

  it('does not kill the sibling terminal when chat closes, and reopening restores chat', async () => {
    const { registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    registry.newTab(id, 0, agentContent('ag-terminal'));
    const killed: string[] = [];
    registry.onCloseAgent = (_, agentId) => killed.push(agentId);
    registry.closeTab(id, 0);
    expect(killed).toEqual([]);
    expect(workspaceAgentIds(ws(registry, id))).toEqual(['ag-terminal']);
    registry.ensurePointGuardWorkspace(root);
    expect(tabCount(ws(registry, id))).toBe(2);
    expect(focusedSurface(ws(registry, id))?.content).toEqual(POINT_GUARD_CONTENT);
  });

  it('persists chat and file tabs across a restart', async () => {
    const { host, persistence, registry } = await withRegistry();
    const id = registry.ensurePointGuardWorkspace(root);
    registry.split(id, 0, 'horizontal');
    const file = required(focusedSurface(ws(registry, id))).id;
    registry.openFile(id, file, root + '\\notes.md');
    registry.activeWorkspaceId = id;
    registry.saveAll();
    await persistence.flush();
    const text = required(host.text(root));
    // Version 3 builds must recognize this as newer and leave it untouched.
    expect((JSON.parse(text) as { version: number }).version).toBe(4);
    const restarted = setup(host).persistence;
    await restarted.open(root);
    const restored = restarted.loadForRegistry(root);
    expect(restored).toEqual([ws(registry, id)]);
    expect(restored[0] !== undefined && isPointGuard(restored[0])).toBe(true);
    expect(reconcileWorkspaces(restored, [])).toEqual(restored);
  });

  it('still loads version three tab layouts and upgrades them without losing ids', async () => {
    let original = adoptingWorkspace('ag-existing', PROJECT_ROOT);
    original = newTab(original, 0, fileContent(root + '\\notes.md')).workspace;
    original = splitPane(original, 0, 'vertical', agentContent('ag-sibling')).workspace;
    const io = memoryWorkspaceFileIO();
    const path = workspaceFilePath(root);
    await saveWorkspaces(io, root, [original], original.id);
    const document = JSON.parse(required(io.files.get(path))) as Record<string, unknown>;
    document.version = 3;
    io.files.set(path, JSON.stringify(document));

    const restored = await loadWorkspaces(io, root);
    expect(restored).toEqual([original]);
    await saveWorkspaces(io, root, restored, original.id);
    const upgraded = JSON.parse(required(io.files.get(path))) as {
      version: number;
      activeWorkspaceId?: string;
    };
    expect(upgraded.version).toBe(4);
    expect(upgraded.activeWorkspaceId).toBe(original.id);
    expect(await loadWorkspaces(io, root)).toEqual([original]);
  });

  it('gives each project root its own Point Guard layout', async () => {
    const { persistence, registry } = await withRegistry();
    const secondRoot = root + '-second';
    await persistence.open(secondRoot);
    const first = registry.ensurePointGuardWorkspace(root);
    const second = registry.ensurePointGuardWorkspace(secondRoot);
    expect(first).not.toBe(second);
    expect(registry.projectRoot(first)).toBe(root);
    expect(registry.projectRoot(second)).toBe(secondRoot);
  });

  it('re-keys a copied project before global lookups and mutations', async () => {
    const copiedRoot = root + '-copy';
    let original = pointGuardWorkspace(root);
    original = splitPane(original, 0, 'vertical', fileContent(null)).workspace;
    original = newTab(original, original.focusedLeafId, fileContent(root + '\\notes.md')).workspace;
    const host = new MemoryHost();
    const text = required(documentText([original], original.id));
    host.put(workspaceFilePath(root), text);
    host.put(workspaceFilePath(copiedRoot), text);
    const { persistence, registry } = await withRegistry(host);
    await persistence.open(copiedRoot);

    const originalId = registry.ensurePointGuardWorkspace(root);
    // Reconcile is also a first-load path, before the chat view has appeared.
    registry.reconcile(copiedRoot, [], []);
    const copiedId = registry.ensurePointGuardWorkspace(copiedRoot);
    const migrated = JSON.parse(required(host.text(copiedRoot))) as {
      activeWorkspaceId?: string;
      workspaces: { id: string }[];
    };
    expect(migrated.activeWorkspaceId).toBe(copiedId);
    expect(migrated.workspaces.map((w) => w.id)).toEqual([copiedId]);
    expect(copiedId).toBe(pointGuardId(copiedRoot));
    expect(copiedId).not.toBe(originalId);
    expect(registry.projectRoot(originalId)).toBe(root);
    expect(registry.projectRoot(copiedId)).toBe(copiedRoot);
    const restored = ws(registry, copiedId);
    expect(restored.root).toEqual(original.root);
    expect(restored.panes).toEqual(original.panes);
    expect(restored.focusedLeafId).toBe(original.focusedLeafId);
    expect(restored.nextLeafId).toBe(original.nextLeafId);
    expect(restored.nextSurfaceId).toBe(original.nextSurfaceId);

    registry.newTab(copiedId, restored.focusedLeafId, fileContent(copiedRoot + '\\copy.md'));
    expect(registry.workspace(originalId)).toEqual(original);
    expect(tabCount(ws(registry, copiedId))).toBe(tabCount(original) + 1);
    registry.saveAll();
    await persistence.flush();
    expect(workspacesFromText(required(host.text(copiedRoot)), copiedRoot)).toEqual([
      ws(registry, copiedId),
    ]);
  });

  it('loads the existing layout of a renamed project with its current id', async () => {
    const renamedRoot = root + '-renamed';
    let original = pointGuardWorkspace(root);
    original = newTab(original, 0, agentContent('ag-terminal')).workspace;
    // The folder moved: its document now sits under the new root.
    const host = new MemoryHost();
    host.put(workspaceFilePath(renamedRoot), required(documentText([original])));
    const { persistence, registry } = setup(host);
    await persistence.open(renamedRoot);

    const renamedId = registry.ensurePointGuardWorkspace(renamedRoot);
    const restored = ws(registry, renamedId);
    expect(renamedId).toBe(pointGuardId(renamedRoot));
    expect(registry.workspace(original.id)).toBeNull();
    expect(restored.panes).toEqual(original.panes);
    expect(restored.root).toEqual(original.root);
  });

  it('re-keys a newer copied document without downgrading it or dropping unknown data', async () => {
    const original = pointGuardWorkspace(root + '-original');
    const future = JSON.parse(required(documentText([original], original.id))) as Record<
      string,
      unknown
    >;
    future.version = CURRENT_DOCUMENT_VERSION + 1;
    future.futureDocument = { value: 'preserve', number: 42 };
    const workspaces = future.workspaces as Record<string, unknown>[];
    const first = required(workspaces[0]);
    first.futureWorkspace = { enabled: true };
    const tree = first.tree as Record<string, unknown>;
    tree.futureTree = { layout: 'new' };
    const tabs = tree.tabs as Record<string, unknown>[];
    required(tabs[0]).futureSurface = { content: 'opaque' };
    tabs.push({ id: 10, kind: 'future_browser', url: 'https://example.com' });
    const host = new MemoryHost();
    host.put(workspaceFilePath(root), JSON.stringify(future));
    const { persistence, registry } = await withRegistry(host);

    const id = registry.ensurePointGuardWorkspace(root);
    expect(id).not.toBe(original.id);
    first.id = id;
    future.activeWorkspaceId = id;
    const migratedText = required(host.text(root));
    expect(JSON.parse(migratedText)).toEqual(future);
    expect(isNewerThanSupported(migratedText)).toBe(true);

    registry.newTab(id, 0);
    registry.saveAll();
    await persistence.flush();
    // Only identity migration is allowed; regular saves must retain the future schema.
    expect(host.text(root)).toBe(migratedText);
  });

  it.each([
    ['no permission', new Error('EACCES: permission denied')],
    ['out of space', new Error('ENOSPC: no space left on device')],
  ])(
    'blocks publication on a failed identity write (%s) and retries without touching the other project',
    async (_, error) => {
      const copiedRoot = root + '-copy';
      const original = pointGuardWorkspace(root);
      const host = new MemoryHost();
      host.put(workspaceFilePath(root), required(documentText([original])));
      host.put(workspaceFilePath(copiedRoot), required(documentText([original], original.id)));
      const before = host.text(copiedRoot);
      const { persistence, registry } = await withRegistry(host);
      const originalId = registry.ensurePointGuardWorkspace(root);
      registry.activate(originalId);

      host.writeError = error;
      await persistence.open(copiedRoot);
      registry.reconcile(copiedRoot, [], []);
      expect(() => registry.ensurePointGuardWorkspace(copiedRoot)).toThrow(error.message);
      expect(registry.workspacesByProject[copiedRoot]).toBeUndefined();
      expect(registry.workspace(pointGuardId(copiedRoot))).toBeNull();
      expect(registry.layoutErrorsByProject[copiedRoot]).toBeDefined();
      expect(registry.workspace(originalId)).toEqual(original);
      registry.saveAll();
      await persistence.flush();
      expect(host.text(copiedRoot)).toBe(before);

      host.writeError = null;
      // The failed read is cached; reading the document again is the retry.
      await persistence.open(copiedRoot);
      const copiedId = registry.ensurePointGuardWorkspace(copiedRoot);
      expect(copiedId).not.toBe(originalId);
      expect(registry.projectRoot(copiedId)).toBe(copiedRoot);
      expect(registry.layoutErrorsByProject[copiedRoot]).toBeUndefined();
      expect(registry.workspace(originalId)).toEqual(original);
      const document = JSON.parse(required(host.text(copiedRoot))) as {
        activeWorkspaceId?: string;
      };
      expect(document.activeWorkspaceId).toBe(copiedId);
    },
  );

  it('reconciles the latest manifest on a successful retry without waiting for another event', async () => {
    let original = pointGuardWorkspace(root + '-source');
    original = newTab(original, 0, agentContent('ag-source')).workspace;
    const host = new MemoryHost();
    host.put(workspaceFilePath(root), required(documentText([original])));
    host.writeError = new Error('EACCES: permission denied');
    const { persistence, registry } = await withRegistry(host);
    registry.reconcile(root, [terminal('ag-previous')], []);
    registry.reconcile(root, [terminal('ag-current')], []);
    expect(registry.workspacesByProject[root]).toBeUndefined();

    host.writeError = null;
    await persistence.open(root);
    const id = registry.ensurePointGuardWorkspace(root);
    expect(contents(ws(registry, id))).toEqual([POINT_GUARD_CONTENT]);
    expect(registry.workspaceIdForAgent('ag-source')).toBeNull();
    expect(registry.workspaceIdForAgent('ag-previous')).toBeNull();
    const current = required(registry.workspaceIdForAgent('ag-current'));
    expect(registry.projectRoot(current)).toBe(root);
    expect(registry.layoutErrorsByProject[root]).toBeUndefined();
  });

  it('does not prune a spawn response before its manifest arrives when Point Guard is revisited', async () => {
    const { registry } = await withRegistry();
    registry.reconcile(root, [], []);
    const id = registry.ensurePointGuardWorkspace(root);
    const surface = required(registry.newTab(id, 0));
    registry.reserveSurface(root, id, surface);
    registry.fulfillReservation(root, id, surface, 'ag-new');

    expect(registry.ensurePointGuardWorkspace(root)).toBe(id);
    expect(registry.workspaceIdForAgent('ag-new')).toBe(id);
    expect(focusedSurface(ws(registry, id))?.content).toEqual(agentContent('ag-new'));
  });

  it('does not let Point Guard claim worktree agents', () => {
    const workspace = newTab(
      pointGuardWorkspace('/tmp/project'),
      0,
      agentContent('ag-worktree'),
    ).workspace;
    const restored = reconcileWorkspaces(
      [workspace],
      [liveAgent('ag-worktree', worktreeContainer('wt-one'))],
    );
    expect(restored[0]?.container).toEqual(PROJECT_ROOT);
    expect(restored[0] === undefined ? null : contents(restored[0])).toEqual([POINT_GUARD_CONTENT]);
    expect(restored.at(-1)?.container).toEqual(worktreeContainer('wt-one'));
  });
});
