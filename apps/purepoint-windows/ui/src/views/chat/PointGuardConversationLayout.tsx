// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PointGuardConversationLayout.swift
// (WIN-8.13): a pane-local conversation rail beside the chat.
//
// In split mode the rail runs the full height of the pane; its header shares the row with the
// pane's tab strip (`tabBar`), so the two read as one band. The rail is 36px collapsed, and
// 180–280px (dragged; at most 45% of the pane) open. Compact mode (the workspace files sidebar)
// swaps the conversation list in for the chat instead.
//
// Differences from Swift: the title bar is a band across the whole window on Windows, so the rail
// has no traffic-light clearance to leave; the shortcut in the toggle's tooltip is Ctrl+Shift+S.
import { type ReactNode, useRef, useState } from 'react';
import { Icon } from '../../theme/Icon.tsx';
import { useElementSize } from './pointGuardParts.tsx';

/** Matches `PANE_TAB_BAR_HEIGHT`. */
export const CONVERSATION_HEADER_HEIGHT = 30;
const COLLAPSED_WIDTH = 36;
const TOGGLE_HELP = 'Toggle conversations (Ctrl+Shift+S)';

export interface PointGuardConversationLayoutProps {
  expanded: boolean;
  onToggle: () => void;
  compact?: boolean;
  /** The pane's tab strip, drawn to the right of the rail's header. */
  tabBar?: ReactNode;
  actions: ReactNode;
  sidebar: ReactNode;
  children: ReactNode;
}

/** The rail's width for a pane `available` px wide. */
export function railWidth(expanded: boolean, preferred: number, available: number): number {
  const width = Math.max(0, available);
  return expanded
    ? Math.min(preferred, Math.max(COLLAPSED_WIDTH, width * 0.45), width)
    : Math.min(COLLAPSED_WIDTH, width);
}

export function PointGuardConversationLayout(props: PointGuardConversationLayoutProps) {
  return props.compact === true ? <CompactLayout {...props} /> : <SplitLayout {...props} />;
}

/** History replaces chat in the workspace inspector, leaving the composer the full width. */
function CompactLayout({
  expanded,
  onToggle,
  actions,
  sidebar,
  children,
}: PointGuardConversationLayoutProps) {
  return (
    <div className="pg-conversation-compact" data-testid="pg-conversation-layout">
      <div className="pg-conversation-compact-bar">
        <button
          type="button"
          className="pg-icon-button"
          title={TOGGLE_HELP}
          aria-label={expanded ? 'Back to chat' : 'Show conversations'}
          onClick={onToggle}
        >
          <Icon name={expanded ? 'bubble.left.and.bubble.right' : 'sidebar.left'} size={14} />
        </button>
        <span className="pg-spacer" />
        {actions}
      </div>
      <div className="pg-conversation-compact-body">{expanded ? sidebar : children}</div>
    </div>
  );
}

function SplitLayout({
  expanded,
  onToggle,
  tabBar,
  actions,
  sidebar,
  children,
}: PointGuardConversationLayoutProps) {
  const [ref, size] = useElementSize<HTMLDivElement>();
  const [preferred, setPreferred] = useState(210);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);
  const width = railWidth(expanded, preferred, size.width);
  const dividerWidth = Math.min(expanded ? 4 : 1, Math.max(0, size.width - width));
  const headerPadding = Math.min(expanded ? 10 : 6, Math.max(0, (width - 24) / 2));

  return (
    <div className="pg-conversation-split" ref={ref} data-testid="pg-conversation-layout">
      <div
        className="pg-conversation-rail"
        style={{ width }}
        data-expanded={expanded}
        data-testid="pg-conversation-rail"
      >
        <div
          className="pg-conversation-rail-header"
          style={{
            height: CONVERSATION_HEADER_HEIGHT,
            paddingLeft: headerPadding,
            paddingRight: headerPadding,
          }}
        >
          {expanded && width >= 160 && <span className="pg-conversation-title">CONVERSATIONS</span>}
          {expanded && <span className="pg-spacer" />}
          <button
            type="button"
            className="pg-icon-button"
            title={TOGGLE_HELP}
            aria-label={expanded ? 'Hide conversations' : 'Show conversations'}
            aria-expanded={expanded}
            onClick={onToggle}
          >
            <Icon name="sidebar.left" size={14} />
          </button>
        </div>
        {expanded && <div className="pg-conversation-rail-body">{sidebar}</div>}
      </div>
      {/* The drag handle is wider than the hairline it draws. */}
      <div
        className="pg-conversation-divider"
        style={{ width: dividerWidth }}
        role="separator"
        aria-orientation="vertical"
        onPointerDown={(event) => {
          if (!expanded) return;
          drag.current = { startX: event.clientX, startWidth: width };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const start = drag.current;
          if (start === null) return;
          setPreferred(
            Math.min(280, Math.max(180, start.startWidth + event.clientX - start.startX)),
          );
        }}
        onPointerUp={() => {
          drag.current = null;
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
      />
      <div className="pg-conversation-main">
        {tabBar}
        <div className="pg-conversation-actions">
          <span className="pg-spacer" />
          {actions}
        </div>
        <div className="pg-conversation-content">{children}</div>
      </div>
    </div>
  );
}
