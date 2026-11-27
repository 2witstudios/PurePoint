// The remote folder picker (WIN-8.14): choose a folder on a machine and open it as a project. A
// machine row's add button opens it. Folders come from the machine's daemon (`fs_list`,
// directories only, through the ssh relay); Open registers the folder there (`init`), and the
// project joins the sidebar under the machine.
//
// The iPad's RemoteFolderPicker+iOS.swift is the model (its tests' platform-neutral part is
// `fsBrowseRequest`); this is a desktop dialog in the shape of the Windows folder picker: a path
// bar with Back, Forward and Up, a list (click selects, double-click or Enter opens a folder), Show
// hidden folders, and Cancel / Open. Open takes the selected folder, else the one listed.

import { useEffect, useId, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import { RemoteFolderBrowser } from '../../state/remoteFolderBrowser.ts';
import type { MachineState } from '../../state/appState.ts';
import { Icon } from '../../theme/Icon.tsx';

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

export function RemoteFolderPicker({
  machine,
  onDismiss,
}: {
  machine: MachineState;
  onDismiss: () => void;
}) {
  const titleId = useId();
  const browser = useMemo(() => RemoteFolderBrowser.forClient(machine.client), [machine]);
  const state = useStore(browser.store);
  const [opening, setOpening] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    void browser.start();
  }, [browser]);

  const chosen = state.selection ?? state.path;
  const open = () => {
    if (chosen === null || opening) return;
    setOpening(true);
    setOpenError(null);
    machine.openProject(chosen).then(onDismiss, (error: unknown) => {
      setOpenError(errorMessage(error));
      setOpening(false);
    });
  };

  return (
    <div
      className="sidebar-confirm-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onDismiss();
      }}
    >
      <div
        className="sidebar-confirm sidebar-sheet remote-folder-picker"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-testid="remote-folder-picker"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onDismiss();
          }
        }}
      >
        <h2 id={titleId} className="sidebar-confirm-title">
          Open a Project on {machine.machine.name}
        </h2>
        <div className="remote-folder-bar">
          <button
            type="button"
            aria-label="Back"
            title="Back"
            disabled={state.backStack.length === 0}
            onClick={() => void browser.goBack()}
          >
            <Icon name="chevron.left" size={12} />
          </button>
          <button
            type="button"
            aria-label="Forward"
            title="Forward"
            disabled={state.forwardStack.length === 0}
            onClick={() => void browser.goForward()}
          >
            <Icon name="chevron.right" size={12} />
          </button>
          <button
            type="button"
            aria-label="Up"
            title="Enclosing folder"
            disabled={state.path === null}
            onClick={() => void browser.goUp()}
          >
            <Icon name="arrow.turn.up.right" size={12} />
          </button>
          <span
            className="remote-folder-path"
            data-testid="remote-folder-path"
            title={state.path ?? ''}
          >
            {state.path ?? 'Loading…'}
          </span>
        </div>
        <div className="remote-folder-list" role="listbox" aria-label="Folders" tabIndex={0}>
          {state.folders.map((folder) => (
            <div
              key={folder.path}
              role="option"
              aria-selected={state.selection === folder.path}
              className="remote-folder-row"
              data-testid={`remote-folder-${folder.name}`}
              tabIndex={-1}
              onClick={() => {
                browser.select(folder.path);
              }}
              onDoubleClick={() => void browser.enter(folder)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void browser.enter(folder);
                }
              }}
            >
              <Icon name="folder.fill" size={13} />
              <span>{folder.name}</span>
              {folder.isSymlink && <span className="remote-folder-link">alias</span>}
            </div>
          ))}
          {!state.loading && state.folders.length === 0 && state.error === null && (
            <div className="remote-folder-empty">No folders</div>
          )}
        </div>
        <label className="remote-folder-hidden">
          <input
            type="checkbox"
            checked={state.showHidden}
            onChange={(event) => void browser.setShowHidden(event.target.checked)}
          />
          Show hidden folders
        </label>
        <div aria-live="polite">
          {(state.error ?? openError) !== null && (
            <p className="sidebar-sheet-note" data-ok="false" role="alert">
              {openError ?? state.error}
            </p>
          )}
        </div>
        <div className="sidebar-confirm-buttons">
          <button type="button" onClick={onDismiss}>
            Cancel
          </button>
          <button
            type="button"
            data-default="true"
            disabled={chosen === null || opening}
            onClick={open}
          >
            {opening ? 'Opening…' : 'Open'}
          </button>
        </div>
      </div>
    </div>
  );
}
