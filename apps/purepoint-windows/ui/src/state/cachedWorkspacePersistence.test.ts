// The cached `WorkspacePersistence` adapter (WIN-6.0): the registry's synchronous interface over
// async layout IO. A memory host stands in for the Tauri `fs_*_text` commands.
import { describe, expect, it } from 'vitest';
import { makeAgentModel } from '../model/workspaceModel';
import {
  LayoutWriteConflictError,
  documentText,
  legacyGridLayoutPath,
  workspaceFilePath,
  type HostFileText,
  type LayoutHostIO,
} from '../model/workspacePersistence';
import { CachedWorkspacePersistence, LayoutNotLoadedError } from './cachedWorkspacePersistence';
import { ManualScheduler, MemoryWorkspacePersistence } from './workspaceRegistry.fixtures';
import { WorkspaceRegistry } from './workspaceRegistry';

const root = 'C:\\work\\app';
const path = workspaceFilePath(root);

/** Files with modification times; every write gets a new one. Writes can be held open. */
class MemoryHost implements LayoutHostIO {
  readonly files = new Map<string, { text: string; modifiedMs: number }>();
  clock = 1_000;
  writes = 0;
  failWrites = false;
  hold: Promise<void> | null = null;

  external(target: string, text: string): number {
    this.clock += 7;
    this.files.set(target, { text, modifiedMs: this.clock });
    return this.clock;
  }

  stat(target: string) {
    const file = this.files.get(target);
    return Promise.resolve(file === undefined ? null : { modifiedMs: file.modifiedMs });
  }

  read(target: string, maxBytes: number): Promise<HostFileText | null> {
    const file = this.files.get(target);
    if (file === undefined) return Promise.resolve(null);
    return Promise.resolve({
      text: file.text.slice(0, maxBytes),
      modifiedMs: file.modifiedMs,
      truncated: file.text.length > maxBytes,
    });
  }

  async write(target: string, text: string, expected: number | null): Promise<number> {
    await this.hold;
    this.writes += 1;
    if (this.failWrites) throw new Error('disk full');
    const current = this.files.get(target)?.modifiedMs ?? null;
    if (current !== expected) throw new LayoutWriteConflictError();
    return this.external(target, text);
  }
}

const agent = (id: string) =>
  makeAgentModel({
    id,
    name: id,
    agentType: 'claude',
    status: 'running',
    prompt: '',
    startedAt: '',
  });

function setup(host = new MemoryHost()) {
  const conflicts: string[] = [];
  const errors: string[] = [];
  const persistence = new CachedWorkspacePersistence({
    host,
    onConflict: (projectRoot) => conflicts.push(projectRoot),
    onWriteError: (_, message) => errors.push(message),
  });
  const scheduler = new ManualScheduler();
  const registry = new WorkspaceRegistry({ persistence, scheduler });
  return { host, persistence, registry, scheduler, conflicts, errors };
}

describe('CachedWorkspacePersistence', () => {
  it('refuses to answer for a project before open, then returns what open read', async () => {
    const { host, persistence, registry } = setup();
    expect(() => persistence.loadForRegistry(root)).toThrow(LayoutNotLoadedError);
    registry.reconcile(root, [agent('ag-a')], []);
    // Swift `RestoreError`: no layout invented.
    expect(registry.workspaces(root)).toEqual([]);

    const theirs = new WorkspaceRegistry({
      persistence: new MemoryWorkspacePersistence(),
      scheduler: new ManualScheduler(),
    });
    theirs.reconcile('other', [agent('ag-a')], []);
    host.external(path, documentText(theirs.workspaces('other')) ?? '');
    await persistence.open(root);
    expect(persistence.isOpen(root)).toBe(true);
    expect(persistence.loadForRegistry(root).map((w) => w.id)).toEqual(['ws-ag-a']);
  });

  it('opens a project with no document as an empty layout with no stamp', async () => {
    const { persistence } = setup();
    await persistence.open(root);
    expect(persistence.loadForRegistry(root)).toEqual([]);
    expect(persistence.localModifiedMs(root)).toBeNull();
  });

  it('migrates a legacy grid layout at open and writes the document through', async () => {
    const { host, persistence } = setup();
    host.external(
      legacyGridLayoutPath(root),
      JSON.stringify({ ownerAgentId: 'ag-a', tree: { type: 'leaf', leafId: 0, agentId: 'ag-a' } }),
    );
    await persistence.open(root);
    expect(persistence.loadForRegistry(root).map((w) => w.id)).toEqual(['ws-ag-a']);
    expect(host.files.has(path)).toBe(true);
    expect(persistence.localModifiedMs(root)).not.toBeNull();
  });

  it('writes the registry layout through a queue, coalescing pending texts', async () => {
    const { host, persistence, registry, scheduler } = setup();
    await persistence.open(root);
    registry.reconcile(root, [agent('ag-a')], []);
    let release = () => {};
    host.hold = new Promise((resolve) => {
      release = resolve;
    });
    scheduler.flush();
    // Decided synchronously against the mirror: the registry already holds a stamp.
    const stamp = persistence.localModifiedMs(root);
    expect(stamp).not.toBeNull();
    registry.reconcile(root, [agent('ag-a'), agent('ag-b')], []);
    scheduler.flush();
    registry.reconcile(root, [agent('ag-a'), agent('ag-b'), agent('ag-c')], []);
    scheduler.flush();
    release();
    await persistence.flush();
    // One write in flight, then only the latest pending text.
    expect(host.writes).toBe(2);
    const onDisk = host.files.get(path)?.text ?? '';
    expect(onDisk).toContain('ag-c');
  });

  it("never mistakes this app's own write for an external change", async () => {
    const { persistence, registry, scheduler } = setup();
    await persistence.open(root);
    registry.reconcile(root, [agent('ag-a')], []);
    scheduler.flush();
    await persistence.flush();
    const stamp = persistence.localModifiedMs(root);
    expect(await persistence.refresh(root)).toBe(false);
    expect(persistence.localModifiedMs(root)).toBe(stamp);
  });

  it('adopts an external write found by refresh', async () => {
    const { host, persistence, registry, scheduler } = setup();
    await persistence.open(root);
    registry.reconcile(root, [agent('ag-a'), agent('ag-b')], []);
    scheduler.flush();
    await persistence.flush();
    // Another machine reordered the workspaces.
    const reordered = [...registry.workspaces(root)].reverse();
    host.external(path, documentText(reordered) ?? '');
    expect(await persistence.refresh(root)).toBe(true);
    registry.adoptLocalLayoutIfChanged(root);
    expect(registry.workspaces(root).map((w) => w.id)).toEqual(['ws-ag-b', 'ws-ag-a']);
  });

  it('returns a conflict for a document changed since the registry stamp', async () => {
    const { host, persistence } = setup();
    host.external(path, documentText([]) ?? '');
    await persistence.open(root);
    const stamp = persistence.localModifiedMs(root);
    host.external(path, '{"version":4,"workspaces":[]} ');
    await persistence.refresh(root);
    const outcome = persistence.saveLocal(documentText([]) ?? '', root, stamp);
    expect(outcome.kind).toBe('conflict');
  });

  it('skips a document written by a newer build', async () => {
    const { host, persistence } = setup();
    host.external(path, '{"version":99,"workspaces":[]}');
    await persistence.open(root);
    const stamp = persistence.localModifiedMs(root);
    expect(persistence.saveLocal(documentText([]) ?? '', root, stamp)).toEqual({ kind: 'skipped' });
    expect(persistence.saveLocal('x', 'C:\\never-opened', null)).toEqual({ kind: 'skipped' });
  });

  it('reports a queued write that met another writer, and the registry merges and saves again', async () => {
    const { host, persistence, registry, scheduler, conflicts } = setup();
    await persistence.open(root);
    registry.reconcile(root, [agent('ag-a'), agent('ag-b')], []);
    scheduler.flush();
    await persistence.flush();
    let release = () => {};
    host.hold = new Promise((resolve) => {
      release = resolve;
    });
    registry.closeWorkspace('ws-ag-b');
    scheduler.flush();
    // `pu grid` writes between this app's decision and its write.
    host.external(path, documentText([...registry.workspaces(root)].reverse()) ?? '');
    release();
    await persistence.flush();
    expect(conflicts).toEqual([root]);
    // The owner adopts: the mirror now holds the other writer's document.
    registry.adoptLocalLayoutIfChanged(root);
    scheduler.flush();
    host.hold = null;
    await persistence.flush();
    expect(host.files.get(path)?.text).toBeDefined();
  });

  it('reports other write failures and keeps the text', async () => {
    const { host, persistence, registry, scheduler, errors } = setup();
    await persistence.open(root);
    host.failWrites = true;
    registry.reconcile(root, [agent('ag-a')], []);
    scheduler.flush();
    await persistence.flush();
    expect(errors).toEqual(['disk full']);
    expect(persistence.readLocal(path, 1_000_000).data).toContain('ag-a');
  });

  it('reads the mirror for readLocal and throws without a document', async () => {
    const { persistence } = setup();
    await persistence.open(root);
    expect(() => persistence.readLocal(path, 100)).toThrow();
    persistence.saveLocal('{"version":4,"workspaces":[]}', root, null);
    const read = persistence.readLocal(path, 5);
    expect(read.truncated).toBe(true);
    expect(read.modifiedMs).toBe(persistence.localModifiedMs(root));
    await persistence.close(root);
    expect(persistence.isOpen(root)).toBe(false);
  });
});
