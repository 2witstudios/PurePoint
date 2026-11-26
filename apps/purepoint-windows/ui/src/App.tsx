import { AppStateContext } from './views/shell/appContext.ts';
import { ContentView } from './views/shell/ContentView.tsx';
import type { AppState } from './state/appState.ts';

/** The main window (WIN-7.1): the shell, given the app's state. */
export function App({ app }: { app: AppState }) {
  return (
    <AppStateContext.Provider value={app}>
      <ContentView />
    </AppStateContext.Provider>
  );
}
