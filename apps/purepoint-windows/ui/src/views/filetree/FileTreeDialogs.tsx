// The file tree's Rename…, Delete… and failure alerts (WIN-8.4), drawn like the Mac's `NSAlert`
// sheets: a modal card over the window, the default button first and focused, Escape cancels.
import { type ReactNode, useEffect, useRef, useState } from 'react';
import type { FileNode } from '../../model/fileNode';
import { Icon } from '../../theme/Icon';

function Alert({
  title,
  message,
  children,
  onCancel,
  testId,
}: {
  title: string;
  message?: string;
  children: ReactNode;
  onCancel: () => void;
  testId: string;
}) {
  return (
    <div
      className="file-tree-alert-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="file-tree-alert"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testId}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onCancel();
          }
        }}
      >
        <Icon name="exclamationmark.triangle.fill" size={28} className="file-tree-alert-icon" />
        <h2>{title}</h2>
        {message !== undefined && <p>{message}</p>}
        {children}
      </div>
    </div>
  );
}

/** A file or folder name: one path component (Windows' rules: none of `\/:*?"<>|`). */
export function validFileName(name: string): boolean {
  const trimmed = name.trim();
  return (
    trimmed !== '' &&
    trimmed !== '.' &&
    trimmed !== '..' &&
    !/[\\/:*?"<>|]/.test(trimmed) &&
    // eslint-disable-next-line no-control-regex -- control characters are not allowed in names
    !/[\u0000-\u001f]/.test(trimmed)
  );
}

export function FileTreeRenameDialog({
  name,
  onRename,
  onCancel,
}: {
  name: string;
  onRename: (name: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(name);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const element = input.current;
    if (element === null) return;
    element.focus();
    // Select the stem, as Explorer does.
    const dot = name.lastIndexOf('.');
    element.setSelectionRange(0, dot > 0 ? dot : name.length);
  }, [name]);
  const valid = validFileName(value);
  return (
    <Alert title={`Rename ${name}`} onCancel={onCancel} testId="file-tree-rename">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (valid) onRename(value.trim());
        }}
      >
        <input
          ref={input}
          className="file-tree-alert-input"
          aria-label="New name"
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
        <div className="file-tree-alert-buttons">
          <button type="submit" className="file-tree-alert-default" disabled={!valid}>
            Rename
          </button>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </form>
    </Alert>
  );
}

export function FileTreeDeleteDialog({
  node,
  onDelete,
  onCancel,
}: {
  node: FileNode;
  onDelete: () => void;
  onCancel: () => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    button.current?.focus();
  }, []);
  return (
    <Alert
      title={`Delete ${node.name}?`}
      message={
        node.isDirectory
          ? 'The folder and everything in it will be deleted. This cannot be undone.'
          : 'The file will be deleted. This cannot be undone.'
      }
      onCancel={onCancel}
      testId="file-tree-delete"
    >
      <div className="file-tree-alert-buttons">
        <button
          type="button"
          ref={button}
          className="file-tree-alert-default file-tree-alert-destructive"
          onClick={onDelete}
        >
          Delete
        </button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </Alert>
  );
}

export function FileTreeErrorDialog({
  title,
  message,
  onDismiss,
}: {
  title: string;
  message: string;
  onDismiss: () => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    button.current?.focus();
  }, []);
  return (
    <Alert title={title} message={message} onCancel={onDismiss} testId="file-tree-error">
      <div className="file-tree-alert-buttons">
        <button type="button" ref={button} className="file-tree-alert-default" onClick={onDismiss}>
          OK
        </button>
      </div>
    </Alert>
  );
}
