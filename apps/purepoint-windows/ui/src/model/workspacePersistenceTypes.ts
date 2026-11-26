// The part of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/WorkspacePersistence.swift that
// `WorkspaceRegistry` calls (WIN-6.2), as an interface the registry is given. WIN-6.3 implements it
// over `.pu/workspaces.json`; registry tests use an in-memory fake.
//
// Every member mirrors one Swift member the registry uses, with the same name and argument order,
// except where noted. Swift's `Data` (the encoded layout document) is the document's JSON text here.
// Calls are synchronous, as in Swift: an implementation over async file I/O keeps what it needs in
// memory (or the registry gains async variants when that lands).

import type { Workspace } from './workspace';

/** An encoded layout document: the JSON text of `.pu/workspaces.json` (Swift `Data`). */
export type LayoutDocumentData = string;

/** What a conditional layout save did (Swift `RemoteSaveOutcome`). */
export type RemoteSaveOutcome =
  | { readonly kind: 'written'; readonly modifiedMs: number }
  /**
   * The document changed since this app last read or wrote it; it was not overwritten. Carries
   * that document for the caller to adopt.
   */
  | {
      readonly kind: 'conflict';
      readonly document: LayoutDocumentData;
      readonly modifiedMs: number;
    }
  /** Nothing written: the document could not be read safely, or is a newer version. */
  | { readonly kind: 'skipped' };

/** A file as a project host read it (Swift `HostFileData`). */
export interface HostFileData {
  readonly data: LayoutDocumentData;
  /** The file was longer than the `maxBytes` asked for, so `data` is incomplete. */
  readonly truncated: boolean;
  readonly modifiedMs: number;
}

export interface WorkspacePersistence {
  /** `<projectRoot>/.pu/workspaces.json`. */
  filePath(projectRoot: string): string;

  /**
   * The local project's stored layout (migrating older documents), `[]` when there is none.
   * Throws when the layout exists but cannot be restored safely (an identity migration that
   * could not be written): the registry then reports the error rather than adopt an empty layout.
   */
  loadForRegistry(projectRoot: string): Workspace[];

  /** The encoded layout document, or `null` when it cannot be encoded. */
  documentData(
    workspaces: readonly Workspace[],
    activeWorkspaceId: string | null,
  ): LayoutDocumentData | null;

  /**
   * Decode a layout document; `[]` when it does not decode. `projectId` keys Point Guard
   * workspaces. (Swift `workspaces(from:projectId:)`.)
   */
  workspaces(data: LayoutDocumentData, projectId: string): Workspace[];

  /** Whether a document was written by a newer build (its version is above the supported one). */
  isNewerThanSupported(data: LayoutDocumentData): boolean;

  /** Modification time of a local project's layout document, `null` when there is none. */
  localModifiedMs(projectRoot: string): number | null;

  /**
   * Write a local project's layout unless the document on disk changed since this app last read
   * or wrote it (`expectedModifiedMs`, `null` when there was no document); a changed document is
   * returned as a conflict to adopt. A document deleted meanwhile is written again.
   */
  saveLocal(
    data: LayoutDocumentData,
    projectRoot: string,
    expectedModifiedMs: number | null,
  ): RemoteSaveOutcome;

  /** The largest layout document read through a host (4 MiB). */
  readonly maxRemoteLayoutBytes: number;

  /**
   * Read a local file, at most `maxBytes` of it. Throws when it cannot be read. Stands in for
   * Swift's `LocalProjectHost.shared.read(path:maxBytes:)`, the one host call the registry makes.
   */
  readLocal(path: string, maxBytes: number): HostFileData;
}
