// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/WorkspaceFilesLayout.swift (WIN-8.2).
//
// A workspace's content beside its files sidebar (WorkspaceFilesSidebar), which the user can
// resize (240–640 px, 360 ideal) and hide. While hidden, the content carries the button that
// reopens it (Swift puts it in the top-right pane's tab bar; the pane grid is WIN-7.4).
// One WorkspaceFilesState per root and machine, so changing roots never mixes their files.
import { type ReactNode, useMemo, useRef, useState } from 'react';
import { WorkspaceFilesState } from '../../state/workspaceFilesState.ts';
import { useSidebarVisibilityStore } from '../../state/sidebarVisibilityStore.ts';
import { WorkspaceFilesSidebar, WorkspaceSidebarToggle } from './WorkspaceFilesSidebar.tsx';
import './detail.css';

export const WORKSPACE_SIDEBAR_MIN_WIDTH = 240;
export const WORKSPACE_SIDEBAR_IDEAL_WIDTH = 360;
export const WORKSPACE_SIDEBAR_MAX_WIDTH = 640;

const clampWidth = (width: number) =>
  Math.min(WORKSPACE_SIDEBAR_MAX_WIDTH, Math.max(WORKSPACE_SIDEBAR_MIN_WIDTH, width));

export interface WorkspaceFilesLayoutProps {
  rootPath: string;
  machineId: string | null;
  children: ReactNode;
  /** Tests pass a state over a scripted git service. */
  createState?: (rootPath: string, machineId: string | null) => WorkspaceFilesState;
}

const defaultCreate = (rootPath: string, machineId: string | null) =>
  new WorkspaceFilesState(rootPath, { machineId });

export function WorkspaceFilesLayout({
  rootPath,
  machineId,
  children,
  createState = defaultCreate,
}: WorkspaceFilesLayoutProps) {
  const showSidebar = useSidebarVisibilityStore((state) => state.workspaceSidebarShown);
  // Keyed on machine and path (Swift `.onChange(of: identity)`).
  const filesState = useMemo(
    () => createState(rootPath, machineId),
    [createState, rootPath, machineId],
  );
  const [width, setWidth] = useState(WORKSPACE_SIDEBAR_IDEAL_WIDTH);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);

  return (
    <div className="workspace-files-layout" data-view="WorkspaceFilesLayout">
      <div className="workspace-files-content">
        {children}
        {!showSidebar && (
          <div className="workspace-files-reopen">
            <WorkspaceSidebarToggle />
          </div>
        )}
      </div>
      {showSidebar && (
        <>
          <div
            className="workspace-files-resizer"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize workspace sidebar"
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              event.preventDefault();
              drag.current = { startX: event.clientX, startWidth: width };
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={(event) => {
              const start = drag.current;
              if (start === null) return;
              // The sidebar is on the right: dragging left widens it.
              setWidth(clampWidth(start.startWidth - (event.clientX - start.startX)));
            }}
            onPointerUp={() => {
              drag.current = null;
            }}
          />
          <aside className="workspace-files-sidebar" style={{ width }}>
            <WorkspaceFilesSidebar key={filesState.identity} state={filesState} />
          </aside>
        </>
      )}
    </div>
  );
}
