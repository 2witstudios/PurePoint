// The editor column shared by FilePaneView and the worktree and root checkout views (WIN-8.5): the
// save-error banner, EditorNoticeBanners, the "File changed on disk." banner, then the open file
// (the editor, the binary placeholder, or `renderFile`'s alternative such as a Markdown preview)
// or the empty state. Swift draws this in each view (`FilePaneView.content`,
// `WorktreeDetailView.editorContent`); the three are the same.
import { type ReactNode, useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import type { EditorTab } from '../../model/editorTab';
import { fileHostErrorMessage, type FileHost } from '../../services/fileHost';
import { EditorState } from '../../state/editorState';
import type { SfSymbolName } from '../../theme/icons';
import { Icon } from '../../theme/Icon';
import { EditorNSView } from '../editor/EditorNSView';
import { EditorNotice, EditorNoticeBanners } from '../editor/EditorNoticeBanners';
import './filePane.css';

export function FilePanePlaceholder({ text, icon }: { text: string; icon: SfSymbolName }) {
  return (
    <div className="file-pane-placeholder">
      <Icon name={icon} size={28} className="file-pane-placeholder-icon" />
      <span>{text}</span>
    </div>
  );
}

export interface FileEditorContentProps {
  editor: EditorState;
  /** The empty state's text (Swift: "Select a file" in a pane, "Select a file to edit" in a view). */
  emptyText: string;
  autoFocus?: boolean;
  /** Draw the file some other way (the Markdown preview); null draws the editor. */
  renderFile?: (file: EditorTab) => ReactNode;
}

export function FileEditorContent({
  editor,
  emptyText,
  autoFocus = false,
  renderFile,
}: FileEditorContentProps) {
  const file = useStore(editor.store, (s) => s.currentFile);
  const externalChange = useStore(editor.store, (s) => s.externalChangeAlert);
  const [saveError, setSaveError] = useState<string | null>(null);

  const save = () => {
    editor.saveFile().then(
      () => {
        setSaveError(null);
      },
      (error: unknown) => {
        // A conflict has its own banner (Reload / Overwrite).
        if (editor.store.getState().saveConflict) return;
        setSaveError(`Save failed: ${fileHostErrorMessage(error)}`);
      },
    );
  };

  const alternative = file !== null && renderFile !== undefined ? renderFile(file) : null;
  return (
    <div className="file-editor-content">
      {saveError !== null && (
        <EditorNotice text={saveError} icon="exclamationmark.triangle.fill" tone="error">
          <button
            type="button"
            onClick={() => {
              setSaveError(null);
            }}
          >
            Dismiss
          </button>
        </EditorNotice>
      )}
      <EditorNoticeBanners editor={editor} onSaveError={setSaveError} />
      {externalChange !== null && (
        <EditorNotice
          text="File changed on disk."
          icon="exclamationmark.triangle.fill"
          tone="caution"
          testId="editor-external-change"
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
              editor.dismissExternalChange();
            }}
          >
            Keep mine
          </button>
        </EditorNotice>
      )}
      {file === null ? (
        <FilePanePlaceholder text={emptyText} icon="doc.text" />
      ) : file.isBinary ? (
        <FilePanePlaceholder text="Binary file — can't be shown" icon="doc.fill" />
      ) : alternative !== null ? (
        alternative
      ) : (
        <EditorNSView
          // One editor per file: switching files starts a fresh undo history.
          key={file.id}
          content={file.content}
          language={file.language}
          isEditable={!file.isReadOnly}
          autoFocus={autoFocus}
          onContentChanged={(content) => {
            editor.updateContent(content);
          }}
          onSave={save}
        />
      )}
    </div>
  );
}

/**
 * A view's own EditorState (the worktree and root checkout views): a new one per root, on the
 * given host, its watcher stopped when the view goes (Swift `.task(id:)` and `.onDisappear`).
 */
export function useEditorState(rootPath: string, host: FileHost): EditorState {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a new editor per root and host
  const editor = useMemo(() => new EditorState(), [rootPath, host.identity]);
  editor.setHost(host);
  useEffect(
    () => () => {
      editor.stopWatching();
    },
    [editor],
  );
  return editor;
}
