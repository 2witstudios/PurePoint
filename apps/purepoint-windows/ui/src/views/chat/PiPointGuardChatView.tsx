// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PiPointGuardChatView.swift (WIN-9.5):
// the native presentation of the same authoritative Pi session the phone uses.
//
// - The conversation rail lists Pi's conversations (search, Refresh, settings); choosing one
//   resumes it, or, while Point Guard works, opens it read-only (Resume asks to stop first).
// - A new conversation opens on the court (WIN-8.13 `PointGuardCourtView`) until its first message
//   is on its way; plays send their prompt (a saved one is fetched fresh first).
// - The transcript follows the output while the reader is at the bottom (96px slack); scrolling
//   up stops following and shows Latest. Scrolls go through `PiTranscriptScrollCoordinator`, so
//   Markdown and expanding tool output re-anchor without a resize/scroll feedback loop. The last
//   200 rows show; "Show earlier messages" pages back (`PiTranscriptWindow`).
// - The composer: Enter sends, Shift+Enter is a new line; while Point Guard works the draft goes
//   After reply or Steers, and Stop stops the run and its queue (cancelled items come back as
//   Restore receipts). Recovery receipts, the queued count and an extension's suggested text
//   sit above it. Files attach from the + button, a drop or a paste (WIN-9.7 limits).
// - Extension dialogs (select, confirm, input, editor) open as a sheet; closing it cancels.
//
// Differences from Swift: the iPad photo picker and docked court composer are not ported; the
// transcript is DOM scrolling (scroll events stand in for the scroll-phase callbacks).
import {
  type ClipboardEvent,
  type DragEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useStore } from 'zustand';
import {
  base64ToBytes,
  isImage,
  PiTranscriptWindow,
  type PiChatMessage,
  type PiComposerAttachment,
  type PiConversation,
  type PiExtensionDialog,
  type PiToolActivity,
  type PiTranscriptRow,
} from '../../model/piChatDomain.ts';
import {
  resolvePlays,
  type PointGuardPlay,
  type PointGuardPlaybook,
  type PointGuardPrompt,
} from '../../model/pointGuardPlays.ts';
import type { PiChatModel } from '../../state/piChat.ts';
import { attachToComposer } from '../../services/attachmentImport.ts';
import { pointGuardSessionFor, type PointGuardSession } from '../../state/pointGuardSession.ts';
import {
  pointGuardPrompt,
  pointGuardPrompts,
  promptSourceFor,
} from '../../state/appStatePointGuardPlays.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useAppState } from '../shell/appContext.ts';
import { InlineErrorBanner } from '../shell/DaemonErrorBanner.tsx';
import { ChatMarkdown } from './ChatMarkdown.tsx';
import { PiTranscriptScrollCoordinator } from './piTranscriptScrollCoordinator.ts';
import { PointGuardConversationLayout } from './PointGuardConversationLayout.tsx';
import { PointGuardCourtView } from './PointGuardCourtView.tsx';
import { ChatSheet, ConfirmDialog, MenuButton, TypingIndicatorBubble } from './pointGuardParts.tsx';
import { toolStatusLabel } from './ToolCallCardView.tsx';
import { onToggleChatSidebar } from './chatSidebarEvents.ts';
import './pointGuard.css';

/** The model's setters, called outside render (React treats props as immutable there). */
function setChatError(chat: PiChatModel, message: string | null): void {
  chat.error = message;
}

function setPlaybookPrompts(playbook: PointGuardPlaybook, prompts: PointGuardPrompt[]): void {
  playbook.prompts = prompts;
}

/** How close to the bottom still counts as following the output. */
const FOLLOW_SLACK = 96;

export interface PiPointGuardChatViewProps {
  session: PointGuardSession;
  isFocused?: boolean;
  compact?: boolean;
  tabBar?: ReactNode;
  /** The rail's visibility when it is not the shared setting (the compact sidebar keeps its own). */
  sidebarShown?: boolean;
  onToggleSidebar?: () => void;
}

/** Point Guard in the workspace files sidebar: compact, unfocused, with its own rail toggle. */
export function CompactPointGuardChat() {
  const app = useAppState();
  const [shown, setShown] = useState(false);
  useEffect(
    () =>
      onToggleChatSidebar(() => {
        setShown((value) => !value);
      }),
    [],
  );
  return (
    <PiPointGuardChatView
      session={pointGuardSessionFor(app)}
      isFocused={false}
      compact
      sidebarShown={shown}
      onToggleSidebar={() => {
        setShown((value) => !value);
      }}
    />
  );
}

export function PiPointGuardChatView({
  session,
  isFocused = true,
  compact = false,
  tabBar,
  sidebarShown,
  onToggleSidebar,
}: PiPointGuardChatViewProps) {
  const app = useAppState();
  const { chat, playbook } = session;
  const state = useStore(chat.store);
  const sharedShown = useStore(session.showConversations, (s) => s.shown);
  const showSidebar = sidebarShown ?? sharedShown;
  const assignments = useStore(playbook.store, (s) => s.assignments);
  const prompts = useStore(playbook.store, (s) => s.prompts);
  const plays = useMemo(() => resolvePlays(assignments, prompts), [assignments, prompts]);
  const [confirm, setConfirm] = useState<'new' | 'resume' | null>(null);
  const [pendingSession, setPendingSession] = useState<PiConversation | null>(null);
  const [fetchingPlay, setFetchingPlay] = useState(false);
  /** The snapshot revision a court send went out at, until its reply lands. */
  const [awaitingReply, setAwaitingReply] = useState<number | null>(null);
  const [followOutput, setFollowOutput] = useState(true);

  const busy = state.snapshot?.busy === true;
  const sessionId = state.snapshot?.sessionId ?? null;
  const revision = state.snapshot?.revision ?? null;
  const recoverableCount = state.submissions.filter((x) => x.recoverable).length;
  const sending = state.submissions.some((x) => x.status === 'Sending');

  // Lifecycle: the conversation list follows the connection, idle and session changes.
  useEffect(() => {
    if (state.connected) void chat.loadConversations();
  }, [chat, state.connected, busy, sessionId]);
  // A new snapshot or a failed send brings the court back (render-time reset, not an effect).
  const replyKey = String(revision) + ':' + String(recoverableCount);
  const [seenReplyKey, setSeenReplyKey] = useState(replyKey);
  if (seenReplyKey !== replyKey) {
    setSeenReplyKey(replyKey);
    setAwaitingReply(null);
  }

  const showsCourt =
    state.browsing === null &&
    state.transcriptRows.length === 0 &&
    !busy &&
    awaitingReply === null &&
    !sending;

  /** Sends from the court and keeps it down until the reply's snapshot lands. */
  const sendFromCourt = useCallback(
    async (send: () => Promise<void>) => {
      const at = chat.snapshot?.revision ?? null;
      const previous = chat.submissions.at(-1)?.id;
      const sent = send();
      if (chat.submissions.at(-1)?.id !== previous) setAwaitingReply(at ?? 0);
      await sent;
    },
    [chat],
  );

  const refreshPlays = useCallback(async () => {
    const listed = await pointGuardPrompts(promptSourceFor(app));
    if (listed !== null) setPlaybookPrompts(playbook, listed);
  }, [app, playbook]);

  /** Runs a play in the session it was clicked in; a session switched in the meantime drops it. */
  const runPlay = useCallback(
    (play: PointGuardPlay, session: string | null) => {
      if (session === null || chat.snapshot?.sessionId !== session) return;
      setFollowOutput(true);
      if (play.source.kind !== 'saved') {
        void sendFromCourt(() => chat.submitPlay(play.prompt));
        return;
      }
      const name = play.source.name;
      if (fetchingPlay) return;
      setFetchingPlay(true);
      void (async () => {
        try {
          // A saved play sends its prompt as it is now; one deleted or needing input is refused.
          const prompt = await pointGuardPrompt(promptSourceFor(app), name);
          if (prompt !== null && !prompt.needsInput) {
            if (prompt.body === '') {
              setChatError(
                chat,
                `The saved prompt “${name}” is empty, so there’s nothing to send.`,
              );
              return;
            }
            if (chat.snapshot?.sessionId !== session) return;
            if (!chat.canSendPlay) {
              setChatError(chat, 'Point Guard got busy. Run the play again once it’s free.');
              return;
            }
            await sendFromCourt(() => chat.submitPlay(prompt.body));
            return;
          }
          const listed = await pointGuardPrompts(promptSourceFor(app));
          if (listed !== null) setPlaybookPrompts(playbook, listed);
          setChatError(
            chat,
            listed !== null && !listed.some((p) => p.name === name && !p.needsInput)
              ? `The saved prompt “${name}” was deleted or now needs input, so this play is back to its default.`
              : `Couldn’t load the saved prompt “${name}”. Check the connection to PurePoint and try again.`,
          );
        } finally {
          setFetchingPlay(false);
        }
      })();
    },
    [app, chat, fetchingPlay, playbook, sendFromCourt],
  );

  const submitDraft = (mode?: 'after' | 'steer') => {
    setFollowOutput(true);
    const send = () => chat.submit(mode ?? (chat.busy ? 'after' : 'send'));
    if (showsCourt) void sendFromCourt(send);
    else void send();
  };

  const composer = (
    <Composer session={session} compact={compact} isFocused={isFocused} onSubmit={submitDraft} />
  );

  const dialog = state.snapshot?.dialogs[0] ?? null;

  return (
    <div className="pg-chat" data-testid="pg-chat">
      <PointGuardConversationLayout
        expanded={showSidebar}
        onToggle={() => {
          if (onToggleSidebar !== undefined) onToggleSidebar();
          else session.setShowConversations(!showSidebar);
        }}
        compact={compact}
        tabBar={tabBar}
        actions={
          <button
            type="button"
            className="pg-text-icon-button"
            title="Start a new Point Guard conversation"
            aria-label="New Chat"
            data-testid="pg-new-chat"
            disabled={!state.connected || state.changingSession}
            onClick={() => {
              if (chat.busy) setConfirm('new');
              else void chat.changeSession();
            }}
          >
            <Icon name="square.and.pencil" size={13} />
            <span>New Chat</span>
          </button>
        }
        sidebar={
          <ConversationList
            session={session}
            onOpenSettings={() => {
              app.setShowSettings(true);
            }}
            onPick={(conversation) => {
              if (chat.busy) {
                setPendingSession(conversation);
                void chat.browse(conversation);
              } else {
                void chat.changeSession(conversation.id);
              }
            }}
          />
        }
      >
        <div className="pg-chat-content">
          {showsCourt ? (
            <CourtHost
              session={session}
              plays={plays}
              compact={compact}
              canRun={chat.canSendPlay && !fetchingPlay}
              composer={composer}
              onMount={refreshPlays}
              onRun={(play) => {
                runPlay(play, sessionId);
              }}
              onOpenSettings={() => {
                app.setShowSettings(true);
              }}
            />
          ) : (
            <Transcript
              session={session}
              compact={compact}
              followOutput={followOutput}
              setFollowOutput={setFollowOutput}
              onResume={(conversation) => {
                setPendingSession(conversation);
                if (chat.busy) setConfirm('resume');
                else void chat.changeSession(conversation.id);
              }}
            />
          )}
          {state.error !== null && (
            <InlineErrorBanner
              message={state.error}
              onDismiss={() => {
                setChatError(chat, null);
              }}
            />
          )}
          {state.browsing === null && !showsCourt && composer}
        </div>
      </PointGuardConversationLayout>
      {confirm === 'new' && (
        <ConfirmDialog
          title="Stop Point Guard before starting a new conversation?"
          action="Stop and start new"
          onCancel={() => {
            setConfirm(null);
          }}
          onConfirm={() => {
            setConfirm(null);
            void chat.changeSession(null, true);
          }}
        />
      )}
      {confirm === 'resume' && pendingSession !== null && (
        <ConfirmDialog
          title="Stop Point Guard before resuming this conversation?"
          action="Stop and resume"
          onCancel={() => {
            setConfirm(null);
          }}
          onConfirm={() => {
            setConfirm(null);
            void chat.changeSession(pendingSession.id, true);
          }}
        />
      )}
      {dialog !== null && <PiDialogSheet key={dialog.id} session={session} dialog={dialog} />}
    </div>
  );
}

function CourtHost({
  session,
  plays,
  compact,
  canRun,
  composer,
  onMount,
  onRun,
  onOpenSettings,
}: {
  session: PointGuardSession;
  plays: readonly PointGuardPlay[];
  compact: boolean;
  canRun: boolean;
  composer: ReactNode;
  onMount: () => Promise<void>;
  onRun: (play: PointGuardPlay) => void;
  onOpenSettings: () => void;
}) {
  const connected = useStore(session.chat.store, (s) => s.connected);
  const prompts = useStore(session.playbook.store, (s) => s.prompts);
  // Re-lists the global prompts so edits made elsewhere show on the court.
  useEffect(() => {
    void onMount();
  }, [onMount]);
  return (
    <PointGuardCourtView
      plays={plays}
      prompts={prompts ?? []}
      canRun={canRun}
      connected={connected}
      compact={compact}
      composer={composer}
      onRun={onRun}
      onAssign={(slot, prompt) => {
        session.playbook.assign(slot, prompt);
      }}
      onReset={(slot) => {
        session.playbook.reset(slot);
      }}
      onOpenSettings={onOpenSettings}
    />
  );
}

function ConversationList({
  session,
  onPick,
  onOpenSettings,
}: {
  session: PointGuardSession;
  onPick: (conversation: PiConversation) => void;
  onOpenSettings: () => void;
}) {
  const { chat } = session;
  const conversations = useStore(chat.store, (s) => s.conversations);
  const connected = useStore(chat.store, (s) => s.connected);
  const changing = useStore(chat.store, (s) => s.changingSession);
  const current = useStore(chat.store, (s) => s.snapshot?.sessionId ?? null);
  const [search, setSearch] = useState('');
  const needle = search.toLocaleLowerCase();
  const shown = conversations.filter(
    (c) => needle === '' || c.title.toLocaleLowerCase().includes(needle),
  );
  return (
    <div className="pg-conversations" data-testid="pg-conversations">
      <input
        className="pg-text-field pg-conversation-search"
        type="search"
        placeholder="Search conversations"
        aria-label="Search conversations"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
        }}
      />
      <ul className="pg-conversation-list" role="list">
        {shown.map((conversation) => (
          <li key={conversation.id}>
            <button
              type="button"
              className="pg-conversation-row"
              aria-current={conversation.id === current ? 'true' : undefined}
              disabled={!connected || changing}
              onClick={() => {
                onPick(conversation);
              }}
            >
              {conversation.title}
            </button>
          </li>
        ))}
      </ul>
      <div className="pg-conversation-footer">
        <button
          type="button"
          className="pg-text-icon-button"
          disabled={!connected}
          onClick={() => void chat.loadConversations()}
        >
          <Icon name="arrow.clockwise" size={11} />
          <span>Refresh</span>
        </button>
        <span className="pg-spacer" />
        <button
          type="button"
          className="pg-icon-button"
          title="Point Guard connection settings"
          aria-label="Point Guard connection settings"
          onClick={onOpenSettings}
        >
          <Icon name="gearshape" size={12} />
        </button>
      </div>
    </div>
  );
}

function MessageView({ message, compact }: { message: PiChatMessage; compact: boolean }) {
  if (message.role === 'user') {
    return (
      <div className="pg-message-user-row" data-testid="pg-message-user">
        <div
          className="pg-bubble pg-bubble-user"
          data-compact={compact}
          aria-label={`You: ${message.text}`}
        >
          {message.text}
        </div>
        {message.error != null && <div className="pg-message-error">{message.error}</div>}
      </div>
    );
  }
  if (message.role === 'notice') {
    return (
      <div className="pg-message-notice">
        <Icon name="text.alignleft" size={12} />
        <span>{message.text}</span>
      </div>
    );
  }
  return (
    <div className="pg-message-assistant-row" data-testid="pg-message-assistant">
      <div className="pg-bubble pg-bubble-assistant" data-compact={compact}>
        <ChatMarkdown text={message.text} />
        {message.error != null && <div className="pg-message-error">{message.error}</div>}
      </div>
    </div>
  );
}

function PiToolDetail({
  tool,
  onLayoutChange,
}: {
  tool: PiToolActivity;
  onLayoutChange: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const label =
    tool.state === 'finished'
      ? 'Done'
      : tool.state === 'running' || tool.state === 'failed'
        ? toolStatusLabel(false, tool.state === 'running' ? 'running' : 'failed')
        : tool.state.charAt(0).toUpperCase() + tool.state.slice(1);
  return (
    <details
      className="pg-tool-card"
      data-failed={tool.state === 'failed'}
      open={expanded}
      onToggle={(event) => {
        const open = event.currentTarget.open;
        if (open === expanded) return;
        onLayoutChange();
        setExpanded(open);
      }}
    >
      <summary className="pg-tool-summary">
        <span className="pg-tool-name">{tool.name}</span>
        <span className="pg-tool-status">{label}</span>
      </summary>
      <pre className="pg-tool-output">{tool.text === '' ? 'No output' : tool.text}</pre>
    </details>
  );
}

function PiToolActivityView({
  tools,
  onLayoutChange,
}: {
  tools: readonly PiToolActivity[];
  onLayoutChange: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const running = tools.filter((t) => t.state === 'running').length;
  const failed = tools.filter((t) => t.state === 'failed').length;
  return (
    <details
      className="pg-tool-activity"
      data-testid="pg-tool-activity"
      open={expanded}
      onToggle={(event) => {
        const open = event.currentTarget.open;
        if (open === expanded) return;
        onLayoutChange();
        setExpanded(open);
      }}
    >
      <summary
        className="pg-tool-activity-summary"
        aria-description="Expand to inspect tool calls and their output"
      >
        {running > 0 && <span className="pg-tool-running-dot" />}
        <span className="pg-tool-activity-count">
          {tools.length} tool call{tools.length === 1 ? '' : 's'}
        </span>
        {running > 0 ? (
          <span className="pg-tool-activity-state">Working</span>
        ) : failed > 0 ? (
          <span className="pg-tool-activity-failed">{failed} failed</span>
        ) : null}
      </summary>
      <div className="pg-tool-activity-body">
        {tools.map((tool) => (
          <PiToolDetail key={tool.id} tool={tool} onLayoutChange={onLayoutChange} />
        ))}
      </div>
    </details>
  );
}

function Transcript({
  session,
  compact,
  followOutput,
  setFollowOutput,
  onResume,
}: {
  session: PointGuardSession;
  compact: boolean;
  followOutput: boolean;
  setFollowOutput: (follow: boolean) => void;
  onResume: (conversation: PiConversation) => void;
}) {
  const { chat } = session;
  const rows = useStore(chat.store, (s) => s.transcriptRows);
  const browsing = useStore(chat.store, (s) => s.browsing);
  const busy = useStore(chat.store, (s) => s.snapshot?.busy === true);
  const notices = useStore(chat.store, (s) => s.snapshot?.notices ?? null);
  const sessionId = useStore(chat.store, (s) => s.snapshot?.sessionId ?? null);
  const revision = useStore(chat.store, (s) => s.snapshot?.revision ?? null);
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const coordinator = useMemo(() => new PiTranscriptScrollCoordinator(), []);
  const transcriptWindow = useMemo(() => new PiTranscriptWindow(), []);
  const [, setWindowRevision] = useState(0);
  const follow = useRef(followOutput);
  useLayoutEffect(() => {
    follow.current = followOutput;
  }, [followOutput]);
  const userScrolling = useRef(false);

  const shownRows: readonly PiTranscriptRow[] = useMemo(
    () =>
      browsing === null
        ? rows
        : browsing.messages.map((message) => ({
            kind: 'message' as const,
            id: message.id,
            message,
          })),
    [rows, browsing],
  );

  const measure = useCallback(() => {
    const element = scroller.current;
    if (element === null) return null;
    const geometry = {
      contentHeight: element.scrollHeight,
      viewportHeight: element.clientHeight,
      visibleBottom: element.scrollTop + element.clientHeight,
    };
    coordinator.update(geometry);
    return geometry;
  }, [coordinator]);

  const requestScroll = useCallback(() => {
    if (!follow.current || userScrolling.current || chat.store.getState().browsing !== null) return;
    const wanted = chat.snapshot?.sessionId ?? null;
    coordinator.request(
      () =>
        scroller.current !== null &&
        follow.current &&
        !userScrolling.current &&
        chat.store.getState().browsing === null &&
        (chat.snapshot?.sessionId ?? null) === wanted,
      () => {
        const element = scroller.current;
        if (element !== null) element.scrollTop = element.scrollHeight;
      },
    );
  }, [chat, coordinator]);

  // Appearing, a new session or leaving history: follow from the bottom again.
  useLayoutEffect(() => {
    coordinator.reset();
    transcriptWindow.reset();
    setFollowOutput(true);
    follow.current = true;
    measure();
    requestScroll();
  }, [
    coordinator,
    transcriptWindow,
    sessionId,
    browsing?.sessionId,
    measure,
    requestScroll,
    setFollowOutput,
  ]);

  useLayoutEffect(() => {
    coordinator.outputChanged();
    measure();
    requestScroll();
  }, [coordinator, revision, measure, requestScroll]);

  // Markdown and expanded tool output can resize after the snapshot arrives.
  useEffect(() => {
    const element = content.current;
    if (element === null || typeof ResizeObserver === 'undefined') return;
    let last = { content: 0, viewport: 0 };
    const observer = new ResizeObserver(() => {
      const geometry = measure();
      if (geometry === null) return;
      if (geometry.contentHeight !== last.content || geometry.viewportHeight !== last.viewport) {
        last = { content: geometry.contentHeight, viewport: geometry.viewportHeight };
        requestScroll();
      }
    });
    observer.observe(element);
    if (scroller.current !== null) observer.observe(scroller.current);
    return () => {
      observer.disconnect();
    };
  }, [measure, requestScroll]);

  const hidden = transcriptWindow.firstIndex(shownRows);
  const visible = transcriptWindow.visible(shownRows);
  const layoutChanged = () => {
    coordinator.userLayoutChanged();
  };

  return (
    <div className="pg-transcript-host">
      <div
        ref={scroller}
        className="pg-transcript"
        data-testid="pg-transcript"
        onWheel={() => {
          // A reader's own scroll stops following until they return to the bottom.
          coordinator.cancel();
          userScrolling.current = true;
        }}
        onPointerDown={() => {
          userScrolling.current = true;
        }}
        onPointerUp={() => {
          userScrolling.current = false;
        }}
        onKeyDown={(event) => {
          if (['PageUp', 'ArrowUp', 'Home'].includes(event.key)) {
            coordinator.cancel();
            userScrolling.current = true;
          }
        }}
        onScroll={() => {
          const geometry = measure();
          if (geometry === null) return;
          const atBottom = geometry.visibleBottom >= geometry.contentHeight - FOLLOW_SLACK;
          if (userScrolling.current) {
            setFollowOutput(atBottom);
            follow.current = atBottom;
            if (atBottom) userScrolling.current = false;
          } else if (atBottom && !follow.current) {
            setFollowOutput(true);
            follow.current = true;
          }
        }}
        tabIndex={-1}
      >
        <div ref={content} className="pg-transcript-content" data-compact={compact}>
          {browsing !== null && (
            <div className="pg-browsing" data-testid="pg-browsing">
              <div className="pg-browsing-title">Reading: {browsing.title}</div>
              <div className="pg-setup-row">
                <button
                  type="button"
                  className="pg-button"
                  onClick={() => {
                    onResume({ id: browsing.sessionId, title: browsing.title });
                  }}
                >
                  Resume
                </button>
                <button
                  type="button"
                  className="pg-button"
                  onClick={() => {
                    chat.closeBrowsing();
                  }}
                >
                  Back to live
                </button>
              </div>
            </div>
          )}
          {hidden > 0 && (
            <button
              type="button"
              className="pg-link-button pg-show-earlier"
              onClick={() => {
                transcriptWindow.showEarlier(shownRows);
                setWindowRevision((n) => n + 1);
              }}
            >
              Show earlier messages ({hidden} more)
            </button>
          )}
          {visible.map((row) =>
            row.kind === 'message' ? (
              <MessageView key={row.id} message={row.message} compact={compact} />
            ) : (
              <PiToolActivityView key={row.id} tools={row.tools} onLayoutChange={layoutChanged} />
            ),
          )}
          {browsing === null && busy && <TypingIndicatorBubble compact={compact} />}
          {browsing === null &&
            (notices ?? []).map((notice, index) => (
              <div key={index} className="pg-notice">
                {notice}
              </div>
            ))}
        </div>
      </div>
      {/* Fades the transcript into the composer. */}
      <div className="pg-transcript-fade" aria-hidden="true" />
      {!followOutput && (
        <button
          type="button"
          className="pg-button pg-latest"
          onClick={() => {
            userScrolling.current = false;
            setFollowOutput(true);
            follow.current = true;
            coordinator.outputChanged();
            requestScroll();
          }}
        >
          <Icon name="arrow.down" size={12} />
          <span>Latest</span>
        </button>
      )}
    </div>
  );
}

function AttachmentChip({ file, onRemove }: { file: PiComposerAttachment; onRemove: () => void }) {
  const url = useMemo(() => {
    if (!isImage(file) || typeof URL.createObjectURL !== 'function') return null;
    const bytes = base64ToBytes(file.data);
    return URL.createObjectURL(new Blob([bytes as BlobPart], { type: file.mimeType }));
  }, [file]);
  useEffect(
    () => () => {
      if (url !== null) URL.revokeObjectURL(url);
    },
    [url],
  );
  return (
    <div className="pg-attachment-chip" data-testid="pg-attachment">
      {url !== null ? (
        <img src={url} alt="" className="pg-attachment-thumb" />
      ) : (
        <Icon name="doc.text" size={16} />
      )}
      <span className="pg-attachment-name">{file.name}</span>
      <button
        type="button"
        className="pg-icon-button"
        aria-label={`Remove ${file.name}`}
        onClick={onRemove}
      >
        <Icon name="xmark" size={10} />
      </button>
    </div>
  );
}

function Composer({
  session,
  compact,
  isFocused,
  onSubmit,
}: {
  session: PointGuardSession;
  compact: boolean;
  isFocused: boolean;
  onSubmit: (mode?: 'after' | 'steer') => void;
}) {
  const { chat } = session;
  const state = useStore(chat.store);
  const busy = state.snapshot?.busy === true;
  const canSend = chat.canSend;
  const [importing, setImporting] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const queue = state.snapshot?.queue ?? [];
  const recoverable = state.submissions.filter((x) => x.recoverable);

  useEffect(() => {
    if (isFocused) field.current?.focus();
    else field.current?.blur();
  }, [isFocused]);

  // Grow with the draft, up to eight lines.
  useLayoutEffect(() => {
    const element = field.current;
    if (element === null) return;
    element.style.height = 'auto';
    element.style.height = `${String(Math.min(element.scrollHeight, 8 * 20 + 14))}px`;
  }, [state.draft]);

  const attach = async (files: readonly File[]) => {
    if (files.length === 0) return;
    const key = chat.draftKey;
    setImporting(true);
    try {
      await attachToComposer(files, {
        addAttachment: (file) => {
          if (chat.draftKey !== key) {
            setChatError(
              chat,
              'The conversation changed. Attach the file again to your current draft.',
            );
            return;
          }
          chat.addAttachment(file);
        },
        get error() {
          return chat.error;
        },
        set error(value) {
          setChatError(chat, value);
        },
      });
      field.current?.focus();
    } finally {
      setImporting(false);
    }
  };

  const submit = (mode?: 'after' | 'steer') => {
    onSubmit(mode);
    field.current?.focus();
  };

  return (
    <div className="pg-composer-host" data-compact={compact}>
      {recoverable.map((receipt) => (
        <div key={receipt.id} className="pg-receipt" data-testid="pg-receipt">
          <div className="pg-receipt-status">{receipt.status}</div>
          <div className="pg-receipt-text">{receipt.text}</div>
          <div className="pg-setup-row">
            <span className="pg-spacer" />
            <button
              type="button"
              className="pg-button"
              onClick={() => {
                chat.restore(receipt);
              }}
            >
              Restore
            </button>
            <button
              type="button"
              className="pg-button"
              onClick={() => {
                chat.dismissSubmission(receipt.id);
              }}
            >
              Dismiss
            </button>
          </div>
        </div>
      ))}
      {state.submissions.some((x) => x.status === 'Sending') && (
        <div className="pg-composer-note">Sending…</div>
      )}
      {queue.length > 0 && (
        <div className="pg-composer-note" data-testid="pg-queued">
          {queue.length} queued {queue.length === 1 ? 'message' : 'messages'}
        </div>
      )}
      {state.editorOffer !== null && (
        <button
          type="button"
          className="pg-link-button"
          onClick={() => {
            chat.useEditorOffer();
          }}
        >
          Add suggested text to draft
        </button>
      )}
      <div
        className="pg-composer"
        data-testid="pg-composer"
        onDragOver={(event: DragEvent) => {
          if (event.dataTransfer.types.includes('Files')) event.preventDefault();
        }}
        onDrop={(event: DragEvent) => {
          if (event.dataTransfer.files.length === 0) return;
          event.preventDefault();
          void attach([...event.dataTransfer.files]);
        }}
      >
        {state.attachments.length > 0 && (
          <div className="pg-attachments">
            {state.attachments.map((file) => (
              <AttachmentChip
                key={file.id}
                file={file}
                onRemove={() => {
                  chat.removeAttachment(file.id);
                }}
              />
            ))}
          </div>
        )}
        <textarea
          ref={field}
          className="pg-composer-input"
          data-testid="pg-composer-input"
          rows={1}
          placeholder="Message Point Guard…"
          aria-label="Message draft"
          value={state.draft}
          onChange={(event) => {
            chat.setDraft(event.target.value);
          }}
          onPaste={(event: ClipboardEvent) => {
            // An image with no text attaches; anything else pastes as text.
            const files = [...event.clipboardData.files];
            if (files.length > 0 && event.clipboardData.getData('text/plain') === '') {
              event.preventDefault();
              void attach(files);
            }
          }}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return;
            event.preventDefault();
            if (chat.canSend && !importing) submit();
          }}
        />
        <div className="pg-composer-actions">
          <input
            ref={picker}
            type="file"
            multiple
            hidden
            accept="image/*,text/*,.json,.md,.ts,.tsx,.js,.py,.rs,.swift,.go,.java,.c,.cpp,.h,.cs,.sh,.ps1,.yaml,.yml,.toml"
            onChange={(event) => {
              const files = [...(event.target.files ?? [])];
              event.target.value = '';
              void attach(files);
            }}
          />
          <button
            type="button"
            className="pg-composer-plus"
            aria-label="Add attachment"
            title="Attach an image or text file"
            disabled={importing || state.attachments.length >= 4}
            onClick={() => picker.current?.click()}
          >
            {importing ? <span className="pg-spinner" /> : <Icon name="plus" size={18} />}
          </button>
          <span className="pg-spacer" />
          {busy ? (
            <>
              <MenuButton
                label="Send options"
                className={compact ? 'pg-composer-action' : 'pg-send-options'}
                title="Choose when to send your draft"
                disabled={!canSend || importing}
                items={[
                  {
                    label: 'Send after reply',
                    onSelect: () => {
                      submit('after');
                    },
                  },
                  {
                    label: 'Steer current work',
                    onSelect: () => {
                      submit('steer');
                    },
                  },
                ]}
              >
                {compact ? <Icon name="arrow.up" size={14} /> : 'Send options'}
              </MenuButton>
              <button
                type="button"
                className="pg-composer-action"
                data-enabled={state.connected}
                data-testid="pg-stop"
                aria-label="Stop Point Guard"
                title="Stop Point Guard and queued messages; delegated workers keep running"
                disabled={!state.connected}
                onClick={() => void chat.stop()}
              >
                <Icon name="stop.fill" size={14} />
              </button>
            </>
          ) : (
            <button
              type="button"
              className="pg-composer-action"
              data-enabled={canSend && !importing}
              data-testid="pg-send"
              aria-label="Send message"
              title="Send message (Enter)"
              disabled={!canSend || importing}
              onClick={() => {
                submit();
              }}
            >
              <Icon name="arrow.up" size={14} />
            </button>
          )}
        </div>
        {busy && state.attachments.length > 0 && (
          <div className="pg-composer-note">Attachments are ready for your next message.</div>
        )}
      </div>
      {!compact && (
        <div className="pg-composer-hint">Enter to send · Shift+Enter for a new line</div>
      )}
    </div>
  );
}

function PiDialogSheet({
  session,
  dialog,
}: {
  session: PointGuardSession;
  dialog: PiExtensionDialog;
}) {
  const { chat } = session;
  const [text, setText] = useState(dialog.prefill ?? '');
  const cancel = () => void chat.answer(dialog, { cancelled: true });
  return (
    <ChatSheet
      title={dialog.title ?? 'Point Guard needs your input'}
      onDone={cancel}
      width={420}
      testId="pg-dialog"
    >
      <div className="pg-dialog">
        {dialog.message != null && <p className="pg-dialog-message">{dialog.message}</p>}
        {dialog.method === 'select' &&
          (dialog.options ?? []).map((option, index) => (
            <button
              key={`${option}-${String(index)}`}
              type="button"
              className="pg-button"
              onClick={() => {
                const optionId = dialog.optionIds?.[index];
                void chat.answer(dialog, optionId === undefined ? {} : { optionId });
              }}
            >
              {option}
            </button>
          ))}
        {dialog.method !== 'select' && dialog.method !== 'confirm' && (
          <textarea
            className="pg-dialog-text"
            aria-label={dialog.placeholder ?? 'Response'}
            placeholder={dialog.placeholder ?? undefined}
            value={text}
            onChange={(event) => {
              setText(event.target.value);
            }}
          />
        )}
        <div className="pg-setup-row">
          <button type="button" className="pg-button" onClick={cancel}>
            Cancel
          </button>
          <span className="pg-spacer" />
          {dialog.method === 'confirm' ? (
            <>
              <button
                type="button"
                className="pg-button"
                onClick={() => void chat.answer(dialog, { confirmed: false })}
              >
                No
              </button>
              <button
                type="button"
                className="pg-button pg-button-primary"
                onClick={() => void chat.answer(dialog, { confirmed: true })}
              >
                Confirm
              </button>
            </>
          ) : dialog.method !== 'select' ? (
            <button
              type="button"
              className="pg-button pg-button-primary"
              onClick={() => void chat.answer(dialog, { value: text })}
            >
              Send
            </button>
          ) : null}
        </div>
      </div>
    </ChatSheet>
  );
}
