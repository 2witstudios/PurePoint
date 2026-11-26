// Placeholders and shared pieces of the detail screens (WIN-7.1, WIN-8.2).
import type { ReactNode } from 'react';
import { Icon } from '../../theme/Icon.tsx';
import type { SfSymbolName } from '../../theme/icons.ts';

/** A screen that is not ported yet: its title, and the view that will draw it. */
export function ScreenPlaceholder({
  icon,
  title,
  view,
}: {
  icon: SfSymbolName;
  title: string;
  view?: string;
}) {
  return (
    <div className="detail-placeholder" data-view={view}>
      <Icon name={icon} size={40} className="detail-placeholder-icon" />
      <span className="detail-placeholder-title">{title}</span>
      {view !== undefined && <span className="detail-placeholder-view">{view}</span>}
    </div>
  );
}

/** Swift's centred icon-and-caption empty states (`editorPlaceholder`, `binaryPlaceholder`). */
export function EmptyState({ icon, children }: { icon: SfSymbolName; children: ReactNode }) {
  return (
    <div className="detail-empty">
      <Icon name={icon} size={28} className="detail-empty-icon" />
      {children}
    </div>
  );
}

/** Swift `editorPlaceholder`: no file open. The editor itself is WIN-8.5. */
export function EditorPlaceholder() {
  return (
    <EmptyState icon="doc.text">
      <span>Select a file to edit</span>
    </EmptyState>
  );
}

/** Swift `FileTreeSidebarView` (WIN-8.4): its header and hide button; the tree comes with 8.4. */
export function FileTreeSidebarPlaceholder({ onHide }: { onHide: () => void }) {
  return (
    <div className="detail-file-tree">
      <div className="detail-file-tree-header">
        <button
          type="button"
          className="detail-icon-button"
          aria-label="Hide file tree"
          title="Hide file tree"
          onClick={onHide}
        >
          <Icon name="sidebar.left" size={14} />
        </button>
        <span>Files</span>
      </div>
      <ScreenPlaceholder icon="folder" title="Files" view="FileTreeSidebarView" />
    </div>
  );
}
