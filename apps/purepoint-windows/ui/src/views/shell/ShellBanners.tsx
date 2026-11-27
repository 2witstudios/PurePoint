// The banners across the top of the window (ContentView's `DaemonErrorBanner` overlay): the
// daemon error AppState holds, and the installer's failure (`installer-state`, WIN-10.1).
import { useStore } from 'zustand';
import { create } from 'zustand';
import {
  installerBannerMessage,
  installerRun,
  type InstallerState,
} from '../../services/installer.ts';
import { useAppState } from './appContext.ts';
import { DaemonErrorBanner } from './DaemonErrorBanner.tsx';

export interface InstallerBannerState {
  readonly installer: InstallerState;
  /** The user closed the banner for the current failure; a new state shows it again. */
  readonly dismissed: boolean;
  setInstaller(state: InstallerState): void;
  dismiss(): void;
}

export function createInstallerBannerStore() {
  return create<InstallerBannerState>()((set) => ({
    installer: { state: 'idle' },
    dismissed: false,
    setInstaller: (installer) => {
      set({ installer, dismissed: false });
    },
    dismiss: () => {
      set({ dismissed: true });
    },
  }));
}

export const useInstallerBannerStore = createInstallerBannerStore();

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

/** Runs the installer again; its outcome replaces the banner. */
export function retryInstall(store = useInstallerBannerStore): void {
  store.getState().setInstaller({ state: 'running' });
  installerRun()
    .then((state) => {
      store.getState().setInstaller(state);
    })
    .catch((error: unknown) => {
      store.getState().setInstaller({ state: 'failed', message: errorText(error), report: null });
    });
}

export function ShellBanners({ installerStore = useInstallerBannerStore }) {
  const app = useAppState();
  const daemonError = useStore(app.store, (state) => state.daemonError);
  const installer = useStore(installerStore, (state) => state.installer);
  const installerDismissed = useStore(installerStore, (state) => state.dismissed);
  const installerMessage = installerDismissed ? null : installerBannerMessage(installer);

  if (daemonError === null && installerMessage === null) return null;
  return (
    <div className="shell-banners">
      {installerMessage !== null && (
        <DaemonErrorBanner
          message={`Couldn't install the command-line tools: ${installerMessage}`}
          action={{
            label: 'Retry',
            onClick: () => {
              retryInstall(installerStore);
            },
          }}
          onDismiss={() => {
            installerStore.getState().dismiss();
          }}
        />
      )}
      {daemonError !== null && (
        <DaemonErrorBanner
          message={daemonError}
          onDismiss={() => {
            app.setDaemonError(null);
          }}
        />
      )}
    </div>
  );
}
