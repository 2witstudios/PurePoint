// Port of apps/purepoint-macos/purepoint-macos/Views/Channel/ChannelHoverRow.swift (WIN-8.10).
//
// Actions surface on pointer hover and keyboard focus without crowding the timeline: React 👍,
// Reply in thread (top-level messages), and a menu with Copy text, Mark unread and, for the
// sender's own messages, Edit message.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Icon } from '../../theme/Icon.tsx';

export interface ChannelHoverRowProps {
  canReply: boolean;
  canEdit: boolean;
  reply: () => void;
  react: () => void;
  copy: () => void;
  unread: () => void;
  edit: () => void;
  /** Accessible name of the row (the message's author and text). */
  label: string;
  className?: string;
  children: ReactNode;
}

export function ChannelHoverRow({
  canReply,
  canEdit,
  reply,
  react,
  copy,
  unread,
  edit,
  label,
  className,
  children,
}: ChannelHoverRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (!(event.target instanceof Node) || !menu.current?.contains(event.target)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', close);
    return () => {
      document.removeEventListener('mousedown', close);
    };
  }, [menuOpen]);

  const choose = (action: () => void) => () => {
    setMenuOpen(false);
    action();
  };

  return (
    <div
      className={`channel-row${menuOpen ? ' channel-row-active' : ''}${className ? ` ${className}` : ''}`}
      tabIndex={0}
      role="article"
      aria-label={label}
    >
      {children}
      <div className="channel-row-actions" role="toolbar" aria-label="Message actions">
        <button
          type="button"
          className="channel-action"
          aria-label="React 👍"
          title="React 👍"
          onClick={react}
        >
          <Icon name="hand.thumbsup" size={12} />
        </button>
        {canReply && (
          <button
            type="button"
            className="channel-action"
            aria-label="Reply in thread"
            title="Reply in thread"
            onClick={reply}
          >
            <Icon name="bubble.left" size={12} />
          </button>
        )}
        <div className="channel-menu-anchor" ref={menu}>
          <button
            type="button"
            className="channel-action"
            aria-label="More actions"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => {
              setMenuOpen((open) => !open);
            }}
          >
            <Icon name="ellipsis" size={12} />
          </button>
          {menuOpen && (
            <div
              className="channel-menu"
              role="menu"
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  event.stopPropagation();
                  setMenuOpen(false);
                }
              }}
            >
              <button type="button" role="menuitem" onClick={choose(copy)}>
                Copy text
              </button>
              <button type="button" role="menuitem" onClick={choose(unread)}>
                Mark unread
              </button>
              {canEdit && (
                <button type="button" role="menuitem" onClick={choose(edit)}>
                  Edit message
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
