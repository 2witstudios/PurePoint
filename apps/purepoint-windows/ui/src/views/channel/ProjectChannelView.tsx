// Port of apps/purepoint-macos/purepoint-macos/Views/Channel/ProjectChannelView.swift (WIN-8.10).
//
// A project's shared channel: the header (members, search, Mark as read / Refresh), the unread
// banner, search, errors, the timeline (older pages, day dividers, the "New messages" boundary,
// grouped rows, references, reactions, reply counts and the inline thread) and the composer.
// Messages count as read once at least half of them has been on screen while the window is
// active; opening an older timeline alone marks nothing read.
//
// Differences from Swift: `onScrollVisibilityChange(threshold: 0.5)` is an IntersectionObserver
// at 0.5, `NSApp.isActive` is `document.hasFocus()`, `scrollPosition(id:)` is the browser's scroll
// anchoring, and the members popover, menus and sheets are in-page popups.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import {
  authorInitials,
  channelDay,
  channelTime,
  humanAuthors as findHumanAuthors,
  isGrouped,
  referenceTitle,
  replyCountTitle,
  type ChannelAuthor,
  type ChannelMessage,
} from '../../model/channelModel.ts';
import { agentDisplayName } from '../../model/workspaceModel.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ChatInlineMarkdown } from '../chat/ChatMarkdown.tsx';
import { ChannelComposer } from './ChannelComposer.tsx';
import { ChannelHoverRow } from './ChannelHoverRow.tsx';
import { ChannelReferenceView, type ChannelReferenceSelection } from './ChannelReferenceView.tsx';
import './channel.css';

export interface ProjectChannelViewProps {
  project: ProjectState;
  /** The worktree screen's side panel: a shorter header without the subtitle. */
  compact?: boolean;
  /** Swift `NSApp.isActive && scenePhase == .active`. */
  isAppActive?: () => boolean;
  /** Date formatting locale (the system's by default). */
  locale?: string;
  copyText?: (text: string) => void;
}

const defaultIsActive = () => document.hasFocus() && document.visibilityState === 'visible';
const defaultCopy = (text: string) => {
  void navigator.clipboard.writeText(text).catch(() => undefined);
};

function Avatar({ author }: { author: ChannelAuthor }) {
  return (
    <span className={`channel-avatar${author.kind === 'agent' ? ' agent' : ''}`} aria-hidden="true">
      {authorInitials(author.name)}
    </span>
  );
}

function Popup({
  onClose,
  className,
  label,
  children,
}: {
  onClose: () => void;
  className: string;
  label: string;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (!(event.target instanceof Node) || !ref.current?.parentElement?.contains(event.target)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', close);
    return () => {
      document.removeEventListener('mousedown', close);
    };
  }, [onClose]);
  return (
    <div
      ref={ref}
      className={className}
      role="menu"
      aria-label={label}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      {children}
    </div>
  );
}

export function ProjectChannelView({
  project,
  compact = false,
  isAppActive = defaultIsActive,
  locale,
  copyText = defaultCopy,
}: ProjectChannelViewProps) {
  const channel = project.channel;
  const state = useStore(channel.store);
  const rootAgents = useStore(project.store, (fields) => fields.rootAgents);
  const worktrees = useStore(project.store, (fields) => fields.worktrees);
  const agents = useMemo(
    () => [...worktrees.flatMap((worktree) => worktree.agents), ...rootAgents],
    [rootAgents, worktrees],
  );
  const [searchVisible, setSearchVisible] = useState(false);
  const [searchText, setSearchText] = useState(() => channel.query);
  const [editing, setEditing] = useState<ChannelMessage | null>(null);
  const [editText, setEditText] = useState('');
  const [unreadBoundary, setUnreadBoundary] = useState<number | null>(() => channel.readSequence);
  const [atBottom, setAtBottom] = useState(true);
  const [showingMembers, setShowingMembers] = useState(false);
  const [showingMenu, setShowingMenu] = useState(false);
  const [mentionMenu, setMentionMenu] = useState<'draft' | 'reply' | null>(null);
  const [referenceSelection, setReferenceSelection] = useState<ChannelReferenceSelection | null>(
    null,
  );
  const scroller = useRef<HTMLDivElement>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const searchField = useRef<HTMLInputElement>(null);
  const visibleIds = useRef(new Set<string>());
  const observer = useRef<IntersectionObserver | null>(null);
  const observed = useRef(new Map<Element, string>());

  const humanAuthors = useMemo(() => findHumanAuthors(state.messages), [state.messages]);
  const mentions = useMemo(
    () => [...agents.map(agentDisplayName), ...humanAuthors.map((author) => author.name)],
    [agents, humanAuthors],
  );
  const topLevel = useMemo(
    () => state.messages.filter((message) => message.parent_id === null),
    [state.messages],
  );

  // Swift `.onAppear` / `.onDisappear`.
  useEffect(() => {
    const visible = visibleIds.current;
    channel.start();
    return () => {
      visible.clear();
      channel.stop();
    };
  }, [channel]);

  // Latest values for the observer callbacks.
  const latest = useRef({ editing, referenceSelection, isAppActive });
  useEffect(() => {
    latest.current = { editing, referenceSelection, isAppActive };
  });

  const markVisibleMessages = useCallback(() => {
    const { editing: edit, referenceSelection: reference, isAppActive: active } = latest.current;
    const s = channel.store.getState();
    if (!active() || edit !== null || reference !== null || s.query !== '') return;
    for (const message of [...s.messages, ...s.threadMessages]) {
      if (!visibleIds.current.has(message.id)) continue;
      if (message.parent_id === null || s.threadId === message.parent_id) {
        channel.markVisible(message);
      }
    }
  }, [channel]);

  // Swift `didBecomeActiveNotification` / `scenePhase == .active`.
  useEffect(() => {
    const onActive = () => {
      markVisibleMessages();
    };
    window.addEventListener('focus', onActive);
    document.addEventListener('visibilitychange', onActive);
    return () => {
      window.removeEventListener('focus', onActive);
      document.removeEventListener('visibilitychange', onActive);
    };
  }, [markVisibleMessages]);

  // One observer for every row: visible means at least half on screen.
  useEffect(() => {
    if (typeof IntersectionObserver === 'undefined') return;
    const watching = observed.current;
    const io = new IntersectionObserver(
      (entries) => {
        let appeared = false;
        for (const entry of entries) {
          const id = watching.get(entry.target);
          if (id === undefined) continue;
          if (entry.isIntersecting && entry.intersectionRatio >= 0.5) {
            visibleIds.current.add(id);
            appeared = true;
          } else {
            visibleIds.current.delete(id);
          }
        }
        if (appeared) markVisibleMessages();
      },
      { root: scroller.current, threshold: [0, 0.5] },
    );
    observer.current = io;
    for (const element of watching.keys()) io.observe(element);
    const sentinel = bottom.current;
    const bottomObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) setAtBottom(entry.isIntersecting);
      },
      { root: scroller.current, threshold: 0.5 },
    );
    if (sentinel !== null) bottomObserver.observe(sentinel);
    return () => {
      io.disconnect();
      bottomObserver.disconnect();
      observer.current = null;
    };
  }, [markVisibleMessages]);

  const observe = useCallback(
    (id: string) => (element: HTMLDivElement | null) => {
      if (element === null) return;
      for (const [known, knownId] of observed.current) {
        if (knownId === id && known !== element) {
          observer.current?.unobserve(known);
          observed.current.delete(known);
        }
      }
      if (!observed.current.has(element)) {
        observed.current.set(element, id);
        observer.current?.observe(element);
      }
    },
    [],
  );

  // Rows that left the DOM stop counting.
  useEffect(() => {
    for (const [element, id] of observed.current) {
      if (!element.isConnected) {
        observer.current?.unobserve(element);
        observed.current.delete(element);
        visibleIds.current.delete(id);
      }
    }
  });

  const scrollToBottom = useCallback((smooth: boolean) => {
    const view = scroller.current;
    if (view === null) return;
    if (typeof view.scrollTo === 'function') {
      view.scrollTo({ top: view.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
    } else {
      view.scrollTop = view.scrollHeight;
    }
  }, []);

  // Swift `.onChange(of: channel.messages.last?.id)`: follow new messages while at the bottom.
  const lastId = state.messages.at(-1)?.id;
  useEffect(() => {
    if (lastId !== undefined && atBottom && channel.query === '') scrollToBottom(false);
    // Only a new last message scrolls; reaching the bottom does not.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastId, channel, scrollToBottom]);

  const revealUnread = () => {
    void channel.revealFirstUnread().then((target) => {
      if (target === null) return;
      requestAnimationFrame(() => {
        const element = scroller.current?.querySelector(
          `[data-message-id="${CSS.escape(target)}"]`,
        );
        element?.scrollIntoView({ block: 'start', behavior: 'smooth' });
      });
    });
  };

  const markRead = () => {
    channel.markRead();
    setUnreadBoundary(null);
  };

  const applySearch = (text: string) => {
    channel.setQuery(text);
    void channel.search();
  };

  const clearSearch = (hide: boolean) => {
    setSearchText('');
    if (hide) setSearchVisible(false);
    applySearch('');
  };

  const day = (value: string) => channelDay(value, locale);
  const time = (value: string) => channelTime(value, locale);

  const messageRow = (message: ChannelMessage, grouped = false, observeRow = true) => (
    <div
      key={message.id}
      data-message-id={message.id}
      ref={observeRow ? observe(message.id) : undefined}
    >
      <ChannelHoverRow
        canReply={message.parent_id === null}
        canEdit={message.author.id === state.selfAuthorId}
        label={`${message.author.name}: ${message.text}`}
        reply={() => {
          void channel.openThread(message.id);
        }}
        react={() => {
          void channel.react(message);
        }}
        copy={() => {
          copyText(message.text);
        }}
        unread={() => {
          channel.markUnread(message);
          setUnreadBoundary(message.sequence - 1);
        }}
        edit={() => {
          setEditText(message.text);
          setEditing(message);
        }}
      >
        <div className={`channel-message${grouped ? ' grouped' : ''}`}>
          {grouped ? (
            <span className="channel-gutter-time">{time(message.created_at)}</span>
          ) : (
            <Avatar author={message.author} />
          )}
          <div className="channel-message-body">
            {!grouped && (
              <div className="channel-message-header">
                <span className="channel-author">{message.author.name}</span>
                {message.author.kind === 'agent' && (
                  <span className="channel-agent-badge">
                    {message.author.agent_type ?? 'agent'}
                  </span>
                )}
                <span className="channel-time">{time(message.created_at)}</span>
                {message.author.branch !== null && (
                  <span className="channel-branch">{message.author.branch}</span>
                )}
              </div>
            )}
            <ChatInlineMarkdown text={message.text} className="channel-message-text" />
            {message.edited_at !== null && <span className="channel-edited">edited</span>}
            {message.references.length > 0 && (
              <div className="channel-references">
                {message.references.map((reference, index) => (
                  <button
                    key={index}
                    type="button"
                    className="channel-reference"
                    title="Open reference changes"
                    onClick={() => {
                      setReferenceSelection({ message, reference });
                    }}
                  >
                    <Icon
                      name={
                        reference.kind === 'pr'
                          ? 'arrow.triangle.pull'
                          : 'point.3.connected.trianglepath.dotted'
                      }
                      size={11}
                    />
                    <span>{referenceTitle(reference)}</span>
                  </button>
                ))}
              </div>
            )}
            <div className="channel-reactions">
              {message.reactions.map((reaction) => (
                <button
                  key={reaction.emoji}
                  type="button"
                  className={`channel-reaction${reaction.author_ids.includes(state.selfAuthorId) ? ' mine' : ''}`}
                  onClick={() => {
                    void channel.react(message);
                  }}
                >
                  {`${reaction.emoji} ${String(reaction.author_ids.length)}`}
                </button>
              ))}
              {(state.replyCounts[message.id] ?? 0) > 0 && (
                <button
                  type="button"
                  className="channel-reply-count"
                  onClick={() => {
                    void channel.openThread(message.id);
                  }}
                >
                  {replyCountTitle(state.replyCounts[message.id] ?? 0)}
                </button>
              )}
            </div>
          </div>
          <span className="channel-message-trailing" />
        </div>
      </ChannelHoverRow>
    </div>
  );

  const composer = (reply: boolean) => {
    const text = reply ? state.replyDraft : state.draft;
    const setText = (value: string) => {
      if (reply) channel.setReplyDraft(value);
      else channel.setDraft(value);
    };
    const send = () => {
      void channel.send(reply);
    };
    const menuKey = reply ? 'reply' : 'draft';
    return (
      <div
        className="channel-composer"
        data-testid={reply ? 'channel-reply-composer' : 'channel-composer'}
      >
        <ChannelComposer
          text={text}
          onTextChange={setText}
          placeholder={reply ? 'Reply' : 'Message channel'}
          prompt={reply ? 'Reply to thread…' : `Message #${project.projectName}`}
          mentions={mentions}
          onSubmit={send}
        />
        <div className="channel-composer-bar">
          <div className="channel-menu-anchor">
            <button
              type="button"
              className="channel-tool channel-mention-button"
              aria-label="Mention someone"
              title="Mention someone"
              aria-haspopup="menu"
              aria-expanded={mentionMenu === menuKey}
              onClick={() => {
                setMentionMenu((open) => (open === menuKey ? null : menuKey));
              }}
            >
              @
            </button>
            {mentionMenu === menuKey && (
              <Popup
                className="channel-menu channel-menu-up"
                label="Mention someone"
                onClose={() => {
                  setMentionMenu(null);
                }}
              >
                {agents.map((agent) => (
                  <button
                    key={agent.id}
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setMentionMenu(null);
                      setText(`${text}@${agentDisplayName(agent)} `);
                    }}
                  >
                    {`@${agentDisplayName(agent)}`}
                  </button>
                ))}
              </Popup>
            )}
          </div>
          <button
            type="button"
            className="channel-tool"
            aria-label="Insert inline code"
            title="Insert inline code"
            onClick={() => {
              setText(`${text}\`code\``);
            }}
          >
            <Icon name="chevron.left.forwardslash.chevron.right" size={12} />
          </button>
          <span className="channel-hint">Enter to send · Shift Enter for a new line</span>
          <span className="channel-spacer" />
          <button
            type="button"
            className="channel-send"
            aria-label="Send message"
            title="Send message"
            disabled={state.isSending || text.trim() === ''}
            onClick={send}
          >
            {state.isSending ? (
              <span className="channel-spinner small" aria-label="Sending" />
            ) : (
              <Icon name="arrow.up" size={12} />
            )}
          </button>
        </div>
      </div>
    );
  };

  const thread = () => (
    <div className="channel-thread" data-testid="channel-thread">
      <div className="channel-thread-header">
        <span className="channel-thread-title">Thread</span>
        <span className="channel-spacer" />
        <button
          type="button"
          className="channel-tool"
          aria-label="Close thread"
          title="Close thread"
          onClick={() => {
            channel.closeThread();
          }}
        >
          <Icon name="xmark" size={11} />
        </button>
      </div>
      <div className="channel-thread-messages">
        {state.threadHasMore && (
          <button
            type="button"
            className="channel-load-older"
            onClick={() => {
              void channel.loadOlderReplies();
            }}
          >
            Load older replies
          </button>
        )}
        {state.threadMessages.map((message) => messageRow(message))}
      </div>
      {composer(true)}
    </div>
  );

  const error = state.mutationError ?? state.readError;

  return (
    <div className={`channel-view${compact ? ' compact' : ''}`} data-view="ProjectChannelView">
      <div className="channel-header">
        <span className="channel-hash">#</span>
        <div className="channel-heading">
          <span className="channel-title">{project.projectName}</span>
          {!compact && (
            <span className="channel-subtitle">A shared place to keep each other up to date</span>
          )}
        </div>
        <span className="channel-spacer" />
        <div className="channel-menu-anchor">
          <button
            type="button"
            className="channel-tool"
            aria-label="Channel members"
            title="Channel members"
            aria-expanded={showingMembers}
            onClick={() => {
              setShowingMembers((shown) => !shown);
            }}
          >
            <Icon name="person.2" size={14} />
          </button>
          {showingMembers && (
            <Popup
              className="channel-popover"
              label="Project members"
              onClose={() => {
                setShowingMembers(false);
              }}
            >
              <span className="channel-popover-title">Project members</span>
              {agents.map((agent) => (
                <span key={agent.id} className="channel-member">
                  <Icon name="cpu" size={12} />
                  <span>{agentDisplayName(agent)}</span>
                  <span className="channel-spacer" />
                  <span className="channel-member-type">{agent.agentType}</span>
                </span>
              ))}
              {humanAuthors.map((author) => (
                <span key={author.id} className="channel-member">
                  <Icon name="person" size={12} />
                  <span>{author.name}</span>
                </span>
              ))}
            </Popup>
          )}
        </div>
        <button
          type="button"
          className="channel-tool"
          aria-label="Search messages"
          title="Search messages (Ctrl+F)"
          onClick={() => {
            setSearchVisible(true);
            requestAnimationFrame(() => searchField.current?.focus());
          }}
        >
          <Icon name="magnifyingglass" size={14} />
        </button>
        <div className="channel-menu-anchor">
          <button
            type="button"
            className="channel-tool"
            aria-label="Channel options"
            aria-haspopup="menu"
            aria-expanded={showingMenu}
            onClick={() => {
              setShowingMenu((shown) => !shown);
            }}
          >
            <Icon name="ellipsis" size={14} />
          </button>
          {showingMenu && (
            <Popup
              className="channel-menu"
              label="Channel options"
              onClose={() => {
                setShowingMenu(false);
              }}
            >
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setShowingMenu(false);
                  markRead();
                }}
              >
                Mark as read
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setShowingMenu(false);
                  void channel.refresh();
                }}
              >
                Refresh
              </button>
            </Popup>
          )}
        </div>
      </div>
      {state.unreadCount > 0 && state.query === '' && (
        <div className="channel-unread-banner">
          <button type="button" onClick={revealUnread}>
            {`${String(state.unreadCount)} unread messages ↓`}
          </button>
          <span className="channel-spacer" />
          <button type="button" aria-label="Mark as read" title="Mark as read" onClick={markRead}>
            <Icon name="xmark" size={11} />
          </button>
        </div>
      )}
      {(searchVisible || state.query !== '') && (
        <div className="channel-search">
          <input
            ref={searchField}
            type="search"
            className="channel-search-field"
            placeholder="Search messages, people, branches…"
            aria-label="Search messages"
            value={searchText}
            onChange={(event) => {
              setSearchText(event.target.value);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                applySearch(searchText);
              } else if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                clearSearch(true);
              }
            }}
          />
          {state.query !== '' && (
            <button
              type="button"
              className="channel-link-button"
              onClick={() => {
                clearSearch(false);
              }}
            >
              Clear search
            </button>
          )}
        </div>
      )}
      {error !== null && (
        <div className="channel-error" role="alert">
          <Icon name="exclamationmark.circle" size={13} />
          <span className="channel-error-text">{error}</span>
          <span className="channel-spacer" />
          <button
            type="button"
            className="channel-button"
            onClick={() => {
              void channel.refresh();
            }}
          >
            Refresh
          </button>
          {state.mutationError !== null && (
            <button
              type="button"
              className="channel-button"
              onClick={() => {
                channel.dismissMutationError();
              }}
            >
              Dismiss
            </button>
          )}
        </div>
      )}
      <div
        className="channel-timeline"
        ref={scroller}
        tabIndex={-1}
        onKeyDown={(event) => {
          if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
            event.preventDefault();
            setSearchVisible(true);
            requestAnimationFrame(() => searchField.current?.focus());
          }
        }}
      >
        {state.isLoading && state.messages.length === 0 ? (
          <div className="channel-loading">
            <span className="channel-spinner" aria-label="Loading" />
          </div>
        ) : (
          <div className="channel-timeline-content" data-testid="channel-timeline">
            {state.hasMore && (
              <button
                type="button"
                className="channel-load-older"
                onClick={() => {
                  void channel.loadOlder();
                }}
              >
                Load older messages
              </button>
            )}
            {topLevel.length === 0 && (
              <div className="channel-empty">
                <Icon
                  name="bubble.left.and.bubble.right"
                  size={30}
                  className="channel-empty-icon"
                />
                <span className="channel-empty-title">
                  {state.query === '' ? 'The conversation starts here' : 'No messages found'}
                </span>
                {state.query === '' && (
                  <span className="channel-empty-detail">
                    Share progress, ask a question, or leave a note for the team.
                  </span>
                )}
              </div>
            )}
            {topLevel.map((message, index) => {
              const previous = index > 0 ? topLevel[index - 1] : undefined;
              const boundary =
                unreadBoundary !== null &&
                message.sequence > unreadBoundary &&
                (previous === undefined || previous.sequence <= unreadBoundary);
              const newDay =
                previous === undefined || day(previous.created_at) !== day(message.created_at);
              return (
                <div key={message.id} className="channel-entry">
                  {boundary && (
                    <div className="channel-divider channel-new-messages">
                      <span className="channel-rule" />
                      <span>New messages</span>
                      <span className="channel-rule" />
                    </div>
                  )}
                  {newDay && (
                    <div className="channel-divider channel-day">
                      <span className="channel-rule" />
                      <span>{day(message.created_at)}</span>
                      <span className="channel-rule" />
                    </div>
                  )}
                  {messageRow(message, isGrouped(message, previous, locale))}
                  {state.query !== '' &&
                    state.messages
                      .filter((reply) => reply.parent_id === message.id)
                      .map((reply) => (
                        <div key={reply.id} className="channel-search-reply">
                          {messageRow(reply, false, false)}
                        </div>
                      ))}
                  {state.threadId === message.id && thread()}
                </div>
              );
            })}
          </div>
        )}
        <div className="channel-bottom" ref={bottom} />
      </div>
      {!atBottom && state.query === '' && (
        <div className="channel-jump">
          <button
            type="button"
            className="channel-jump-button"
            onClick={() => {
              scrollToBottom(true);
            }}
          >
            <Icon name="arrow.down" size={11} />
            <span>
              {state.unreadCount > 0
                ? `${String(state.unreadCount)} new · Jump to latest`
                : 'Jump to latest'}
            </span>
          </button>
        </div>
      )}
      {composer(false)}
      {referenceSelection !== null && (
        <ChannelReferenceView
          project={project}
          selection={referenceSelection}
          onDismiss={() => {
            setReferenceSelection(null);
          }}
        />
      )}
      {editing !== null && (
        <div className="channel-sheet-backdrop">
          <div
            className="channel-sheet"
            role="dialog"
            aria-modal="true"
            aria-label="Edit message"
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.stopPropagation();
                setEditing(null);
              }
            }}
          >
            <h2 className="channel-sheet-title">Edit message</h2>
            <textarea
              className="channel-edit-text"
              aria-label="Message text"
              autoFocus
              value={editText}
              onChange={(event) => {
                setEditText(event.target.value);
              }}
            />
            <div className="channel-sheet-footer">
              <span className="channel-spacer" />
              <button
                type="button"
                className="channel-button"
                onClick={() => {
                  setEditing(null);
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                className="channel-button prominent"
                disabled={editText.trim() === ''}
                onClick={() => {
                  const message = editing;
                  void channel.edit(message, editText).then(() => {
                    if (channel.error === null) setEditing(null);
                  });
                }}
              >
                Save
              </button>
            </div>
            {error !== null && <span className="channel-sheet-error">{error}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
