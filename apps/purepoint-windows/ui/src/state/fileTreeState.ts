// Port of apps/purepoint-macos/purepoint-macos/State/FileTreeState.swift (WIN-8.4): a lazily
// listed, watched file tree rooted at a worktree or project.
//
// Shape (DEC-14): what Swift's `@Observable` publishes (`rootNodes`, `selectedFilePath`,
// `searchQuery`) lives in `store`, plus `expandedPaths`, which Swift keeps both here and in the
// outline view (one copy here serves both). Nodes are values: every refresh builds and filters a
// whole replacement before publishing it, so an unfiltered listing is never on screen, and a
// refresh superseded by a newer one (or a new root) is dropped.
//
// Differences from Swift: a local listing is a backend round trip, so `load` publishes an empty
// tree until the first refresh arrives (Swift scans the root synchronously); the methods that
// refresh return the refresh's promise for callers (and tests) that wait for it.

import { createStore, type StoreApi } from 'zustand/vanilla';
import { relativePath, sortedFileNodes, type FileNode } from '../model/fileNode';
import { localFileHost, type FileHost, type FileHostEntry } from '../services/fileHost';

/** Swift `FileTreeState.hiddenNames`: never listed (`src-tauri/src/watchers/file_tree.rs`). */
export const FILE_TREE_HIDDEN_NAMES: ReadonlySet<string> = new Set([
  '.git',
  '.DS_Store',
  '.build',
  '.swiftpm',
  'xcuserdata',
  'DerivedData',
  '__pycache__',
  '.tsbuildinfo',
  'node_modules',
]);

export interface FileTreeFields {
  readonly rootNodes: readonly FileNode[];
  readonly selectedFilePath: string | null;
  readonly searchQuery: string;
  /** Absolute paths of the expanded folders. */
  readonly expandedPaths: ReadonlySet<string>;
}

/** Swift's filter: hidden names never show; an ignored entry shows only if it is a dot name. */
export function visibleEntry(entry: FileHostEntry): boolean {
  if (FILE_TREE_HIDDEN_NAMES.has(entry.name)) return false;
  return !(entry.ignored && !entry.name.startsWith('.'));
}

export class FileTreeState {
  readonly store: StoreApi<FileTreeFields>;
  private worktreePath: string | null = null;
  private hostValue: FileHost = localFileHost;
  private stopWatcher: (() => void) | null = null;
  private generation = 0;

  constructor() {
    this.store = createStore<FileTreeFields>()(() => ({
      rootNodes: [],
      selectedFilePath: null,
      searchQuery: '',
      expandedPaths: new Set(),
    }));
  }

  private get s(): FileTreeFields {
    return this.store.getState();
  }

  get rootNodes(): readonly FileNode[] {
    return this.s.rootNodes;
  }

  get host(): FileHost {
    return this.hostValue;
  }

  get isLocal(): boolean {
    return this.hostValue.isLocal;
  }

  get rootPath(): string | null {
    return this.worktreePath;
  }

  load(worktreePath: string, host: FileHost = localFileHost): Promise<void> {
    this.worktreePath = worktreePath;
    this.hostValue = host;
    this.store.setState({ rootNodes: [], expandedPaths: new Set(), selectedFilePath: null });
    const refreshed = this.refresh();
    this.stopWatcher?.();
    this.stopWatcher = host.watch(worktreePath, () => {
      void this.refresh();
    });
    return refreshed;
  }

  expandNode(node: FileNode): Promise<void> {
    if (!node.isDirectory || this.worktreePath === null) return Promise.resolve();
    const expanded = new Set(this.s.expandedPaths);
    expanded.add(node.absolutePath);
    this.store.setState({ expandedPaths: expanded });
    return this.refresh();
  }

  collapseNode(node: FileNode): Promise<void> {
    const expanded = new Set(this.s.expandedPaths);
    expanded.delete(node.absolutePath);
    this.store.setState({ expandedPaths: expanded });
    return this.refresh();
  }

  setSearchQuery(searchQuery: string): void {
    this.store.setState({ searchQuery });
  }

  select(path: string | null): void {
    this.store.setState({ selectedFilePath: path });
  }

  refresh(): Promise<void> {
    const root = this.worktreePath;
    if (root === null) return Promise.resolve();
    this.generation += 1;
    const generation = this.generation;
    const expanded = this.s.expandedPaths;
    const host = this.hostValue;
    return this.tree(host, root, root, expanded).then((nodes) => {
      if (generation !== this.generation || this.worktreePath !== root) return;
      this.store.setState({ rootNodes: nodes });
    });
  }

  stopWatching(): void {
    this.generation += 1;
    this.stopWatcher?.();
    this.stopWatcher = null;
  }

  /** Swift `filteredTree` / `remoteTree`: one listing per directory, expanded ones recursed. */
  private async tree(
    host: FileHost,
    directory: string,
    root: string,
    expanded: ReadonlySet<string>,
  ): Promise<FileNode[]> {
    let entries: FileHostEntry[];
    try {
      entries = await host.list(directory, root);
    } catch {
      return [];
    }
    const nodes = sortedFileNodes(
      entries.filter(visibleEntry).map((entry) => ({
        name: entry.name,
        absolutePath: entry.path,
        relativePath: relativePath(root, entry.path) ?? entry.name,
        isDirectory: entry.isDirectory,
        children: [],
      })),
    );
    return Promise.all(
      nodes.map(async (node) =>
        node.isDirectory && expanded.has(node.absolutePath)
          ? { ...node, children: await this.tree(host, node.absolutePath, root, expanded) }
          : node,
      ),
    );
  }
}
