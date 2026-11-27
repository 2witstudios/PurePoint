// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PointGuardView.swift (WIN-8.13): the
// Point Guard screen (the sidebar's dashboard). Point Guard is a root workspace, using the same
// tabs, splits and palette as agents: once Pi reports its working folder, the screen is that
// folder's Point Guard workspace (`ws-point-guard:<root>`), whose Point Guard tabs run the chat
// and whose other tabs are terminals and agents in that folder. Before Pi reports a folder, or
// when its folder is too broad to be a project (the home folder, a drive root), the chat runs on
// its own, with the pane controls where the tab strip would be. A layout that could not be
// restored offers Retry.
//
// `PointGuardChatSurfaceView` is one Point Guard tab's chat (the pane grid renders it for a
// `pointGuard` tab). The conversation rail's visibility is one shared setting; Toggle Chat
// Sidebar flips it only while a chat is on screen.
import { type ReactNode, useEffect } from 'react';
import { useStore } from 'zustand';
import { isPointGuard } from '../../model/workspace.ts';
import { pointGuardSessionFor } from '../../state/pointGuardSession.ts';
import { useAppState } from '../shell/appContext.ts';
import { PaneGridView } from '../panegrid/PaneGridView.tsx';
import { paneActiveTab } from '../../model/workspace.ts';
import { ScreenPlaceholder } from '../detail/placeholders.tsx';
import { PiPointGuardChatView } from './PiPointGuardChatView.tsx';
import { PointGuardPaneControls } from './PointGuardPaneControls.tsx';
import { onToggleChatSidebar } from './chatSidebarEvents.ts';
import { CONVERSATION_HEADER_HEIGHT } from './PointGuardConversationLayout.tsx';
import './pointGuard.css';

export function PointGuardView() {
  const app = useAppState();
  const registry = app.registry;
  const session = pointGuardSessionFor(app);
  const pointGuardRoot = useStore(app.store, (state) => state.pointGuardRoot);
  // Re-render as projects and workspaces change.
  useStore(app.store, (state) => state.projectRevision);
  const projectRoot =
    pointGuardRoot === null || app.isTooBroadProjectRoot(pointGuardRoot) ? null : pointGuardRoot;
  const layoutError = useStore(registry?.store ?? app.store, () =>
    projectRoot === null ? null : (registry?.layoutErrorsByProject[projectRoot] ?? null),
  );
  const workspace = useStore(registry?.store ?? app.store, () =>
    projectRoot === null
      ? null
      : (registry?.workspacesByProject[projectRoot]?.find(isPointGuard) ?? null),
  );
  const hasVisibleChat =
    projectRoot === null ||
    (workspace !== null &&
      Object.values(workspace.panes).some(
        (pane) => pane !== undefined && paneActiveTab(pane)?.content.kind === 'pointGuard',
      ));

  useEffect(() => {
    app.activatePointGuardWorkspace();
  }, [app, projectRoot]);

  // The workspace owns this shared setting once, even when a terminal or file sibling has focus.
  // Background chat tabs don't handle it.
  useEffect(
    () =>
      onToggleChatSidebar(() => {
        if (hasVisibleChat)
          session.setShowConversations(!session.showConversations.getState().shown);
      }),
    [hasVisibleChat, session],
  );

  let body: ReactNode;
  if (projectRoot !== null && layoutError !== null) {
    body = (
      <div className="pg-layout-error" data-testid="pg-layout-error">
        <ScreenPlaceholder icon="exclamationmark.triangle" title="Could not restore layout" />
        <p>{layoutError}</p>
        <button
          type="button"
          className="pg-button"
          onClick={() => {
            app.activatePointGuardWorkspace();
          }}
        >
          Retry
        </button>
      </div>
    );
  } else if (workspace !== null && registry !== null) {
    body = <PaneGridView key={workspace.id} workspaceId={workspace.id} registry={registry} />;
  } else if (projectRoot === null) {
    // Without a workspace there is no pane tab strip, so the controls take its place beside the
    // conversation rail's header.
    body = (
      <PointGuardChatSurfaceView
        isFocused
        tabBar={
          <div className="pg-controls-strip" style={{ height: CONVERSATION_HEADER_HEIGHT }}>
            <PointGuardPaneControls session={session} />
          </div>
        }
      />
    );
  } else {
    body = null;
  }
  return (
    <div className="pg-view" data-testid="point-guard-view">
      {body}
    </div>
  );
}

/** One Point Guard tab's chat. */
export function PointGuardChatSurfaceView({
  isFocused,
  tabBar,
}: {
  isFocused: boolean;
  tabBar?: ReactNode;
}) {
  const app = useAppState();
  return (
    <PiPointGuardChatView
      session={pointGuardSessionFor(app)}
      isFocused={isFocused}
      tabBar={tabBar}
    />
  );
}
