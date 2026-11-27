// Port of apps/purepoint-macos/purepoint-macos/Views/Terminal/TerminalContainerView.swift
// (WIN-7.3): hosts the cached terminal for one agent. Mounting shows the agent's terminal (made
// on first use) and focuses it, as switching agents does on the Mac; unmounting parks it hidden,
// alive, until the cache evicts it. Usable on its own; the pane grid (WIN-7.4) places one per
// pane, with `autoFocus` following the grid's focused pane.
import '@xterm/xterm/css/xterm.css';
import { useEffect, useLayoutEffect, useRef } from 'react';
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
  const autoFocusRef = useRef(autoFocus);
  const viewRef = useRef<{ focus(): void } | null>(null);
  useLayoutEffect(() => {
    onFocusRef.current = onFocus;
    autoFocusRef.current = autoFocus;
  });

  useLayoutEffect(() => {
    const container = ref.current;
    if (container === null) return;
    const target = host ?? terminalHost();
    const view = target.cache.acquire(agentId);
    view.mount(container);
    viewRef.current = view;
    let frame: number | null = null;
    if (autoFocusRef.current) {
      // Deferred like `makeTerminalFirstResponder`, once layout has settled.
      frame = requestAnimationFrame(() => {
        frame = null;
        view.focus();
      });
    }
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      viewRef.current = null;
      if (view.element.parentElement === container) view.park(target.parking);
      target.cache.release(agentId);
    };
  }, [agentId, host]);

  // A shown terminal whose pane takes focus (the grid's focus moves) takes keyboard focus too,
  // without remounting.
  const mounted = useRef(false);
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    if (!autoFocus) return;
    const frame = requestAnimationFrame(() => {
      viewRef.current?.focus();
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [autoFocus]);

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
