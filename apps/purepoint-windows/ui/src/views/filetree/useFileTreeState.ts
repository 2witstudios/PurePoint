// A view's own FileTreeState (WIN-8.4): one per root and host, listed and watched while `active`
// (Swift `.onChange(of: showFileTree)`: `load` when shown, `stopWatching` when hidden or gone).
import { useEffect, useMemo } from 'react';
import type { FileHost } from '../../services/fileHost';
import { FileTreeState } from '../../state/fileTreeState';

export function useFileTreeState(rootPath: string, host: FileHost, active: boolean): FileTreeState {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a new tree per root and host
  const state = useMemo(() => new FileTreeState(), [rootPath, host.identity]);
  useEffect(() => {
    if (!active) return;
    void state.load(rootPath, host);
    return () => {
      state.stopWatching();
    };
  }, [state, active, rootPath, host]);
  return state;
}
