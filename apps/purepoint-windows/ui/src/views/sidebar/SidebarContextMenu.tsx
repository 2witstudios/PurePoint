// The sidebar's pop-up menus (WIN-7.8): a row's context menu and the worktree add menu, the Mac's
// `NSMenu`s filled by `addItems(_:for:to:)` in SidebarOutlineViewController.swift. The entries are
// exactly what `SidebarMenuModel` (model/sidebarMenu.ts) produces; choosing one hands its action
// descriptor back to the controller.
//
// An in-page menu, positioned at the pointer (or under the row for the keyboard's menu key), with
// the menu bar's look (views/shell/layout.css) and keys: arrows move, Enter chooses, Escape and a
// click elsewhere close.

import { type KeyboardEvent, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { SidebarMenuAction, SidebarMenuEntry } from '../../model/sidebarMenu.ts';

export interface SidebarMenuPosition {
  readonly x: number;
  readonly y: number;
}

export interface SidebarContextMenuProps {
  readonly entries: readonly SidebarMenuEntry[];
  readonly position: SidebarMenuPosition;
  readonly label: string;
  readonly onChoose: (action: SidebarMenuAction) => void;
  /** Closed without a choice; `refocus` when it was the keyboard (Escape). */
  readonly onClose: (refocus: boolean) => void;
}

/** Keeps the menu inside the window. */
const MARGIN = 4;

function items(menu: HTMLElement | null): HTMLButtonElement[] {
  return menu === null
    ? []
    : [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')];
}

export function SidebarContextMenu({
  entries,
  position,
  label,
  onChoose,
  onClose,
}: SidebarContextMenuProps) {
  const menu = useRef<HTMLUListElement>(null);
  const [placed, setPlaced] = useState(position);

  useLayoutEffect(() => {
    const element = menu.current;
    if (element === null) return;
    const { width, height } = element.getBoundingClientRect();
    const maxX = window.innerWidth - width - MARGIN;
    const maxY = window.innerHeight - height - MARGIN;
    setPlaced({
      x: Math.max(MARGIN, Math.min(position.x, maxX)),
      y: Math.max(MARGIN, Math.min(position.y, maxY)),
    });
    items(element)[0]?.focus();
  }, [position]);

  // A press anywhere else, or the window losing focus, closes it.
  useEffect(() => {
    const close = (event: Event) => {
      if (event.target instanceof Node && menu.current?.contains(event.target) === true) return;
      onClose(false);
    };
    const blur = () => {
      onClose(false);
    };
    document.addEventListener('pointerdown', close, true);
    document.addEventListener('contextmenu', close, true);
    window.addEventListener('blur', blur);
    return () => {
      document.removeEventListener('pointerdown', close, true);
      document.removeEventListener('contextmenu', close, true);
      window.removeEventListener('blur', blur);
    };
  }, [onClose]);

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const list = items(menu.current);
    const index = list.findIndex((item) => item === document.activeElement);
    switch (event.key) {
      case 'ArrowDown':
        list[(index + 1) % list.length]?.focus();
        break;
      case 'ArrowUp':
        list[(index - 1 + list.length) % list.length]?.focus();
        break;
      case 'Home':
        list[0]?.focus();
        break;
      case 'End':
        list[list.length - 1]?.focus();
        break;
      case 'Escape':
        onClose(true);
        break;
      case 'Tab':
        onClose(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  return (
    <ul
      ref={menu}
      className="menu-dropdown sidebar-context-menu"
      role="menu"
      aria-label={label}
      data-testid="sidebar-context-menu"
      style={{ left: placed.x, top: placed.y }}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      {entries.map((entry, index) =>
        entry.type === 'separator' ? (
          <li key={`separator-${String(index)}`} role="separator" />
        ) : (
          <li key={`${entry.title}-${String(index)}`} role="none">
            <button
              type="button"
              role="menuitem"
              disabled={!entry.isEnabled}
              onClick={() => {
                onChoose(entry.action);
              }}
            >
              <span className="menu-item-label">{entry.title}</span>
            </button>
          </li>
        ),
      )}
    </ul>
  );
}
