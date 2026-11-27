// Port of apps/purepoint-macos/purepoint-macos/Views/WindowChrome.swift (WIN-7.1).
//
// The Mac makes its content span a transparent title bar so the sidebar reaches the top edge. The
// Windows window has no title bar at all (`decorations: false`, DEC-10), so this draws one: a
// transparent band over the top of the sidebar and detail columns holding the menu bar, the
// sidebar toggle and Windows 11 caption buttons (Segoe Fluent Icons glyphs).
//
// The band reports its layout to the backend (`chrome_set_layout`), which answers Windows' hit
// tests over it: HTCAPTION over empty space (drag, Aero Snap, double-click to maximize, the
// window menu on right-click) and HTMAXBUTTON over maximize, so hovering it opens Snap Layouts.
// Elements marked `data-chrome-interactive` keep their clicks. The maximize button's mouse input
// therefore goes to the backend, which reports hover/press back; its click handler still serves
// the keyboard. `data-tauri-drag-region` keeps the band draggable if the backend's part is absent.
import {
  type ReactNode,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  type ChromeLayout,
  isMaximized,
  onCaptionButtons,
  onWindowResized,
  rectOf,
  runWindowAction,
  setChromeLayout,
} from '../../services/windowChrome.ts';

/** Measures the band for `chrome_set_layout`, in client CSS px. */
export function measureChrome(header: HTMLElement): ChromeLayout | null {
  const maximize = header.querySelector('[data-chrome-maximize]');
  if (maximize === null) return null;
  return {
    caption: rectOf(header),
    maximize: rectOf(maximize),
    interactive: [...header.querySelectorAll('[data-chrome-interactive]')].map(rectOf),
  };
}

function ignore(): void {
  // Outside Tauri (tests, a plain browser) there is no backend to tell.
}

/** Keeps the backend's copy of the band's layout current. */
function useReportLayout(ref: RefObject<HTMLElement | null>) {
  const last = useRef('');
  useLayoutEffect(() => {
    const header = ref.current;
    if (header === null) return;
    const report = () => {
      const layout = measureChrome(header);
      if (layout === null) return;
      const key = JSON.stringify(layout);
      if (key === last.current) return;
      last.current = key;
      setChromeLayout(layout).catch(ignore);
    };
    report();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(report);
    if (observer !== null) {
      observer.observe(header);
      for (const element of header.querySelectorAll('[data-chrome-interactive]')) {
        observer.observe(element);
      }
    }
    window.addEventListener('resize', report);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', report);
    };
  });
}

/** Whether the window is maximized, following every resize. */
function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    let active = true;
    let stop: (() => void) | null = null;
    const refresh = () => {
      isMaximized()
        .then((value) => {
          if (active) setMaximized(value);
        })
        .catch(ignore);
    };
    try {
      refresh();
      onWindowResized(refresh)
        .then((unlisten) => {
          if (active) stop = unlisten;
          else unlisten();
        })
        .catch(ignore);
    } catch {
      // No Tauri window (tests, a plain browser).
    }
    return () => {
      active = false;
      stop?.();
    };
  }, []);
  return maximized;
}

function useCaptionButtonState() {
  const [state, setState] = useState({ maximizeHover: false, maximizePressed: false });
  useEffect(() => {
    let active = true;
    let stop: (() => void) | null = null;
    onCaptionButtons((next) => {
      if (active) setState(next);
    })
      .then((unlisten) => {
        if (active) stop = unlisten;
        else unlisten();
      })
      .catch(ignore);
    return () => {
      active = false;
      stop?.();
    };
  }, []);
  return state;
}

function safeWindowAction(action: Parameters<typeof runWindowAction>[0]) {
  try {
    runWindowAction(action);
  } catch (error) {
    console.error(`PurePoint: window ${action} failed`, error);
  }
}

/** Windows 11 minimize, maximize/restore and close. */
export function CaptionButtons() {
  const maximized = useMaximized();
  const { maximizeHover, maximizePressed } = useCaptionButtonState();
  const maximizeClass = [
    'caption-button',
    maximizeHover ? 'is-hover' : '',
    maximizePressed ? 'is-pressed' : '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div className="caption-buttons">
      <button
        type="button"
        className="caption-button"
        aria-label="Minimize"
        title="Minimize"
        data-chrome-interactive
        onClick={() => {
          safeWindowAction('minimize');
        }}
      >
        {''}
      </button>
      <button
        type="button"
        className={maximizeClass}
        aria-label={maximized ? 'Restore' : 'Maximize'}
        data-chrome-maximize
        onClick={() => {
          safeWindowAction('toggleMaximize');
        }}
      >
        {maximized ? '' : ''}
      </button>
      <button
        type="button"
        className="caption-button caption-button-close"
        aria-label="Close"
        title="Close"
        data-chrome-interactive
        onClick={() => {
          safeWindowAction('close');
        }}
      >
        {''}
      </button>
    </div>
  );
}

export interface WindowChromeProps {
  /** The band's leading content: the menu bar and the sidebar toggle. */
  children?: ReactNode;
}

/** The title bar band. */
export function WindowChrome({ children }: WindowChromeProps) {
  const ref = useRef<HTMLElement>(null);
  useReportLayout(ref);
  return (
    <header className="title-bar" ref={ref} data-tauri-drag-region>
      <div className="title-bar-leading">{children}</div>
      <div className="title-bar-drag" data-tauri-drag-region />
      <CaptionButtons />
    </header>
  );
}
