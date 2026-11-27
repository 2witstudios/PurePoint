// The Swift `.toggleChatSidebar` notification (WIN-9.5): the Toggle Chat Sidebar command (Ctrl+Shift+S)
// posts it, and the Point Guard surfaces on screen toggle their conversation rail.
type Listener = () => void;

const listeners = new Set<Listener>();

/** Post the toggle to every listening Point Guard surface. */
export function postToggleChatSidebar(): void {
  for (const listener of [...listeners]) listener();
}

/** Listen for the toggle; returns the stop. */
export function onToggleChatSidebar(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
