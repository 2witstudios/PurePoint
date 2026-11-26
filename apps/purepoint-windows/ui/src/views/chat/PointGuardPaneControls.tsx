// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PointGuardPaneControls.swift (WIN-8.13):
// the connection controls that live in a Point Guard tab's strip — a status dot (typing dots
// while Point Guard works) and an options menu with Connect phone, Setup and Connect/Disconnect.
// History controls stay in the conversation rail.
import { useStore } from 'zustand';
import { Icon } from '../../theme/Icon.tsx';
import type { PointGuardSession } from '../../state/pointGuardSession.ts';
import { useAppState } from '../shell/appContext.ts';
import { MenuButton, TypingDots } from './pointGuardParts.tsx';

export function PointGuardPaneControls({ session }: { session: PointGuardSession }) {
  const { chat, service } = session;
  const connected = useStore(chat.store, (state) => state.connected);
  const status = useStore(chat.store, (state) => state.connectionStatus);
  const busy = useStore(chat.store, (state) => state.snapshot?.busy === true);
  // Setup and Connect phone open Settings › Point Guard until WIN-9.5's setup sheet.
  const app = useAppState();

  return (
    <div className="pg-pane-controls" data-testid="pg-pane-controls">
      <span
        className="pg-connection-indicator"
        title={status}
        role="status"
        aria-label={busy ? 'Point Guard is working' : status}
      >
        {busy ? (
          <TypingDots diameter={3} spacing={1.5} />
        ) : (
          <span className="pg-connection-dot" data-connected={connected} />
        )}
      </span>
      <MenuButton
        label="Point Guard options"
        items={[
          {
            label: 'Connect phone',
            onSelect: () => {
              app.setShowSettings(true);
              void service.connectPhone();
            },
          },
          {
            label: 'Setup',
            onSelect: () => {
              app.setShowSettings(true);
            },
          },
          {
            label: connected ? 'Disconnect' : 'Connect',
            separated: true,
            onSelect: () => {
              if (connected) chat.disconnect();
              else chat.connect();
            },
          },
        ]}
      >
        <Icon name="ellipsis" size={14} />
      </MenuButton>
    </div>
  );
}
