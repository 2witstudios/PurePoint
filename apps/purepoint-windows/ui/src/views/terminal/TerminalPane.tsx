// Port of apps/purepoint-macos/purepoint-macos/Views/Terminal/TerminalContainerView.swift
// (WIN-7.3): hosts the cached terminal for one agent. Mounting shows the agent's terminal (made
// on first use) and focuses it, as switching agents does on the Mac; unmounting parks it hidden,
// alive, until the cache evicts it. Usable on its own; the pane grid (WIN-7.4) places several.
import '@xterm/xterm/css/xterm.css';
import { useLayoutEffect, useRef } from 'react';
import { terminalHost, type TerminalHost } from './terminals.ts';
import './terminal.css';

export interface TerminalPaneProps {
  agentId: string;
  /** Take keyboard focus when shown (the Mac focuses a terminal it switches to). */
  autoFocus?: boolean;
  /** A pane was clicked (`onFocus`: the grid's focused pane). */
  onFocus?: () => void;
  host?: TerminalHost;
}

export function TerminalPane({ agentId, autoFocus = true, onFocus, host }: TerminalPaneProps) {
  const ref = useRef<HTMLDivElement>(null);
  const onFocusRef = useRef(onFocus);
  useLayoutEffect(() => {
    onFocusRef.current = onFocus;
  });

  useLayoutEffect(() => {
    const container = ref.current;
    if (container === null) return;
    const target = host ?? terminalHost();
    const view = target.cache.acquire(agentId);
    view.mount(container);
    let frame: number | null = null;
    if (autoFocus) {
      // Deferred like `makeTerminalFirstResponder`, once layout has settled.
      frame = requestAnimationFrame(() => {
        frame = null;
        view.focus();
      });
    }
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      if (view.element.parentElement === container) view.park(target.parking);
      target.cache.release(agentId);
    };
  }, [agentId, autoFocus, host]);

  return (
    <div
      ref={ref}
      className="terminal-pane"
      data-testid="terminal-pane"
      data-agent-id={agentId}
      onMouseDown={() => {
        onFocusRef.current?.();
      }}
    />
  );
}
