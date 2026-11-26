// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/FileTreeSidebarView.swift (WIN-8.4): the
// file navigator column of a file pane and of the worktree and root checkout views: a header with
// the hide button and "Files", the search field, and the outline.
import { useStore } from 'zustand';
import type { FileTreeState } from '../../state/fileTreeState';
import { Icon } from '../../theme/Icon';
import { FileTreeOutlineView } from './FileTreeOutlineView';
import './filetree.css';

export interface FileTreeSidebarViewProps {
  fileTreeState: FileTreeState;
  onHide: () => void;
  onFileSelected: (path: string, name: string) => void;
}

export function FileTreeSidebarView({
  fileTreeState,
  onHide,
  onFileSelected,
}: FileTreeSidebarViewProps) {
  const searchQuery = useStore(fileTreeState.store, (s) => s.searchQuery);
  return (
    <div className="file-tree-sidebar" data-view="FileTreeSidebarView">
      <div className="file-tree-sidebar-header">
        <button
          type="button"
          className="file-tree-icon-button"
          aria-label="Hide file tree"
          title="Hide file tree"
          onClick={onHide}
        >
          <Icon name="sidebar.left" size={14} />
        </button>
        <span className="file-tree-sidebar-title">Files</span>
      </div>
      <input
        className="file-tree-search"
        type="search"
        placeholder="Search files..."
        aria-label="Search files"
        value={searchQuery}
        onChange={(event) => {
          fileTreeState.setSearchQuery(event.target.value);
        }}
      />
      <div className="file-tree-divider" />
      <FileTreeOutlineView fileTreeState={fileTreeState} onFileSelected={onFileSelected} />
    </div>
  );
}
