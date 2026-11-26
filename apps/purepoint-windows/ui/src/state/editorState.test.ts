// WIN-8.5: EditorState (Swift State/EditorState.swift) over an in-memory host: open, edit, the
// conditional save and its conflict, reload, external changes, and the remote read-only limit.
import { describe, expect, it } from 'vitest';
import { memoryFileHost } from '../services/fileHost.fixtures';
import { MAX_REMOTE_SAVE_BYTES } from '../services/fileHost';
import { EditorState, isTooLargeToSave } from './editorState';

const ROOT = 'C:\\repo';
const FILE = `${ROOT}\\src\\main.ts`;

function editorOn(files: Record<string, string> = { [FILE]: 'let a = 1;\n' }, isLocal = true) {
  const host = memoryFileHost(files, { isLocal });
  const editor = new EditorState();
  editor.setHost(host);
  return { host, editor, s: () => editor.store.getState() };
}

describe('EditorState', () => {
  it('given a file, should open it clean with its language and hide the changes', async () => {
    const { editor, s } = editorOn();
    expect(s().showChanges).toBe(true);
    await editor.openFile(FILE, 'main.ts');
    expect(s().currentFile).toMatchObject({
      id: FILE,
      name: 'main.ts',
      content: 'let a = 1;\n',
      isDirty: false,
      isBinary: false,
      language: 'typescript',
      isReadOnly: false,
    });
    expect(s().showChanges).toBe(false);
    expect(editor.hasUnsavedChanges).toBe(false);
  });

  it('given an edit, should be dirty until saved, and save to the host', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    editor.updateContent('let a = 2;\n');
    expect(editor.hasUnsavedChanges).toBe(true);
    await editor.saveFile();
    expect(host.files.get(FILE)?.content).toBe('let a = 2;\n');
    expect(s().currentFile?.isDirty).toBe(false);
    // A second save is checked against the time of the first.
    editor.updateContent('let a = 3;\n');
    await editor.saveFile();
    expect(host.files.get(FILE)?.content).toBe('let a = 3;\n');
  });

  it('given the file changed on disk since it was opened, should refuse the save as a conflict', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    host.files.set(FILE, { content: 'theirs', modifiedMs: 99_999, binary: false });
    editor.updateContent('mine');
    await expect(editor.saveFile()).rejects.toThrow(/changed since it was read/);
    expect(s().saveConflict).toBe(true);
    expect(host.files.get(FILE)?.content).toBe('theirs');
    expect(editor.hasUnsavedChanges).toBe(true);

    await editor.overwriteChanges();
    expect(host.files.get(FILE)?.content).toBe('mine');
    expect(s().saveConflict).toBe(false);
    expect(editor.hasUnsavedChanges).toBe(false);
  });

  it('given a conflict, should reload the version on disk', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    host.files.set(FILE, { content: 'theirs', modifiedMs: 99_999, binary: false });
    editor.updateContent('mine');
    await editor.saveFile().catch(() => undefined);
    await editor.reloadFile();
    expect(s().currentFile).toMatchObject({ content: 'theirs', isDirty: false });
    expect(s().saveConflict).toBe(false);
    // The reloaded version is the one the next save is checked against.
    editor.updateContent('again');
    await editor.saveFile();
    expect(host.files.get(FILE)?.content).toBe('again');
  });

  it('given an external change to a clean file, should reload it', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    host.writeExternally(FILE, 'from outside');
    await expect.poll(() => s().currentFile?.content).toBe('from outside');
    expect(s().currentFile?.isDirty).toBe(false);
    expect(s().externalChangeAlert).toBeNull();
  });

  it('given an external change to a dirty file, should keep the edits and raise the alert', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    editor.updateContent('mine');
    host.writeExternally(FILE, 'from outside');
    await expect.poll(() => s().externalChangeAlert).toBe(FILE);
    expect(s().currentFile?.content).toBe('mine');
    editor.dismissExternalChange();
    expect(s().externalChangeAlert).toBeNull();
  });

  it('given its own save, should not treat the write as an external change', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    editor.updateContent('mine');
    await editor.saveFile();
    host.notify(FILE);
    await editor.checkForExternalChanges();
    expect(s().externalChangeAlert).toBeNull();
    expect(s().currentFile?.content).toBe('mine');
  });

  it('given stopWatching, should ignore later changes', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    editor.stopWatching();
    host.writeExternally(FILE, 'from outside');
    await Promise.resolve();
    expect(s().currentFile?.content).toBe('let a = 1;\n');
  });

  it('given a slower read of an earlier file, should keep the file asked for last', async () => {
    const other = `${ROOT}\\b.md`;
    const { host, editor, s } = editorOn({ [FILE]: 'a', [other]: 'b' });
    const read = host.read.bind(host);
    let release: () => void = () => undefined;
    host.read = (path, max) =>
      path === FILE
        ? new Promise((resolve) => {
            release = () => {
              resolve(read(path, max));
            };
          })
        : read(path, max);
    const slow = editor.openFile(FILE, 'main.ts');
    await editor.openFile(other, 'b.md');
    release();
    await slow;
    expect(s().currentFile?.id).toBe(other);
  });

  it('given a missing or binary file, should report it', async () => {
    const { editor, s } = editorOn({ [`${ROOT}\\a.bin`]: 'x\0y' });
    await editor.openFile(`${ROOT}\\gone.txt`, 'gone.txt');
    expect(s().openError).toMatch(/^Could not open gone.txt: /);
    editor.dismissOpenError();
    expect(s().openError).toBeNull();
    await editor.openFile(`${ROOT}\\a.bin`, 'a.bin');
    expect(s().currentFile?.isBinary).toBe(true);
  });

  it('given a remote file too large to save, should open it read-only', async () => {
    const big = 'x'.repeat(MAX_REMOTE_SAVE_BYTES + 1);
    const { editor, s } = editorOn({ [FILE]: big }, false);
    await editor.openFile(FILE, 'main.ts');
    expect(s().currentFile?.isReadOnly).toBe(true);
    expect(editor.readOnlyReason).toMatch(/^Read-only: files over 750 KB/);
    editor.updateContent('changed');
    expect(editor.hasUnsavedChanges).toBe(false);
    expect(isTooLargeToSave(big, { isLocal: true } as never)).toBe(false);
  });

  it('given the file asked for again, should only hide the changes', async () => {
    const { host, editor, s } = editorOn();
    await editor.openFile(FILE, 'main.ts');
    editor.updateContent('mine');
    editor.setShowChanges(true);
    host.listed.length = 0;
    await editor.openFile(FILE, 'main.ts');
    expect(s().showChanges).toBe(false);
    expect(s().currentFile?.content).toBe('mine');
  });
});
