// The window's AppState (Swift `@Environment(AppState.self)`), provided by App.tsx.
import { createContext, useContext } from 'react';
import type { AppState } from '../../state/appState.ts';

export const AppStateContext = createContext<AppState | null>(null);

export function useAppState(): AppState {
  const app = useContext(AppStateContext);
  if (app === null) throw new Error('PurePoint: useAppState outside AppStateContext');
  return app;
}
