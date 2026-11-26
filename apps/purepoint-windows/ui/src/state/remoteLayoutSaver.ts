// Swift `RemoteLayoutSaver` (Views/PaneGrid/WorkspacePersistence.swift), which ProjectState owns
// for a remote project (WIN-6.0): saves of its layout through the machine's host, in order, each
// conditional on the document's modification time as this app last read or wrote it.

import type { Workspace } from '../model/workspace';
import {
  documentText,
  saveRemoteLayout,
  type LayoutHostIO,
  type LayoutSaveOutcome,
} from '../model/workspacePersistence';

export type RemoteLayoutWrite = (
  text: string,
  expectedModifiedMs: number | null,
) => Promise<LayoutSaveOutcome>;

export class RemoteLayoutSaver {
  private knownModifiedMs: number | null = null;
  private conflictHandler: ((document: string, modifiedMs: number) => void) | null = null;
  private tail: Promise<void> = Promise.resolve();
  private pending = 0;

  constructor(private readonly write: RemoteLayoutWrite) {}

  static forHost(projectRoot: string, host: LayoutHostIO): RemoteLayoutSaver {
    return new RemoteLayoutSaver((text, expected) =>
      saveRemoteLayout(host, text, projectRoot, expected),
    );
  }

  /**
   * Called with the machine's document and its modification time when a save found it changed.
   * The handler adopts it and then calls `loaded`, so writes queued behind the conflict conflict
   * too instead of reverting it.
   */
  onConflict(handler: (document: string, modifiedMs: number) => void): void {
    this.conflictHandler = handler;
  }

  /** The document as just loaded, so the next save is conditional on it. */
  loaded(modifiedMs: number | null): void {
    this.knownModifiedMs = modifiedMs;
  }

  /** Queue a write of `workspaces`; `onWritten` runs once it lands. */
  save(
    workspaces: readonly Workspace[],
    activeWorkspaceId: string | null,
    onWritten?: () => void,
  ): void {
    const text = documentText(workspaces, activeWorkspaceId);
    if (text !== null) this.enqueue(text, onWritten);
  }

  enqueue(text: string, onWritten?: () => void): void {
    this.pending += 1;
    this.tail = this.tail.then(async () => {
      try {
        const outcome = await this.write(text, this.knownModifiedMs);
        switch (outcome.kind) {
          case 'written':
            this.knownModifiedMs = outcome.modifiedMs;
            onWritten?.();
            break;
          case 'conflict':
            // The stamp advances only once the document is adopted (`loaded`).
            this.conflictHandler?.(outcome.text, outcome.modifiedMs);
            break;
          case 'skipped':
            break;
        }
      } catch {
        // A failed write is skipped, as Swift's `.skipped`.
      } finally {
        this.pending -= 1;
      }
    });
  }

  /** Whether writes are queued or in flight. */
  get isBusy(): boolean {
    return this.pending > 0;
  }

  /** Resolves true once every queued write finished, false if `timeoutMs` passed first. */
  async flush(timeoutMs: number): Promise<boolean> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<false>((resolve) => {
      timer = setTimeout(() => {
        resolve(false);
      }, timeoutMs);
    });
    try {
      for (;;) {
        const tail = this.tail;
        const done = await Promise.race([tail.then(() => true as const), timeout]);
        if (!done) return false;
        if (tail === this.tail) return true;
      }
    } finally {
      clearTimeout(timer);
    }
  }
}
