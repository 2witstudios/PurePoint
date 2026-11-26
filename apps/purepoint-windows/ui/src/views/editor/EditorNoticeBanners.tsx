// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/EditorNoticeBanners.swift (WIN-8.5): the
// notices an editor needs whatever view hosts it: a file that could not be opened, a remote file
// opened read-only, and a save refused because the file changed since it was opened (Reload or
// Overwrite). The Mac's refused save is remote-only; every save is conditional here, so a local
// file reads "changed on disk".
import type { ReactNode } from 'react';
import { useStore } from 'zustand';
import { fileHostErrorMessage } from '../../services/fileHost';
import type { EditorState } from '../../state/editorState';
import type { SfSymbolName } from '../../theme/icons';
import { Icon } from '../../theme/Icon';
import './editor.css';

export function EditorNotice({
  text,
  icon,
  tone,
  children,
  testId,
}: {
  text: string;
  icon: SfSymbolName;
  tone: 'warning' | 'caution' | 'neutral' | 'error';
  children?: ReactNode;
  testId?: string;
}) {
  return (
    <div className="editor-notice" data-tone={tone} role="status" data-testid={testId}>
      <Icon name={icon} size={12} className="editor-notice-icon" />
      <span className="editor-notice-text">{text}</span>
      <span className="editor-notice-actions">{children}</span>
    </div>
  );
}

export function EditorNoticeBanners({
  editor,
  onSaveError = () => undefined,
}: {
  editor: EditorState;
  /** Reports the result of an overwrite to the hosting view's save-error banner. */
  onSaveError?: (message: string | null) => void;
}) {
  const openError = useStore(editor.store, (s) => s.openError);
  const saveConflict = useStore(editor.store, (s) => s.saveConflict);
  const file = useStore(editor.store, (s) => s.currentFile);
  const readOnlyReason = file?.isReadOnly === true ? editor.readOnlyReason : null;

  return (
    <>
      {openError !== null && (
        <EditorNotice text={openError} icon="exclamationmark.triangle.fill" tone="warning">
          <button
            type="button"
            onClick={() => {
              editor.dismissOpenError();
            }}
          >
            Dismiss
          </button>
        </EditorNotice>
      )}
      {readOnlyReason !== null && (
        <EditorNotice text={readOnlyReason} icon="lock.fill" tone="neutral" />
      )}
      {saveConflict && file !== null && (
        <EditorNotice
          text={
            editor.host.isLocal
              ? `${file.name} changed on disk since it was opened.`
              : `${file.name} changed on the remote machine since it was opened.`
          }
          icon="exclamationmark.triangle.fill"
          tone="caution"
          testId="editor-save-conflict"
        >
          <button
            type="button"
            onClick={() => {
              void editor.reloadFile();
            }}
          >
            Reload
          </button>
          <button
            type="button"
            onClick={() => {
              editor.overwriteChanges().then(
                () => {
                  onSaveError(null);
                },
                (error: unknown) => {
                  onSaveError(`Failed to save ${file.name}: ${fileHostErrorMessage(error)}`);
                },
              );
            }}
          >
            Overwrite
          </button>
        </EditorNotice>
      )}
    </>
  );
}
