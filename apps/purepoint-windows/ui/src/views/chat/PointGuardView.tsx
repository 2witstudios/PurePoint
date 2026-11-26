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
// `pointGuard` tab): the conversation rail (Pi's conversations; choosing one resumes it) beside
// the court, whose plays send their prompt. The transcript and composer are WIN-9.5's
// `PiPointGuardChatView`, which takes this surface's place.
import { type ReactNode, useEffect } from 'react';
import { useStore } from 'zustand';
import { isPointGuard } from '../../model/workspace.ts';
import { pointGuardSessionFor } from '../../state/pointGuardSession.ts';
import { useAppState } from '../shell/appContext.ts';
import { PaneGridView } from '../panegrid/PaneGridView.tsx';
import { ScreenPlaceholder } from '../detail/placeholders.tsx';
import { PointGuardPaneControls } from './PointGuardPaneControls.tsx';
import {
  CONVERSATION_HEADER_HEIGHT,
  PointGuardConversationLayout,
} from './PointGuardConversationLayout.tsx';
import { PointGuardCourtView } from './PointGuardCourtView.tsx';
import { resolvePlays } from '../../model/pointGuardPlays.ts';
import { pointGuardPrompts, promptSourceFor } from '../../state/appStatePointGuardPlays.ts';
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
  useEffect(() => {
    app.activatePointGuardWorkspace();
  }, [app, projectRoot]);

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
export function PointGuardChatSurfaceView({ tabBar }: { isFocused: boolean; tabBar?: ReactNode }) {
  // `isFocused` moves the composer's focus once the composer lands (WIN-9.5).
  const app = useAppState();
  const session = pointGuardSessionFor(app);
  const { chat, playbook } = session;
  const shown = useStore(session.showConversations, (s) => s.shown);
  const connected = useStore(chat.store, (s) => s.connected);
  const conversations = useStore(chat.store, (s) => s.conversations);
  const current = useStore(chat.store, (s) => s.snapshot?.sessionId ?? null);
  const assignments = useStore(playbook.store, (s) => s.assignments);
  const prompts = useStore(playbook.store, (s) => s.prompts);
  useStore(chat.store, (s) => s.snapshot?.revision);
  useEffect(() => {
    if (connected) void chat.loadConversations();
  }, [chat, connected]);
  useEffect(() => {
    void pointGuardPrompts(promptSourceFor(app)).then((listed) => {
      if (listed !== null) playbook.prompts = listed;
    });
  }, [app, playbook]);
  return (
    <div className="pg-chat" data-testid="pg-chat">
      <PointGuardConversationLayout
        expanded={shown}
        onToggle={() => {
          session.setShowConversations(!shown);
        }}
        tabBar={tabBar}
        actions={null}
        sidebar={
          <ul className="pg-conversation-list" role="list">
            {conversations.map((conversation) => (
              <li key={conversation.id}>
                <button
                  type="button"
                  className="pg-conversation-row"
                  aria-current={conversation.id === current ? 'true' : undefined}
                  disabled={!connected}
                  onClick={() => void chat.changeSession(conversation.id)}
                >
                  {conversation.title}
                </button>
              </li>
            ))}
          </ul>
        }
      >
        <PointGuardCourtView
          plays={resolvePlays(assignments, prompts)}
          prompts={prompts ?? []}
          canRun={chat.canSendPlay}
          connected={connected}
          composer={
            <ScreenPlaceholder
              icon="bubble.left.and.bubble.right"
              title="Composer"
              view="PiPointGuardChatView"
            />
          }
          onRun={(play) => {
            if (play.source.kind === 'builtIn') void chat.submitPlay(play.prompt);
          }}
          onAssign={(slot, prompt) => {
            playbook.assign(slot, prompt);
          }}
          onReset={(slot) => {
            playbook.reset(slot);
          }}
          onOpenSettings={() => {
            app.setShowSettings(true);
          }}
        />
      </PointGuardConversationLayout>
    </div>
  );
}
