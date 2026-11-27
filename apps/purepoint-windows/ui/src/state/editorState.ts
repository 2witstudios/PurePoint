// Port of apps/purepoint-macos/purepoint-macos/State/EditorState.swift (WIN-8.5): the file an
// editor has open, its unsaved edits, and the notices around it (could not open, read-only,
// changed on disk, save refused).
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in `store`. The class satisfies
// `FileTabEditor`, so a file tab's session (fileTabStore.ts) keeps it while the tab is hidden and
// the close confirmation sees its unsaved edits.
//
// Differences from Swift:
// - Every save is conditional, local ones too: it is checked against the modification time of the
//   version shown (`fs_write_text_if_unmodified`), and a file changed on disk since is a
//   `saveConflict` (Reload or Overwrite) rather than silently overwritten. Swift checks only remote
//   saves.
// - The file's folder is watched through the host (`file-tree-changed` locally, a 5 s poll on a
//   machine); a change stats the file: a clean file reloads, a dirty one raises
//   `externalChangeAlert`.
// - Swift's `OpenEditors` registry is the registry's file tab sessions (`unsavedFileTabNames`).

import { createStore, type StoreApi } from 'zustand/vanilla';
import { detectEditorLanguage } from '../model/editorLanguage';
import type { EditorTab } from '../model/editorTab';
import { parentPath, samePath } from '../model/fileNode';
import {
  changeTouches,
  fileHostErrorMessage,
  FileWriteConflictError,
  localFileHost,
  MAX_REMOTE_SAVE_BYTES,
  type FileHost,
} from '../services/fileHost';

export interface EditorFields {
  readonly currentFile: EditorTab | null;
  readonly showChanges: boolean;
  /** The path of an open file with unsaved edits that changed on disk. */
  readonly externalChangeAlert: string | null;
  /** Why the last file asked for could not be opened (too large, unreadable, …). */
  readonly openError: string | null;
  /** A save was refused because the file changed since it was opened: Reload or Overwrite. */
  readonly saveConflict: boolean;
}

/** Swift `isTooLargeToSave`: a remote save travels in one request line. */
export function isTooLargeToSave(content: string, host: FileHost): boolean {
  return !host.isLocal && new TextEncoder().encode(content).length > MAX_REMOTE_SAVE_BYTES;
}

export class EditorState {
  readonly store: StoreApi<EditorFields>;
  private hostValue: FileHost = localFileHost;
  /** The modification time of the open file as last read or written, for conflict checks. */
  private loadedModifiedMs: number | null = null;
  /** The file most recently asked for: a slower earlier read must not replace it. */
  private requestedPath: string | null = null;
  private stopWatcher: (() => void) | null = null;
  private watchedDirectory: string | null = null;

  constructor() {
    this.store = createStore<EditorFields>()(() => ({
      currentFile: null,
      showChanges: true,
      externalChangeAlert: null,
      openError: null,
      saveConflict: false,
    }));
  }

  private get s(): EditorFields {
    return this.store.getState();
  }

  /** The open file now, read fresh after an await (no narrowing carried across it). */
  private fileNow(): EditorTab | null {
    return this.store.getState().currentFile;
  }

  get currentFile(): EditorTab | null {
    return this.s.currentFile;
  }

  get host(): FileHost {
    return this.hostValue;
  }

  /** Where opened files live. Set before opening. */
  setHost(host: FileHost): void {
    if (host === this.hostValue) return;
    // A file watched on another host is no longer the one this editor reads.
    if (host.identity !== this.hostValue.identity) this.stopWatching();
    this.hostValue = host;
  }

  get hasUnsavedChanges(): boolean {
    return this.s.currentFile?.isDirty === true;
  }

  get readOnlyReason(): string | null {
    return this.s.currentFile?.isReadOnly === true
      ? 'Read-only: files over 750 KB cannot be saved on a remote machine.'
      : null;
  }

  setShowChanges(showChanges: boolean): void {
    this.store.setState({ showChanges });
  }

  private patchFile(patch: Partial<EditorTab>): void {
    const file = this.s.currentFile;
    if (file !== null) this.store.setState({ currentFile: { ...file, ...patch } });
  }

  async openFile(path: string, name: string): Promise<void> {
    // Set first: re-selecting the open file must also win over a slower read of another file.
    this.requestedPath = path;
    if (this.s.currentFile?.id === path) {
      this.store.setState({ showChanges: false });
      return;
    }
    const host = this.hostValue;
    try {
      const result = await host.read(path);
      if (this.requestedPath !== path) return;
      // The stamp and the content come from the same read, so a save is checked against exactly
      // the version shown.
      this.loadedModifiedMs = result.modifiedMs;
      this.store.setState({
        openError: null,
        saveConflict: false,
        externalChangeAlert: null,
        showChanges: false,
        currentFile: {
          id: path,
          name,
          content: result.content,
          isDirty: false,
          lastModified: result.modifiedMs,
          isBinary: result.isBinary,
          language: detectEditorLanguage(name),
          isReadOnly: isTooLargeToSave(result.content, host),
        },
      });
      this.watchFile(path);
    } catch (error) {
      if (this.requestedPath !== path) return;
      this.store.setState({ openError: `Could not open ${name}: ${fileHostErrorMessage(error)}` });
    }
  }

  updateContent(content: string): void {
    const file = this.s.currentFile;
    if (file === null || file.isReadOnly || file.content === content) return;
    this.patchFile({ content, isDirty: true });
  }

  /** Save, unless the file changed since the version shown (then `saveConflict`). */
  saveFile(): Promise<void> {
    return this.save(this.loadedModifiedMs);
  }

  /** Save over a version changed since it was opened (the user chose to). */
  async overwriteChanges(): Promise<void> {
    const file = this.s.currentFile;
    if (file === null) return;
    const current = await this.hostValue.stat(file.id).catch(() => null);
    await this.save(current);
  }

  private async save(expectedModifiedMs: number | null): Promise<void> {
    const file = this.s.currentFile;
    if (file === null || file.isReadOnly || file.isBinary) return;
    try {
      const modified = await this.hostValue.write(file.id, file.content, expectedModifiedMs);
      this.store.setState({ saveConflict: false, externalChangeAlert: null });
      // Another file opened while this save was in flight keeps its own stamp.
      if (this.s.currentFile?.id !== file.id) return;
      this.loadedModifiedMs = modified;
      // Edits typed while the save was in flight stay unsaved.
      this.patchFile({
        isDirty: this.s.currentFile.content !== file.content,
        lastModified: modified,
      });
    } catch (error) {
      if (error instanceof FileWriteConflictError) this.store.setState({ saveConflict: true });
      throw error;
    }
  }

  async reloadFile(): Promise<void> {
    const file = this.s.currentFile;
    if (file === null) return;
    this.store.setState({ externalChangeAlert: null });
    const host = this.hostValue;
    try {
      const result = await host.read(file.id);
      if (this.s.currentFile?.id !== file.id) return;
      this.loadedModifiedMs = result.modifiedMs;
      this.store.setState({ saveConflict: false });
      this.patchFile({
        content: result.content,
        isDirty: false,
        isBinary: result.isBinary,
        isReadOnly: isTooLargeToSave(result.content, host),
        lastModified: result.modifiedMs,
      });
    } catch (error) {
      this.store.setState({
        openError: `Could not reload ${file.name}: ${fileHostErrorMessage(error)}`,
      });
    }
  }

  dismissOpenError(): void {
    this.store.setState({ openError: null });
  }

  dismissExternalChange(): void {
    this.store.setState({ externalChangeAlert: null });
  }

  stopWatching(): void {
    this.stopWatcher?.();
    this.stopWatcher = null;
    this.watchedDirectory = null;
  }

  // MARK: - File watching

  private watchFile(path: string): void {
    const directory = parentPath(path);
    if (this.watchedDirectory !== null && samePath(this.watchedDirectory, directory)) return;
    this.stopWatching();
    this.watchedDirectory = directory;
    this.stopWatcher = this.hostValue.watch(directory, (paths) => {
      const file = this.s.currentFile;
      if (file !== null && changeTouches(paths, file.id)) void this.checkForExternalChanges();
    });
  }

  /** Swift `checkForExternalChanges`: a newer file reloads when clean, else raises the alert. */
  async checkForExternalChanges(): Promise<void> {
    const file = this.s.currentFile;
    if (file === null || file.lastModified === null) return;
    const host = this.hostValue;
    const current = await host.stat(file.id).catch(() => null);
    if (current === null || current <= file.lastModified || this.s.currentFile?.id !== file.id) {
      return;
    }
    if (this.s.currentFile.isDirty) {
      this.store.setState({ externalChangeAlert: file.id });
      return;
    }
    try {
      const result = await host.read(file.id);
      // Typing may have started (or another file opened) during the read.
      const now = this.fileNow();
      if (now?.id !== file.id || now.isDirty) return;
      this.loadedModifiedMs = result.modifiedMs;
      this.patchFile({
        content: result.content,
        isBinary: result.isBinary,
        lastModified: result.modifiedMs,
      });
    } catch {
      // Swift ignores a failed reload; the next change tries again.
    }
  }
}
