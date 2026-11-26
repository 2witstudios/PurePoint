// Port of apps/purepoint-macos/purepoint-macos/ContentView.swift (WIN-7.1).
//
// The window: a flush sidebar column and the detail, both running the full window height under a
// transparent title bar band (WindowChrome) that holds the menu bar, the sidebar toggle and the
// caption buttons. The sidebar is resizable (its width persists, sidebarWidth.ts); the banners
// overlay the top; Settings overlays everything. Layout is CSS (layout.css), which replaces the
// Mac's Auto Layout helper (Services/NSView+Constraints.swift).
//
// Differences from Swift:
// - The sidebar outline is WIN-7.2 (`SidebarView`); until then the column lists the nav items.
// - Agent signals (`updateAgentSignals`, `pendingSignalDestination`) are WIN-7.7; Point Guard's
//   working directory (`pointGuardService`) is WIN-9.x; spawn selection (`pendingSelectAgentId`)
//   and the session's workspace restore follow the pane grid (WIN-7.4).
// - The menu commands' accelerators are handled here, for every command (Mac menus handle theirs
//   and `HotkeyMonitor` the rest; WIN-7.6 ports the monitor for terminal-focus rules).
import { type CSSProperties, useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import {
  SIDEBAR_NAV_ITEMS,
  type SidebarSelection,
  navItemTitle,
  selection as sidebarSelection,
  selectionsEqual,
} from '../../model/sidebarItem.ts';
import type { AppState } from '../../state/appState.ts';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import { SIDEBAR_KEY, useSidebarVisibilityStore } from '../../state/sidebarVisibilityStore.ts';
import { Icon } from '../../theme/Icon.tsx';
import { runWindowAction } from '../../services/windowChrome.ts';
import { DetailView, ScreenPlaceholder } from '../detail/DetailView.tsx';
import { useAppState } from './appContext.ts';
import { type AppCommand, createAppActions } from './appCommands.ts';
import { MenuBar } from './MenuBar.tsx';
import { ShellBanners } from './ShellBanners.tsx';
import {
  SIDEBAR_IDEAL_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  useSidebarWidthStore,
} from './sidebarWidth.ts';
import {
  CAPTION_BUTTONS_WIDTH,
  TITLE_BAR_HEIGHT,
  type TitlebarClearance,
  TitlebarClearanceProvider,
} from './TitlebarClearance.tsx';
import { WindowChrome } from './WindowChrome.tsx';
import './layout.css';

const CLEARANCE: TitlebarClearance = { trailing: CAPTION_BUTTONS_WIDTH, height: TITLE_BAR_HEIGHT };
/** Arrow keys on the resize handle move the sidebar edge this far. */
const KEYBOARD_RESIZE_STEP = 10;

/** Keeps a drag's pointer on the handle; jsdom (tests) has no pointer capture. */
function capturePointer(element: Element, pointerId: number, capture: boolean): void {
  try {
    if (capture) element.setPointerCapture(pointerId);
    else element.releasePointerCapture(pointerId);
  } catch {
    // Not supported, or the pointer is already gone.
  }
}

/** Swift `.onChange(of: selection)`: the active project, and the workspace on screen. */
function followSelection(app: AppState, selection: SidebarSelection | null): void {
  app.updateActiveProject(selection);
  const registry = app.registry;
  if (selection?.kind === 'nav' && selection.item === 'dashboard') {
    app.activatePointGuardWorkspace();
  } else if (registry !== null) {
    if (selection?.kind === 'workspace') registry.activate(selection.id);
    else registry.activeWorkspaceId = null;
  }
}

/** The drag handle between the sidebar and the detail. */
export function SidebarResizer({ store = useSidebarWidthStore }) {
  const width = useStore(store, (state) => state.width);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);

  return (
    <div
      className="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuemin={SIDEBAR_MIN_WIDTH}
      aria-valuemax={SIDEBAR_MAX_WIDTH}
      aria-valuenow={width}
      tabIndex={0}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        drag.current = { startX: event.clientX, startWidth: store.getState().width };
        capturePointer(event.currentTarget, event.pointerId, true);
      }}
      onPointerMove={(event) => {
        const start = drag.current;
        if (start === null) return;
        store.getState().preview(start.startWidth + event.clientX - start.startX);
      }}
      onPointerUp={(event) => {
        const start = drag.current;
        if (start === null) return;
        drag.current = null;
        capturePointer(event.currentTarget, event.pointerId, false);
        store.getState().commit(start.startWidth + event.clientX - start.startX);
      }}
      onPointerCancel={() => {
        if (drag.current === null) return;
        drag.current = null;
        store.getState().commit(store.getState().width);
      }}
      onDoubleClick={() => {
        store.getState().commit(SIDEBAR_IDEAL_WIDTH);
      }}
      onKeyDown={(event) => {
        const current = store.getState().width;
        const next =
          event.key === 'ArrowLeft'
            ? current - KEYBOARD_RESIZE_STEP
            : event.key === 'ArrowRight'
              ? current + KEYBOARD_RESIZE_STEP
              : event.key === 'Home'
                ? SIDEBAR_MIN_WIDTH
                : event.key === 'End'
                  ? SIDEBAR_MAX_WIDTH
                  : null;
        if (next === null) return;
        event.preventDefault();
        store.getState().commit(next);
      }}
    />
  );
}

/** Stands in for the sidebar outline (WIN-7.2): the nav rows, so the shell can be driven. */
function SidebarPlaceholder({
  selection,
  onSelect,
}: {
  selection: SidebarSelection | null;
  onSelect: (selection: SidebarSelection) => void;
}) {
  return (
    <nav className="sidebar-placeholder" aria-label="Sidebar">
      {SIDEBAR_NAV_ITEMS.map((item) => {
        const row = sidebarSelection.nav(item);
        return (
          <button
            type="button"
            key={item}
            className="sidebar-nav-row"
            aria-current={selectionsEqual(selection, row) ? 'page' : undefined}
            onClick={() => {
              onSelect(row);
            }}
          >
            {navItemTitle(item)}
          </button>
        );
      })}
      <ProjectListPlaceholder />
      <span className="sidebar-placeholder-view">SidebarView</span>
    </nav>
  );
}

/** Interim project list (the end-to-end smoke test reads it); WIN-7.2's outline replaces it. */
function ProjectListPlaceholder() {
  const app = useAppState();
  const projects = useStore(app.store, (state) => state.projects);
  return (
    <ul data-testid="project-list" className="sidebar-project-list">
      {projects.map((project) => (
        <li key={project.projectRoot} title={project.projectRoot}>
          {project.projectRoot.split(/[\\/]/).filter(Boolean).pop() ?? project.projectRoot}
        </li>
      ))}
    </ul>
  );
}

/** Settings as an overlay (Swift `appState.showSettings`); the screen itself is WIN-8.11. */
function SettingsOverlay({ onClose }: { onClose: () => void }) {
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    panel.current?.focus();
  }, []);
  return (
    <div
      className="settings-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        tabIndex={-1}
        ref={panel}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onClose();
          }
        }}
      >
        <ScreenPlaceholder icon="gear" title="Settings" view="SettingsView" />
      </div>
    </div>
  );
}

function closeWindow(): void {
  try {
    runWindowAction('close');
  } catch (error) {
    console.error('PurePoint: window close failed', error);
  }
}

export function ContentView() {
  const app = useAppState();
  const sidebarShown = useSidebarVisibilityStore((state) => state.sidebarShown);
  const sidebarWidth = useSidebarWidthStore((state) => state.width);
  const showSettings = useStore(app.store, (state) => state.showSettings);
  const [selection, setSelection] = useState<SidebarSelection | null>(() =>
    sidebarSelection.nav('dashboard'),
  );
  const sidebarRef = useRef<HTMLElement>(null);
  const detailRef = useRef<HTMLElement>(null);
  /** A focus request that arrived while the sidebar was hidden. */
  const sidebarFocusPending = useRef(false);

  useEffect(() => {
    if (sidebarShown && sidebarFocusPending.current) {
      sidebarFocusPending.current = false;
      sidebarRef.current?.focus();
    }
  }, [sidebarShown]);

  useEffect(() => {
    followSelection(app, selection);
  }, [app, selection]);

  // Swift `.onChange(of: appState.pendingSelectWorktreeId)`.
  useEffect(
    () =>
      app.store.subscribe((state) => {
        const worktreeId = state.pendingSelectWorktreeId;
        if (worktreeId === null) return;
        app.setPendingSelectWorktreeId(null);
        setSelection(sidebarSelection.worktree(worktreeId));
      }),
    [app],
  );

  const runCommand = useCallback(
    (command: AppCommand) => {
      const actions = createAppActions({
        selectNav: (item) => {
          setSelection(sidebarSelection.nav(item));
        },
        toggleSidebar: (action) => {
          useSidebarVisibilityStore.getState().toggleFor(action);
        },
        showSettings: () => {
          app.setShowSettings(true);
        },
        focusSidebar: () => {
          const visibility = useSidebarVisibilityStore.getState();
          if (visibility.sidebarShown) {
            sidebarRef.current?.focus();
          } else {
            // The column mounts when it reappears; focus it then.
            sidebarFocusPending.current = true;
            visibility.toggle(SIDEBAR_KEY);
          }
        },
        focusContent: () => {
          detailRef.current?.focus();
        },
        closeWindow,
      });
      actions[command]();
    },
    [app],
  );

  // Menu accelerators from the key binding store.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat) return;
      const bindings = useKeyBindingStore.getState();
      if (bindings.recordingAction !== null) return;
      const target = event.target instanceof Element ? event.target : null;
      const focusInTerminal = target?.closest('.xterm') != null;
      const action = bindings.actionFor(event, focusInTerminal);
      if (action === null) return;
      event.preventDefault();
      runCommand(action);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [runCommand]);

  const style = { '--sidebar-width': `${String(sidebarWidth)}px` } as CSSProperties;

  return (
    <TitlebarClearanceProvider value={CLEARANCE}>
      <div className="shell" style={style} data-sidebar-shown={sidebarShown}>
        {sidebarShown && (
          <>
            <aside className="sidebar-column" ref={sidebarRef} tabIndex={-1}>
              <SidebarPlaceholder selection={selection} onSelect={setSelection} />
            </aside>
            <SidebarResizer />
          </>
        )}
        <main className="detail-column" ref={detailRef} tabIndex={-1}>
          <DetailView selection={selection} />
        </main>
        <WindowChrome>
          <MenuBar onCommand={runCommand} />
          <button
            type="button"
            className="title-bar-button"
            data-chrome-interactive
            aria-label={sidebarShown ? 'Hide Sidebar' : 'Show Sidebar'}
            title={sidebarShown ? 'Hide Sidebar' : 'Show Sidebar'}
            onClick={() => {
              runCommand('toggleSidebar');
            }}
          >
            <Icon name="sidebar.left" />
          </button>
        </WindowChrome>
        <ShellBanners />
        {showSettings && (
          <SettingsOverlay
            onClose={() => {
              app.setShowSettings(false);
            }}
          />
        )}
      </div>
    </TitlebarClearanceProvider>
  );
}
