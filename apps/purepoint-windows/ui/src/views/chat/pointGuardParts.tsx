// Small pieces the Point Guard views share (WIN-8.13): the typing dots and bubble
// (apps/shared/ChatMarkdown/ChatBubble.swift's `TypingDots`/`TypingIndicatorBubble`), a button
// that opens a menu (SwiftUI `Menu`), a context menu, a sheet with a Done button (`.sheet`) and a
// confirmation dialog (`.confirmationDialog`).
import {
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react';

/** Three dots rising in turn; static under reduced motion (CSS). */
export function TypingDots({ diameter = 7, spacing = 5 }: { diameter?: number; spacing?: number }) {
  return (
    <span
      className="pg-typing-dots"
      aria-hidden="true"
      style={{ '--pg-dot': `${String(diameter)}px`, gap: spacing } as CSSProperties}
    >
      <span />
      <span />
      <span />
    </span>
  );
}

/** Shown at the end of the transcript while Point Guard is working. */
export function TypingIndicatorBubble({ compact = false }: { compact?: boolean }) {
  return (
    <div className="pg-typing-bubble-row" role="status" aria-label="Point Guard is typing">
      <span className="pg-typing-bubble" data-compact={compact}>
        <TypingDots diameter={compact ? 6 : 7} spacing={compact ? 4 : 5} />
      </span>
    </div>
  );
}

export interface MenuEntry {
  readonly label: string;
  readonly onSelect?: () => void;
  readonly disabled?: boolean;
  /** A divider before this entry. */
  readonly separated?: boolean;
  /** A submenu (SwiftUI's nested `Menu`), shown inline under its label. */
  readonly items?: readonly MenuEntry[];
}

function closeOnOutside(ref: React.RefObject<HTMLElement | null>, close: () => void) {
  const onDown = (event: PointerEvent) => {
    if (ref.current !== null && !ref.current.contains(event.target as Node)) close();
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') close();
  };
  window.addEventListener('pointerdown', onDown, true);
  window.addEventListener('keydown', onKey, true);
  return () => {
    window.removeEventListener('pointerdown', onDown, true);
    window.removeEventListener('keydown', onKey, true);
  };
}

function MenuItems({ items, close }: { items: readonly MenuEntry[]; close: () => void }) {
  return (
    <>
      {items.map((item, index) => (
        <div key={`${item.label}-${String(index)}`} role="none">
          {item.separated === true && <div className="pg-menu-separator" role="separator" />}
          {item.items !== undefined ? (
            <div role="group" aria-label={item.label} className="pg-menu-group">
              <div className="pg-menu-group-label">{item.label}</div>
              <MenuItems items={item.items} close={close} />
            </div>
          ) : (
            <button
              type="button"
              role="menuitem"
              className="pg-menu-item"
              disabled={item.disabled === true || item.onSelect === undefined}
              onClick={() => {
                close();
                item.onSelect?.();
              }}
            >
              {item.label}
            </button>
          )}
        </div>
      ))}
    </>
  );
}

/** A button that opens a menu below it (aligned to its trailing edge). */
export function MenuButton({
  label,
  items,
  children,
  className,
  disabled = false,
  title,
}: {
  label: string;
  items: readonly MenuEntry[];
  children: ReactNode;
  className?: string;
  disabled?: boolean;
  title?: string;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const close = useCallback(() => {
    setOpen(false);
  }, []);
  useEffect(() => (open ? closeOnOutside(anchor, close) : undefined), [open, close]);
  return (
    <div className="pg-menu-anchor" ref={anchor}>
      <button
        type="button"
        className={className ?? 'pg-icon-button'}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        title={title}
        disabled={disabled}
        onClick={() => {
          setOpen((value) => !value);
        }}
      >
        {children}
      </button>
      {open && (
        <div className="pg-menu" role="menu" aria-label={label}>
          <MenuItems items={items} close={close} />
        </div>
      )}
    </div>
  );
}

/** A right-click menu at the pointer (SwiftUI `.contextMenu`). */
export function useContextMenu(items: () => readonly MenuEntry[]) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const menu = useRef<HTMLDivElement>(null);
  const close = useCallback(() => {
    setAt(null);
  }, []);
  useEffect(() => (at !== null ? closeOnOutside(menu, close) : undefined), [at, close]);
  const onContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    setAt({ x: event.clientX, y: event.clientY });
  };
  const element =
    at === null ? null : (
      <div
        ref={menu}
        className="pg-menu pg-context-menu"
        role="menu"
        style={{ position: 'fixed', left: at.x, top: at.y }}
      >
        <MenuItems items={items()} close={close} />
      </div>
    );
  return { onContextMenu, element };
}

/** A modal sheet with a title and Done (Escape closes it). */
export function ChatSheet({
  title,
  onDone,
  children,
  testId,
  width = 620,
}: {
  title: string;
  onDone: () => void;
  children: ReactNode;
  testId?: string;
  width?: number;
}) {
  const sheet = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    sheet.current?.focus();
  }, []);
  return (
    <div
      className="pg-sheet-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onDone();
      }}
    >
      <div
        ref={sheet}
        tabIndex={-1}
        className="pg-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testId}
        style={{ width }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onDone();
          }
        }}
      >
        <div className="pg-sheet-header">
          <h2>{title}</h2>
          <button type="button" className="pg-button" onClick={onDone}>
            Done
          </button>
        </div>
        <div className="pg-sheet-body">{children}</div>
      </div>
    </div>
  );
}

/** A confirmation with one action and Cancel. */
export function ConfirmDialog({
  title,
  action,
  onConfirm,
  onCancel,
}: {
  title: string;
  action: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const confirm = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    confirm.current?.focus();
  }, []);
  return (
    <div
      className="pg-sheet-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="pg-confirm"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onCancel();
          }
        }}
      >
        <p className="pg-confirm-title">{title}</p>
        <div className="pg-confirm-buttons">
          <button
            type="button"
            ref={confirm}
            className="pg-button pg-button-primary"
            onClick={onConfirm}
          >
            {action}
          </button>
          <button type="button" className="pg-button" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

/** Whether the user asked for reduced motion (`accessibilityReduceMotion`). */
export function useReducedMotion(): boolean {
  const query = '(prefers-reduced-motion: reduce)';
  // jsdom has no matchMedia.
  const match = (window as { matchMedia?: Window['matchMedia'] }).matchMedia;
  const [reduced, setReduced] = useState(() => match?.call(window, query).matches ?? false);
  useEffect(() => {
    if (match === undefined) return;
    const media = match.call(window, query);
    const onChange = () => {
      setReduced(media.matches);
    };
    media.addEventListener('change', onChange);
    return () => {
      media.removeEventListener('change', onChange);
    };
  }, [match]);
  return reduced;
}

/** The element's size, following resizes. */
export function useElementSize<T extends HTMLElement>(): [
  React.RefObject<T | null>,
  { width: number; height: number },
] {
  const ref = useRef<T>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      setSize((old) =>
        old.width === rect.width && old.height === rect.height
          ? old
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);
  return [ref, size];
}
