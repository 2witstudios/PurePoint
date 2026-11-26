// Port of `TabCloseConfirmation` in apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneTabBar.swift
// (WIN-7.4): asks before closing tabs would lose something: unsaved edits in a file tab, or, when
// a whole pane goes, more than one running agent (one is what closing a tab does anyway).
//
// Differences from Swift: the Mac asks with a modal `NSAlert` and closes once it returns. A web page
// has no modal run loop, so the request waits in a store and the dialog (mounted once by the shell)
// runs the close from its button, as the iPad's `UIAlertController` does.
import { useEffect, useRef } from 'react';
import { create } from 'zustand';
import { isAgentStatusAlive } from '../../model/agentStatus.ts';
import { contentAgentId, type Surface } from '../../model/workspace.ts';
import type { AppState } from '../../state/appState.ts';
import { Icon } from '../../theme/Icon.tsx';

/** What the confirmation reads: live agent status and the file tabs' unsaved edits. */
export interface CloseConfirmationSource {
  isAgentAlive(agentId: string): boolean;
  /** The unsaved file's name in a file tab, or null when it has nothing unsaved. */
  unsavedFileName(workspaceId: string, surfaceId: number): string | null;
}

export function closeConfirmationSource(app: AppState): CloseConfirmationSource {
  return {
    isAgentAlive: (agentId) => {
      const agent = app.agent(agentId);
      return agent !== null && isAgentStatusAlive(agent.status);
    },
    unsavedFileName: (workspaceId, surfaceId) => {
      const session = app.registry?.fileTabs.existingSession(workspaceId, surfaceId) ?? null;
      if (session === null || !session.hasUnsavedChanges) return null;
      return session.editor.currentFile?.name ?? 'a file';
    },
  };
}

/** What closing `tabs` would lose, or nothing when it needs no confirmation (Swift `reasons`). */
export function closeReasons(
  tabs: readonly Surface[],
  workspaceId: string,
  source: CloseConfirmationSource,
): string[] {
  const running = tabs
    .map((tab) => contentAgentId(tab.content))
    .filter((agentId): agentId is string => agentId !== null && source.isAgentAlive(agentId));
  const unsaved = tabs.flatMap((tab) => {
    if (tab.content.kind !== 'file') return [];
    const name = source.unsavedFileName(workspaceId, tab.id);
    return name === null ? [] : [name];
  });
  if (running.length <= 1 && unsaved.length === 0) return [];
  const reasons: string[] = [];
  if (running.length > 1) reasons.push(`${String(running.length)} running agents will be stopped.`);
  if (unsaved.length > 0) reasons.push(`Unsaved changes to ${unsaved.join(', ')} will be lost.`);
  return reasons;
}

export interface CloseConfirmationRequest {
  readonly title: string;
  readonly message: string;
  readonly confirmLabel: string;
  readonly onConfirm: () => void;
}

interface CloseConfirmationState {
  readonly request: CloseConfirmationRequest | null;
  show(request: CloseConfirmationRequest): void;
  dismiss(): void;
}

export const useCloseConfirmation = create<CloseConfirmationState>()((set) => ({
  request: null,
  show: (request) => {
    set({ request });
  },
  dismiss: () => {
    set({ request: null });
  },
}));

/**
 * Run `close` at once when closing `tabs` loses nothing, otherwise once the user agrees in the
 * confirmation dialog (Swift `TabCloseConfirmation.close`).
 */
export function confirmTabClose(
  tabs: readonly Surface[],
  workspaceId: string,
  source: CloseConfirmationSource,
  close: () => void,
): void {
  const reasons = closeReasons(tabs, workspaceId, source);
  if (reasons.length === 0) {
    close();
    return;
  }
  const pane = tabs.length > 1;
  useCloseConfirmation.getState().show({
    title: pane ? 'Close this pane?' : 'Close this tab?',
    message: reasons.join(' '),
    confirmLabel: pane ? 'Close Pane' : 'Close Tab',
    onConfirm: close,
  });
}

/** The warning alert (Swift's `NSAlert`, `.warning`): the close button, then Cancel. */
export function TabCloseConfirmationDialog() {
  const request = useCloseConfirmation((state) => state.request);
  const dismiss = () => {
    useCloseConfirmation.getState().dismiss();
  };
  const confirmButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (request !== null) confirmButton.current?.focus();
  }, [request]);

  if (request === null) return null;
  return (
    <div
      className="close-confirmation-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) dismiss();
      }}
    >
      <div
        className="close-confirmation"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="close-confirmation-title"
        aria-describedby="close-confirmation-message"
        data-testid="close-confirmation"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            dismiss();
          }
        }}
      >
        <Icon name="exclamationmark.triangle.fill" size={32} className="close-confirmation-icon" />
        <h2 id="close-confirmation-title">{request.title}</h2>
        <p id="close-confirmation-message">{request.message}</p>
        <div className="close-confirmation-buttons">
          <button
            type="button"
            ref={confirmButton}
            className="close-confirmation-confirm"
            onClick={() => {
              dismiss();
              request.onConfirm();
            }}
          >
            {request.confirmLabel}
          </button>
          <button type="button" onClick={dismiss}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
