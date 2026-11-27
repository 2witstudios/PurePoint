// The update dialog and the About pane's update status (WIN-10.3): the Windows face of
// Services/CheckForUpdatesViewModel.swift. On the Mac, Check for Updates… opens Sparkle's standard
// update window; this dialog shows the same states, texts and buttons (Install Update / Remind Me
// Later, then Install and Relaunch), driven by the backend updater (services/updater.ts).
import { useEffect, useRef } from 'react';
import { useStore } from 'zustand';
import {
  canCheckForUpdates,
  downloadFraction,
  updaterText,
  type UpdaterState,
} from '../../services/updater.ts';
import {
  checkForUpdates,
  downloadUpdate,
  installUpdate,
  type UpdaterCommands,
  type UpdaterStore,
  useUpdaterStore,
} from '../../state/updaterStore.ts';

interface UpdaterViewProps {
  store?: UpdaterStore;
  /** Backend commands; tests pass fakes. */
  commands?: UpdaterCommands;
}

function DialogButtons({
  status,
  store,
  commands,
}: {
  status: UpdaterState;
  store: UpdaterStore;
  commands: UpdaterCommands | undefined;
}) {
  const close = () => {
    store.getState().closeDialog();
  };
  switch (status.state) {
    case 'available':
      return (
        <>
          <button type="button" className="update-dialog-button" onClick={close}>
            Remind Me Later
          </button>
          <button
            type="button"
            className="update-dialog-button update-dialog-default"
            onClick={() => void downloadUpdate(store, commands)}
          >
            Install Update
          </button>
        </>
      );
    case 'readyToRestart':
      return (
        <>
          <button type="button" className="update-dialog-button" onClick={close}>
            Later
          </button>
          <button
            type="button"
            className="update-dialog-button update-dialog-default"
            onClick={() => void installUpdate(store, commands)}
          >
            Install and Relaunch
          </button>
        </>
      );
    case 'checking':
    case 'downloading':
    case 'installing':
      // Sparkle's Cancel stops a download; the plugin's download cannot be cancelled, so the
      // dialog hides and the work continues (the About status keeps showing it).
      return (
        <button type="button" className="update-dialog-button" onClick={close}>
          Hide
        </button>
      );
    default:
      return (
        <button
          type="button"
          className="update-dialog-button update-dialog-default"
          onClick={close}
        >
          OK
        </button>
      );
  }
}

/** Sparkle's update window, as a modal over the app. */
export function UpdateDialog({ store = useUpdaterStore, commands }: UpdaterViewProps) {
  const open = useStore(store, (state) => state.dialogOpen);
  const status = useStore(store, (state) => state.status);
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open) panel.current?.focus();
  }, [open]);
  if (!open) return null;

  const text = updaterText(status);
  const fraction = downloadFraction(status);
  const busy = !canCheckForUpdates(status);
  return (
    <div className="update-dialog-backdrop">
      <div
        className="update-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Software Update"
        aria-busy={busy}
        tabIndex={-1}
        ref={panel}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            store.getState().closeDialog();
          }
        }}
      >
        <h2 className="update-dialog-title">{text.title}</h2>
        {text.detail !== null && <p className="update-dialog-detail">{text.detail}</p>}
        {(status.state === 'checking' ||
          status.state === 'downloading' ||
          status.state === 'installing') && (
          <progress
            className="update-dialog-progress"
            aria-label={text.title}
            {...(fraction === null ? {} : { value: fraction, max: 1 })}
          />
        )}
        {status.state === 'available' && status.notes !== null && (
          <section className="update-dialog-notes" aria-label="Release Notes">
            <h3>Release Notes:</h3>
            <pre>{status.notes}</pre>
          </section>
        )}
        <div className="update-dialog-buttons">
          <DialogButtons status={status} store={store} commands={commands} />
        </div>
      </div>
    </div>
  );
}

/** The About pane's update row: the current status and Check for Updates… (SettingsAboutView). */
export function AboutUpdateStatus({ store = useUpdaterStore, commands }: UpdaterViewProps) {
  const status = useStore(store, (state) => state.status);
  const line = updaterText(status).status;
  return (
    <div className="about-update-status">
      <button
        type="button"
        className="update-dialog-button"
        disabled={!canCheckForUpdates(status)}
        onClick={() => void checkForUpdates(store, commands)}
      >
        Check for Updates…
      </button>
      {line !== null && (
        <span className="about-update-line" role="status">
          {line}
        </span>
      )}
    </div>
  );
}
