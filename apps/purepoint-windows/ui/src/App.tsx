import { useEffect, useSyncExternalStore } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { AppState } from './state/appState.ts';
import type { ProjectState } from './state/projectState.ts';
import { TitleBar } from './views/TitleBar.tsx';

// Interim project list so the end-to-end smoke test (WIN-5.7) can see the daemon's projects.
// WIN-7.1's sidebar replaces it.
const appState = isTauri() ? new AppState() : null;
const noProjects: readonly ProjectState[] = [];

function subscribe(onChange: () => void): () => void {
  return appState?.store.subscribe(onChange) ?? (() => undefined);
}

function useProjects(): readonly ProjectState[] {
  return useSyncExternalStore(subscribe, () => appState?.store.getState().projects ?? noProjects);
}

function projectName(root: string): string {
  return root.split(/[\\/]/).filter(Boolean).pop() ?? root;
}

/** Minimal shell; the real window content arrives with WIN-7.1. */
export function App() {
  const projects = useProjects();
  useEffect(() => {
    if (appState === null) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    // Listen before following the daemon's projects, so the first list is not missed.
    void appState.connect().then((unlisten) => {
      if (cancelled) {
        unlisten();
        return;
      }
      stop = unlisten;
      appState.restoreProjects();
    });
    return () => {
      cancelled = true;
      stop?.();
      appState.stopFollowingLocalProjects();
    };
  }, []);

  return (
    <div className="app">
      <TitleBar />
      <main className="app-content">
        <h1>PurePoint</h1>
        <ul data-testid="project-list" aria-label="Projects">
          {projects.map((project) => (
            <li key={project.projectRoot} title={project.projectRoot}>
              {projectName(project.projectRoot)}
            </li>
          ))}
        </ul>
      </main>
    </div>
  );
}
