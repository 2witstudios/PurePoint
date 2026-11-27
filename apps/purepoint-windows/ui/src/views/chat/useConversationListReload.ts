// When the Point Guard conversation rail reloads its list, as PiPointGuardChatView.swift's
// `lifecycle` does: on mount and on connecting (while connected), when Point Guard goes idle
// (busy turns false, not when a run starts), and when the session changes.
import { useEffect, useRef } from 'react';

export function useConversationListReload(
  load: () => void,
  connected: boolean,
  busy: boolean,
  sessionId: string | null,
): void {
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });
  useEffect(() => {
    if (connected) loadRef.current();
  }, [connected, sessionId]);
  const wasBusy = useRef(busy);
  useEffect(() => {
    const was = wasBusy.current;
    wasBusy.current = busy;
    if (was && !busy && connected) loadRef.current();
  }, [busy, connected]);
}
