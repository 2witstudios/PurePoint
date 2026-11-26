// In-memory `WorkspacePersistence` for the registry tests (WIN-6.2): a map of files with
// modification times standing in for `.pu/workspaces.json` on disk, with the same conditional-save
// rules as Swift's `WorkspacePersistence.saveLocal`. The document is the workspaces as JSON; the
// real file format is WIN-6.3's.

import { normalizeWorkspace, pointGuardId, type Workspace } from '../model/workspace';
import type {
  HostFileData,
  LayoutDocumentData,
  RemoteSaveOutcome,
  WorkspacePersistence,
} from '../model/workspacePersistenceTypes';
import type { SaveScheduler } from './workspaceRegistry';

export const CURRENT_DOCUMENT_VERSION = 4;

export interface MemoryDocument {
  readonly version: number;
  readonly workspaces: Workspace[];
  readonly activeWorkspaceId?: string;
}

interface MemoryFile {
  readonly data: LayoutDocumentData;
  readonly modifiedMs: number;
}

export class MemoryWorkspacePersistence implements WorkspacePersistence {
  readonly maxRemoteLayoutBytes = 4 * 1_048_576;
  private readonly files = new Map<string, MemoryFile>();
  private clock = 1_000;

  filePath(projectRoot: string): string {
    return `${projectRoot}/.pu/workspaces.json`;
  }

  /** Write a file as any process would: it gets a new modification time. */
  writeFile(path: string, data: LayoutDocumentData): number {
    this.clock += 1;
    this.files.set(path, { data, modifiedMs: this.clock });
    return this.clock;
  }

  readFile(path: string): LayoutDocumentData | null {
    return this.files.get(path)?.data ?? null;
  }

  removeFile(path: string): void {
    this.files.delete(path);
  }

  loadForRegistry(projectRoot: string): Workspace[] {
    const file = this.files.get(this.filePath(projectRoot));
    return file === undefined ? [] : this.workspaces(file.data, projectRoot);
  }

  documentData(
    workspaces: readonly Workspace[],
    activeWorkspaceId: string | null,
  ): LayoutDocumentData | null {
    const document: MemoryDocument = {
      version: CURRENT_DOCUMENT_VERSION,
      workspaces: [...workspaces],
      ...(activeWorkspaceId === null ? {} : { activeWorkspaceId }),
    };
    return JSON.stringify(document, null, 2);
  }

  workspaces(data: LayoutDocumentData, projectId: string): Workspace[] {
    const document = parseDocument(data);
    if (document === null) return [];
    return document.workspaces.map((workspace) =>
      normalizeWorkspace({
        ...workspace,
        id: workspace.id.startsWith('ws-point-guard:') ? pointGuardId(projectId) : workspace.id,
      }),
    );
  }

  isNewerThanSupported(data: LayoutDocumentData): boolean {
    const document = parseDocument(data);
    return document !== null && document.version > CURRENT_DOCUMENT_VERSION;
  }

  localModifiedMs(projectRoot: string): number | null {
    return this.files.get(this.filePath(projectRoot))?.modifiedMs ?? null;
  }

  saveLocal(
    data: LayoutDocumentData,
    projectRoot: string,
    expectedModifiedMs: number | null,
  ): RemoteSaveOutcome {
    const path = this.filePath(projectRoot);
    const current = this.files.get(path);
    if (current !== undefined && current.modifiedMs !== expectedModifiedMs) {
      if (this.isNewerThanSupported(current.data)) return { kind: 'skipped' };
      return { kind: 'conflict', document: current.data, modifiedMs: current.modifiedMs };
    }
    return { kind: 'written', modifiedMs: this.writeFile(path, data) };
  }

  readLocal(path: string, maxBytes: number): HostFileData {
    const file = this.files.get(path);
    if (file === undefined) throw new Error(`not found: ${path}`);
    return {
      data: file.data.slice(0, maxBytes),
      truncated: file.data.length > maxBytes,
      modifiedMs: file.modifiedMs,
    };
  }
}

function parseDocument(data: LayoutDocumentData): MemoryDocument | null {
  try {
    const parsed: unknown = JSON.parse(data);
    if (typeof parsed !== 'object' || parsed === null) return null;
    const { version, workspaces } = parsed as Partial<MemoryDocument>;
    if (typeof version !== 'number' || !Array.isArray(workspaces)) return null;
    return parsed as MemoryDocument;
  } catch {
    return null;
  }
}

/** A save debounce that never fires on its own: tests flush with `saveAll` / `saveNow`. */
export class ManualScheduler implements SaveScheduler {
  private readonly pending = new Map<number, () => void>();
  private nextHandle = 0;

  schedule(run: () => void): unknown {
    const handle = this.nextHandle++;
    this.pending.set(handle, run);
    return handle;
  }

  cancel(handle: unknown): void {
    this.pending.delete(handle as number);
  }

  /** Run every scheduled save, as the debounce elapsing would. */
  flush(): void {
    const runs = [...this.pending.values()];
    this.pending.clear();
    for (const run of runs) run();
  }
}
