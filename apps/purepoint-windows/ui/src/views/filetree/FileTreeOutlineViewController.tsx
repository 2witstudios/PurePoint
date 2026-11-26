// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/FileTreeOutlineViewController.swift
// (WIN-8.4): the file tree's outline. NSOutlineView is a virtualized list here
// (@tanstack/react-virtual, 24 px rows indented 12 px a level), so a folder of thousands of files
// draws only the rows on screen.
//
// - Clicking a file selects it and opens it (Swift `outlineViewSelectionDidChange`); clicking a
//   folder, its chevron, or Right/Left expands and collapses it. Up/Down move the selection, Enter
//   opens.
// - The context menu (Swift `menuNeedsUpdate`): Open, then on this PC Open in Default App and
//   Reveal in Explorer (Finder's), on a machine Open Local Copy (WIN-8.4 leaves it out: see
//   FileTreeOutlineView), then Copy Path and Copy Relative Path. Rename… (this PC) and Delete… are
//   Windows additions the leaf asks for.
import { useVirtualizer } from '@tanstack/react-virtual';
import { type KeyboardEvent, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { detectEditorLanguage } from '../../model/editorLanguage';
import { editorLanguageIcon } from '../../model/editorTab';
import type { FileNode } from '../../model/fileNode';
import { Icon } from '../../theme/Icon';
import { FILE_TREE_INDENT, FILE_TREE_ROW_HEIGHT, type FileTreeRow } from './fileTreeRows';

export interface FileTreeActions {
  onFileSelected(path: string, name: string): void;
  onNodeExpanded(node: FileNode): void;
  onNodeCollapsed(node: FileNode): void;
  onOpenInDefaultApp(node: FileNode): void;
  onRevealInExplorer(node: FileNode): void;
  onCopyPath(node: FileNode): void;
  onCopyRelativePath(node: FileNode): void;
  onRename(node: FileNode): void;
  onDelete(node: FileNode): void;
}

export interface FileTreeMenuItem {
  readonly id: string;
  readonly title: string;
  readonly perform: () => void;
  readonly separatorBefore?: boolean;
}

/** Swift `menuNeedsUpdate`, plus Open, Rename… and Delete…. */
export function fileTreeMenuItems(
  node: FileNode,
  options: { isLocal: boolean; canRename: boolean },
  actions: FileTreeActions,
): FileTreeMenuItem[] {
  const items: FileTreeMenuItem[] = [];
  if (!node.isDirectory) {
    items.push({
      id: 'open',
      title: 'Open',
      perform: () => {
        actions.onFileSelected(node.absolutePath, node.name);
      },
    });
  }
  if (options.isLocal) {
    if (!node.isDirectory) {
      items.push({
        id: 'openDefault',
        title: 'Open in Default App',
        perform: () => {
          actions.onOpenInDefaultApp(node);
        },
      });
    }
    items.push({
      id: 'reveal',
      title: 'Reveal in Explorer',
      perform: () => {
        actions.onRevealInExplorer(node);
      },
    });
  }
  items.push(
    {
      id: 'copyPath',
      title: 'Copy Path',
      separatorBefore: items.length > 0,
      perform: () => {
        actions.onCopyPath(node);
      },
    },
    {
      id: 'copyRelativePath',
      title: 'Copy Relative Path',
      perform: () => {
        actions.onCopyRelativePath(node);
      },
    },
  );
  if (options.canRename) {
    items.push({
      id: 'rename',
      title: 'Rename…',
      separatorBefore: true,
      perform: () => {
        actions.onRename(node);
      },
    });
  }
  items.push({
    id: 'delete',
    title: 'Delete…',
    separatorBefore: !options.canRename,
    perform: () => {
      actions.onDelete(node);
    },
  });
  return items;
}

function FileTreeContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: readonly FileTreeMenuItem[];
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: x, top: y });

  useLayoutEffect(() => {
    const element = menu.current;
    if (element === null) return;
    const rect = element.getBoundingClientRect();
    setPosition({
      left: Math.max(4, Math.min(x, window.innerWidth - rect.width - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - rect.height - 4)),
    });
    element.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
  }, [x, y]);

  useEffect(() => {
    const close = (event: Event) => {
      if (event.target instanceof Node && menu.current?.contains(event.target)) return;
      onClose();
    };
    const closeNow = () => {
      onClose();
    };
    window.addEventListener('pointerdown', close, true);
    window.addEventListener('blur', closeNow);
    window.addEventListener('resize', closeNow);
    return () => {
      window.removeEventListener('pointerdown', close, true);
      window.removeEventListener('blur', closeNow);
      window.removeEventListener('resize', closeNow);
    };
  }, [onClose]);

  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const buttons = [
      ...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? []),
    ];
    const index = buttons.findIndex((button) => button === document.activeElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      buttons[(index + step + buttons.length) % buttons.length]?.focus();
    }
  };

  return (
    <div
      ref={menu}
      className="file-tree-menu"
      role="menu"
      data-testid="file-tree-menu"
      style={{ left: position.left, top: position.top }}
      onKeyDown={move}
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      {items.map((item) => (
        <div key={item.id}>
          {item.separatorBefore === true && <div className="file-tree-menu-separator" />}
          <button
            type="button"
            role="menuitem"
            className="file-tree-menu-item"
            onClick={() => {
              onClose();
              item.perform();
            }}
          >
            {item.title}
          </button>
        </div>
      ))}
    </div>
  );
}

export interface FileTreeOutlineViewControllerProps {
  rows: readonly FileTreeRow[];
  selectedPath: string | null;
  isLocal: boolean;
  canRename: boolean;
  actions: FileTreeActions;
  onSelect: (path: string | null) => void;
}

export function FileTreeOutlineViewController({
  rows,
  selectedPath,
  isLocal,
  canRename,
  actions,
  onSelect,
}: FileTreeOutlineViewControllerProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; node: FileNode } | null>(null);
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => FILE_TREE_ROW_HEIGHT,
    overscan: 12,
    // Layout is unknown before the first measure (and in tests): draw a screenful.
    initialRect: { width: 240, height: 600 },
  });
  const selectedIndex = rows.findIndex((row) => row.node.absolutePath === selectedPath);

  const activate = (row: FileTreeRow) => {
    const { node } = row;
    onSelect(node.absolutePath);
    if (!node.isDirectory) actions.onFileSelected(node.absolutePath, node.name);
    else if (row.expanded) actions.onNodeCollapsed(node);
    else actions.onNodeExpanded(node);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const row = selectedIndex >= 0 ? rows[selectedIndex] : undefined;
    const go = (index: number) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, index))];
      if (target === undefined) return;
      onSelect(target.node.absolutePath);
      virtualizer.scrollToIndex(rows.indexOf(target));
    };
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        go(selectedIndex + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        go(selectedIndex < 0 ? 0 : selectedIndex - 1);
        break;
      case 'ArrowRight':
        if (row?.node.isDirectory === true && !row.expanded) {
          event.preventDefault();
          actions.onNodeExpanded(row.node);
        }
        break;
      case 'ArrowLeft':
        if (row?.node.isDirectory === true && row.expanded) {
          event.preventDefault();
          actions.onNodeCollapsed(row.node);
        }
        break;
      case 'Enter':
        if (row !== undefined) {
          event.preventDefault();
          activate(row);
        }
        break;
      case 'ContextMenu':
        if (row !== undefined) {
          event.preventDefault();
          const rect = (event.target as HTMLElement).getBoundingClientRect();
          setMenu({ x: rect.left + 16, y: rect.top + 16, node: row.node });
        }
        break;
    }
  };

  return (
    <div
      ref={scroller}
      className="file-tree-outline"
      role="tree"
      aria-label="Files"
      tabIndex={0}
      data-testid="file-tree-outline"
      onKeyDown={onKeyDown}
    >
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((item) => {
          const row = rows[item.index];
          if (row === undefined) return null;
          const { node } = row;
          const icon = node.isDirectory
            ? 'folder.fill'
            : editorLanguageIcon(detectEditorLanguage(node.name));
          const selected = node.absolutePath === selectedPath;
          return (
            <div
              key={node.absolutePath}
              className="file-tree-row"
              role="treeitem"
              aria-level={row.depth + 1}
              aria-expanded={node.isDirectory ? row.expanded : undefined}
              aria-selected={selected}
              aria-label={node.name}
              title={node.relativePath}
              data-testid="file-tree-row"
              data-path={node.absolutePath}
              data-relative-path={node.relativePath}
              data-directory={node.isDirectory}
              data-selected={selected}
              style={{
                transform: `translateY(${String(item.start)}px)`,
                height: FILE_TREE_ROW_HEIGHT,
                paddingLeft: 4 + row.depth * FILE_TREE_INDENT,
              }}
              onClick={() => {
                activate(row);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                onSelect(node.absolutePath);
                setMenu({ x: event.clientX, y: event.clientY, node });
              }}
            >
              <span className="file-tree-disclosure">
                {node.isDirectory && (
                  <Icon name={row.expanded ? 'chevron.down' : 'chevron.right'} size={10} />
                )}
              </span>
              <Icon
                name={icon}
                size={12}
                className={node.isDirectory ? 'file-tree-icon-folder' : 'file-tree-icon'}
                label={node.isDirectory ? 'Folder' : 'File'}
              />
              <span className="file-tree-name">{node.name}</span>
            </div>
          );
        })}
      </div>
      {menu !== null && (
        <FileTreeContextMenu
          x={menu.x}
          y={menu.y}
          items={fileTreeMenuItems(menu.node, { isLocal, canRename }, actions)}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
    </div>
  );
}
