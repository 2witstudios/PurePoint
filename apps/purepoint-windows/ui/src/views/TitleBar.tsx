import { getCurrentWindow } from '@tauri-apps/api/window';

function runWindowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  const window = getCurrentWindow();
  window[action]().catch((error: unknown) => {
    console.error(`PurePoint: window ${action} failed`, error);
  });
}

/**
 * Placeholder chrome for the undecorated window (DEC-10): a drag strip plus
 * minimize, maximize and close. WIN-7.1 replaces it with the real title bar.
 */
export function TitleBar() {
  return (
    <header className="title-bar" data-tauri-drag-region>
      <span className="title-bar-title" data-tauri-drag-region>
        PurePoint
      </span>
      <div className="title-bar-controls">
        <button
          type="button"
          aria-label="Minimize"
          onClick={() => {
            runWindowAction('minimize');
          }}
        >
          &#xE921;
        </button>
        <button
          type="button"
          aria-label="Maximize"
          onClick={() => {
            runWindowAction('toggleMaximize');
          }}
        >
          &#xE922;
        </button>
        <button
          type="button"
          aria-label="Close"
          className="title-bar-close"
          onClick={() => {
            runWindowAction('close');
          }}
        >
          &#xE8BB;
        </button>
      </div>
    </header>
  );
}
