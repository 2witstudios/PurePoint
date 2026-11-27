// The in-app menu bar in the title bar (WIN-7.1): the menus of AppCommands.swift with Windows
// accelerators from the key binding store. See appCommands.ts for why it is in-app.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import {
  APP_MENUS,
  MENU_SEPARATOR,
  acceleratorLabel,
  commandLabel,
  type AppCommand,
  type AppMenu,
} from './appCommands.ts';

export interface MenuBarProps {
  onCommand: (command: AppCommand) => void;
  menus?: readonly AppMenu[];
  /** Commands shown disabled (Check for Updates… while an update runs, Swift `.disabled`). */
  isDisabled?: (command: AppCommand) => boolean;
}

function menuItems(container: HTMLElement | null): HTMLButtonElement[] {
  return container === null
    ? []
    : [...container.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
}

export function MenuBar({ onCommand, menus = APP_MENUS, isDisabled }: MenuBarProps) {
  const bindings = useKeyBindingStore((state) => state.bindings);
  const [openId, setOpenId] = useState<string | null>(null);
  /** Whether opening the menu should focus its first item (it was opened from the keyboard). */
  const focusFirst = useRef(false);
  const barRef = useRef<HTMLDivElement>(null);
  const dropdownRef = useRef<HTMLUListElement>(null);

  // A click anywhere else closes the open menu.
  useEffect(() => {
    if (openId === null) return;
    const close = (event: PointerEvent) => {
      if (!barRef.current?.contains(event.target as Node)) setOpenId(null);
    };
    document.addEventListener('pointerdown', close);
    return () => {
      document.removeEventListener('pointerdown', close);
    };
  }, [openId]);

  useEffect(() => {
    if (openId !== null && focusFirst.current) {
      focusFirst.current = false;
      menuItems(dropdownRef.current)[0]?.focus();
    }
  }, [openId]);

  const open = (id: string, viaKeyboard: boolean) => {
    focusFirst.current = viaKeyboard;
    setOpenId(id);
  };

  const close = (refocus: boolean) => {
    const id = openId;
    setOpenId(null);
    if (refocus && id !== null) {
      barRef.current?.querySelector<HTMLButtonElement>(`[data-menu="${id}"]`)?.focus();
    }
  };

  const neighbour = (offset: number): string | null => {
    const index = menus.findIndex((menu) => menu.id === openId);
    if (index === -1) return null;
    return menus[(index + offset + menus.length) % menus.length]?.id ?? null;
  };

  const onDropdownKeyDown = (event: KeyboardEvent) => {
    const items = menuItems(dropdownRef.current);
    const index = items.findIndex((item) => item === document.activeElement);
    switch (event.key) {
      case 'ArrowDown':
        items[(index + 1) % items.length]?.focus();
        break;
      case 'ArrowUp':
        items[(index - 1 + items.length) % items.length]?.focus();
        break;
      case 'ArrowLeft':
      case 'ArrowRight': {
        const next = neighbour(event.key === 'ArrowLeft' ? -1 : 1);
        if (next !== null) open(next, true);
        break;
      }
      case 'Escape':
        close(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  return (
    <div className="menu-bar" role="menubar" ref={barRef} data-chrome-interactive>
      {menus.map((menu) => {
        const isOpen = menu.id === openId;
        return (
          <div className="menu-bar-menu" key={menu.id}>
            <button
              type="button"
              className="menu-bar-title"
              data-menu={menu.id}
              aria-haspopup="menu"
              aria-expanded={isOpen}
              onClick={(event) => {
                if (isOpen) close(false);
                else open(menu.id, event.detail === 0);
              }}
              onPointerEnter={() => {
                if (openId !== null && !isOpen) open(menu.id, false);
              }}
              onKeyDown={(event) => {
                if (event.key === 'ArrowDown') {
                  event.preventDefault();
                  open(menu.id, true);
                } else if (event.key === 'Escape' && isOpen) {
                  close(true);
                }
              }}
            >
              {menu.title}
            </button>
            {isOpen && (
              <ul
                className="menu-dropdown"
                role="menu"
                aria-label={menu.title}
                ref={dropdownRef}
                onKeyDown={onDropdownKeyDown}
              >
                {menu.entries.map((entry, index) =>
                  entry === MENU_SEPARATOR ? (
                    <li key={`separator-${String(index)}`} role="separator" />
                  ) : (
                    <li key={entry} role="none">
                      <button
                        type="button"
                        role="menuitem"
                        disabled={isDisabled?.(entry) === true}
                        onClick={() => {
                          close(false);
                          onCommand(entry);
                        }}
                      >
                        <span className="menu-item-label">{commandLabel(entry)}</span>
                        <span className="menu-item-accelerator">
                          {acceleratorLabel(entry, bindings)}
                        </span>
                      </button>
                    </li>
                  ),
                )}
              </ul>
            )}
          </div>
        );
      })}
    </div>
  );
}
