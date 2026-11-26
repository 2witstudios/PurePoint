// The remote folder picker's state (WIN-8.14): a machine's folders, listed through its daemon's
// directories-only `fs_list` (`fsBrowseRequest`), for choosing a folder to open as a project.
// The behaviour follows the iPad's `RemoteFolderBrowser` (RemoteFolderPicker+iOS.swift, the Mac
// panel's history): start at the home folder, hidden folders off, Finder-like sort, enter, up
// (selecting the folder just left), back and forward.
//
// Paths are the machine's own: `/`-separated on a Unix machine, `\` on a Windows one.

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Response } from '../protocol';
import { fsBrowseRequest } from './machineStateOpenProject';
import type { DaemonRequester } from '../services/daemonConnection';

export interface RemoteFolder {
  readonly name: string;
  readonly path: string;
  readonly isSymlink: boolean;
}

export interface RemoteFolderBrowserFields {
  /** The folder listed; null before the first listing. */
  readonly path: string | null;
  /** The machine user's home, from the first listing. */
  readonly home: string | null;
  readonly folders: readonly RemoteFolder[];
  readonly selection: string | null;
  readonly showHidden: boolean;
  readonly loading: boolean;
  readonly error: string | null;
  readonly truncated: boolean;
  readonly backStack: readonly string[];
  readonly forwardStack: readonly string[];
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Finder's order: case-insensitive, numbers by value (`alpha2` before `alpha10`). */
export function sortFolders(folders: readonly RemoteFolder[]): RemoteFolder[] {
  return [...folders].sort((a, b) => collator.compare(a.name, b.name));
}

/** The folder above `path` on the machine, or null at a root (`/`, `C:\`). */
export function parentFolder(path: string): string | null {
  const separator = path.includes('\\') || /^[A-Za-z]:/.test(path) ? '\\' : '/';
  const trimmed = path.length > 1 ? path.replace(/[\\/]+$/, '') : path;
  if (trimmed === '/' || /^[A-Za-z]:\\?$/.test(trimmed) || trimmed === '') return null;
  const index = trimmed.lastIndexOf(separator);
  if (index === -1) return null;
  if (index === 0) return '/';
  const parent = trimmed.slice(0, index);
  return /^[A-Za-z]:$/.test(parent) ? parent + '\\' : parent;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

export class RemoteFolderBrowser {
  readonly store: StoreApi<RemoteFolderBrowserFields>;
  private readonly list: (path: string, includeHidden: boolean) => Promise<Response>;
  private request = 0;

  constructor(list: (path: string, includeHidden: boolean) => Promise<Response>) {
    this.list = list;
    this.store = createStore<RemoteFolderBrowserFields>()(() => ({
      path: null,
      home: null,
      folders: [],
      selection: null,
      showHidden: false,
      loading: false,
      error: null,
      truncated: false,
      backStack: [],
      forwardStack: [],
    }));
  }

  /** A browser over a machine's daemon. */
  static forClient(client: DaemonRequester): RemoteFolderBrowser {
    return new RemoteFolderBrowser((path, includeHidden) =>
      client.send(fsBrowseRequest(path, includeHidden)),
    );
  }

  get state(): RemoteFolderBrowserFields {
    return this.store.getState();
  }

  /** Start at the machine user's home folder. */
  async start(): Promise<void> {
    this.store.setState({ backStack: [], forwardStack: [], selection: null });
    await this.load('~', null);
  }

  /** List `path`; the folder left goes on the back stack. */
  async navigate(path: string, select: string | null = null): Promise<void> {
    const previous = this.state.path;
    if (await this.load(path, select)) {
      if (previous !== null && previous !== this.state.path) {
        this.store.setState({ backStack: [...this.state.backStack, previous], forwardStack: [] });
      }
    }
  }

  enter(folder: RemoteFolder): Promise<void> {
    return this.navigate(folder.path);
  }

  /** The enclosing folder, selecting the one just left. */
  async goUp(): Promise<void> {
    const path = this.state.path;
    const parent = path === null ? null : parentFolder(path);
    if (parent === null || path === null) return;
    await this.navigate(parent, path);
  }

  async goBack(): Promise<void> {
    const { backStack, path } = this.state;
    const target = backStack.at(-1);
    if (target === undefined || path === null) return;
    if (await this.load(target, null)) {
      this.store.setState({
        backStack: backStack.slice(0, -1),
        forwardStack: [...this.state.forwardStack, path],
      });
    }
  }

  async goForward(): Promise<void> {
    const { forwardStack, path } = this.state;
    const target = forwardStack.at(-1);
    if (target === undefined || path === null) return;
    if (await this.load(target, null)) {
      this.store.setState({
        forwardStack: forwardStack.slice(0, -1),
        backStack: [...this.state.backStack, path],
      });
    }
  }

  select(path: string | null): void {
    this.store.setState({ selection: path });
  }

  /** List again with or without hidden folders, keeping a selection that is still listed. */
  async setShowHidden(show: boolean): Promise<void> {
    this.store.setState({ showHidden: show });
    const path = this.state.path;
    if (path !== null) await this.load(path, this.state.selection);
  }

  /** The folder Open opens: the selection, else the folder listed. */
  get chosenPath(): string | null {
    return this.state.selection ?? this.state.path;
  }

  /** One listing; false when it failed or a newer one superseded it. */
  private async load(path: string, select: string | null): Promise<boolean> {
    const request = ++this.request;
    this.store.setState({ loading: true, error: null });
    let response: Response;
    try {
      response = await this.list(path, this.state.showHidden);
    } catch (error) {
      if (request !== this.request) return false;
      this.store.setState({ loading: false, error: errorMessage(error) });
      return false;
    }
    if (request !== this.request) return false;
    if (response.type !== 'fs_list_result') {
      this.store.setState({
        loading: false,
        error: response.type === 'error' ? response.message : 'Unexpected reply to fs_list',
      });
      return false;
    }
    const folders = sortFolders(
      response.entries
        .filter((entry) => entry.is_dir)
        .map((entry) => ({ name: entry.name, path: entry.path, isSymlink: entry.is_symlink })),
    );
    const selection =
      select !== null && folders.some((folder) => folder.path === select) ? select : null;
    this.store.setState({
      path: response.path,
      home: this.state.home ?? (path === '~' ? response.path : null),
      folders,
      selection,
      loading: false,
      error: null,
      truncated: response.truncated,
    });
    return true;
  }
}
