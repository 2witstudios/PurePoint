// The workspace that was on screen, remembered across launches (WIN-6.3).
//
// The Mac app keeps this in UserDefaults (`PurePointActiveWorkspaceId`, AppState.swift's
// selection persistence), not in `.pu/workspaces.json`: the document's `activeWorkspaceId` is
// for the CLI only, and selection is per app, not per project. Here it lives in the app's own
// store under `purepoint.workspaceSelection.v1`. Storage can be missing or throw (private
// windows, blocked site data); selection is a convenience, so failures read as "nothing saved".

export const WORKSPACE_SELECTION_KEY = 'purepoint.workspaceSelection.v1';

/** The part of `Storage` this needs, so tests and non-browser callers can pass their own. */
export type SelectionStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

interface StoredSelection {
  readonly activeWorkspaceId: string;
}

function defaultStorage(): SelectionStorage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

/** Remember the selected workspace; `null` forgets it (UserDefaults `set(nil)` removes the key). */
export function saveWorkspaceSelection(
  activeWorkspaceId: string | null,
  storage: SelectionStorage | null = defaultStorage(),
): void {
  try {
    if (activeWorkspaceId === null) {
      storage?.removeItem(WORKSPACE_SELECTION_KEY);
    } else {
      const stored: StoredSelection = { activeWorkspaceId };
      storage?.setItem(WORKSPACE_SELECTION_KEY, JSON.stringify(stored));
    }
  } catch {
    // Selection is not worth failing over.
  }
}

/** The workspace to select once it exists, or `null` when none was saved. */
export function loadWorkspaceSelection(
  storage: SelectionStorage | null = defaultStorage(),
): string | null {
  try {
    const raw = storage?.getItem(WORKSPACE_SELECTION_KEY) ?? null;
    if (raw === null) return null;
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== 'object' || parsed === null) return null;
    const id = (parsed as Partial<Record<keyof StoredSelection, unknown>>).activeWorkspaceId;
    return typeof id === 'string' ? id : null;
  } catch {
    return null;
  }
}
