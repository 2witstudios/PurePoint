// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/FileTreeOutlineView.swift (WIN-8.4):
// binds a FileTreeState to the outline (FileTreeOutlineViewController) and performs its menu
// actions: expand and collapse through the state, Open in Default App and Reveal in Explorer
// through the opener (`NSWorkspace` on the Mac; a program or script is never opened, Reveal is
// offered instead), Copy Path through the clipboard, and Rename… and
// Delete… through the host after a dialog.
//
// Difference from Swift: a remote file's Open Local Copy (fetch it to a temp file, then open that)
// is not offered; Copy Path names the file on its machine.
import { useState } from 'react';
import { useStore } from 'zustand';
import { joinPath, parentPath, type FileNode } from '../../model/fileNode';
import { fileHostErrorMessage } from '../../services/fileHost';
import { openInDefaultApp, revealInExplorer } from '../../services/opener';
import type { FileTreeState } from '../../state/fileTreeState';
import {
  FileTreeBlockedOpenDialog,
  FileTreeDeleteDialog,
  FileTreeErrorDialog,
  FileTreeRenameDialog,
} from './FileTreeDialogs';
import {
  FileTreeOutlineViewController,
  type FileTreeActions,
} from './FileTreeOutlineViewController';
import { fileTreeRows } from './fileTreeRows';

export interface FileTreeOutlineViewProps {
  fileTreeState: FileTreeState;
  onFileSelected: (path: string, name: string) => void;
}

function copy(text: string): void {
  void navigator.clipboard.writeText(text).catch(() => undefined);
}

type Pending =
  | { readonly kind: 'rename'; readonly node: FileNode }
  | { readonly kind: 'delete'; readonly node: FileNode }
  | { readonly kind: 'blocked'; readonly node: FileNode }
  | { readonly kind: 'error'; readonly title: string; readonly message: string };

export function FileTreeOutlineView({ fileTreeState, onFileSelected }: FileTreeOutlineViewProps) {
  const rootNodes = useStore(fileTreeState.store, (s) => s.rootNodes);
  const expandedPaths = useStore(fileTreeState.store, (s) => s.expandedPaths);
  const searchQuery = useStore(fileTreeState.store, (s) => s.searchQuery);
  const selectedPath = useStore(fileTreeState.store, (s) => s.selectedFilePath);
  const [pending, setPending] = useState<Pending | null>(null);
  const host = fileTreeState.host;
  const rows = fileTreeRows(rootNodes, expandedPaths, searchQuery);

  const actions: FileTreeActions = {
    onFileSelected,
    onNodeExpanded: (node) => void fileTreeState.expandNode(node),
    onNodeCollapsed: (node) => void fileTreeState.collapseNode(node),
    onOpenInDefaultApp: (node) => {
      // A program or script would run: offer Reveal in Explorer instead.
      if (openInDefaultApp(node.absolutePath) === 'blocked') setPending({ kind: 'blocked', node });
    },
    onRevealInExplorer: (node) => {
      revealInExplorer(node.absolutePath);
    },
    onCopyPath: (node) => {
      copy(node.absolutePath);
    },
    onCopyRelativePath: (node) => {
      copy(node.relativePath);
    },
    onRename: (node) => {
      setPending({ kind: 'rename', node });
    },
    onDelete: (node) => {
      setPending({ kind: 'delete', node });
    },
  };

  const fail = (title: string, error: unknown) => {
    setPending({ kind: 'error', title, message: fileHostErrorMessage(error) });
  };

  return (
    <>
      <FileTreeOutlineViewController
        rows={rows}
        selectedPath={selectedPath}
        isLocal={host.isLocal}
        canRename={host.rename !== null}
        actions={actions}
        onSelect={(path) => {
          fileTreeState.select(path);
        }}
      />
      {pending?.kind === 'rename' && (
        <FileTreeRenameDialog
          name={pending.node.name}
          onCancel={() => {
            setPending(null);
          }}
          onRename={(name) => {
            const { node } = pending;
            setPending(null);
            const rename = host.rename;
            if (rename === null || name === node.name) return;
            const target = joinPath(parentPath(node.absolutePath), name);
            rename(node.absolutePath, target).then(
              () => fileTreeState.refresh(),
              (error: unknown) => {
                fail(`Could not rename ${node.name}`, error);
              },
            );
          }}
        />
      )}
      {pending?.kind === 'delete' && (
        <FileTreeDeleteDialog
          node={pending.node}
          onCancel={() => {
            setPending(null);
          }}
          onDelete={() => {
            const { node } = pending;
            setPending(null);
            host.remove(node.absolutePath).then(
              () => fileTreeState.refresh(),
              (error: unknown) => {
                fail(`Could not delete ${node.name}`, error);
              },
            );
          }}
        />
      )}
      {pending?.kind === 'blocked' && (
        <FileTreeBlockedOpenDialog
          node={pending.node}
          onCancel={() => {
            setPending(null);
          }}
          onReveal={() => {
            const { node } = pending;
            setPending(null);
            revealInExplorer(node.absolutePath);
          }}
        />
      )}
      {pending?.kind === 'error' && (
        <FileTreeErrorDialog
          title={pending.title}
          message={pending.message}
          onDismiss={() => {
            setPending(null);
          }}
        />
      )}
    </>
  );
}
