// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsHotkeysView.swift (WIN-8.11).
//
// Clicking a row records: the next chord (by `KeyboardEvent.code`, see hotkeyRecorder.ts) becomes
// the action's binding, a chord another action has offers Override (the two swap) or Cancel, a
// chord the terminal keeps is refused, and plain Esc cancels. The listener runs in the capture
// phase and swallows the keys, so nothing else (menu accelerators, the Settings Esc) sees them;
// ContentView's accelerator handler also stands down while `recordingAction` is set.
import { useEffect, useState } from 'react';
import {
  CATEGORY_DISPLAY_NAMES,
  HOTKEY_ACTIONS,
  HOTKEY_CATEGORIES,
  type HotkeyAction,
  type KeyBinding,
  actionDisplayName,
  actionsFor,
  displayKeys,
} from '../../model/keyBinding.ts';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import { Icon } from '../../theme/Icon.tsx';
import { overrideBindings, recordKey } from './hotkeyRecorder.ts';
import { SettingsGroup } from './SettingsGroup.tsx';

type KeyBindingStore = typeof useKeyBindingStore;

interface Pending {
  readonly binding: KeyBinding;
  readonly conflict: HotkeyAction;
}

function HotkeyRow({ action, store }: { action: HotkeyAction; store: KeyBindingStore }) {
  const binding = store((state) => state.bindings[action]);
  const customized = store((state) => state.overrides[action] !== undefined);
  const recording = store((state) => state.recordingAction === action);
  const [pending, setPending] = useState<Pending | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const stop = () => {
    setPending(null);
    setNotice(null);
    if (store.getState().recordingAction === action) store.getState().setRecordingAction(null);
  };

  const start = () => {
    setPending(null);
    setNotice(null);
    store.getState().setRecordingAction(action);
  };

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat) return;
      const state = store.getState();
      const result = recordKey(event, action, state);
      switch (result.kind) {
        case 'cancel':
          setPending(null);
          setNotice(null);
          state.setRecordingAction(null);
          break;
        case 'ignored':
          break;
        case 'reserved':
          setPending(null);
          setNotice(result.message);
          break;
        case 'conflict':
          setNotice(null);
          setPending({ binding: result.binding, conflict: result.action });
          break;
        case 'set':
          state.setBinding(result.binding, action);
          setPending(null);
          setNotice(null);
          state.setRecordingAction(null);
          break;
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
    };
  }, [recording, action, store]);

  // Leaving the pane (or closing Settings) ends a recording, as `.onDisappear` does.
  useEffect(
    () => () => {
      if (store.getState().recordingAction === action) store.getState().setRecordingAction(null);
    },
    [action, store],
  );

  const name = actionDisplayName(action);
  return (
    <div
      className="settings-hotkey-row"
      data-testid={`hotkey-row-${action}`}
      data-recording={recording}
      role="button"
      tabIndex={0}
      aria-label={`${name} shortcut`}
      onClick={() => {
        if (!recording) start();
      }}
      onKeyDown={(event) => {
        if (!recording && (event.key === 'Enter' || event.key === ' ')) {
          event.preventDefault();
          start();
        }
      }}
    >
      <span>{name}</span>
      <span className="settings-spacer" />
      {recording ? (
        pending !== null ? (
          <span className="settings-hotkey-conflict">
            <span className="settings-warning">
              Already used by {actionDisplayName(pending.conflict)}
            </span>
            <span className="settings-hotkey-buttons">
              <button
                type="button"
                onClick={(event) => {
                  event.stopPropagation();
                  overrideBindings(store.getState(), action, pending.conflict, pending.binding);
                  stop();
                }}
              >
                Override
              </button>
              <button
                type="button"
                onClick={(event) => {
                  event.stopPropagation();
                  stop();
                }}
              >
                Cancel
              </button>
            </span>
          </span>
        ) : (
          <span className="settings-hotkey-recording">
            <span className={notice === null ? 'settings-pulse' : 'settings-warning'} role="status">
              {notice ?? 'Press shortcut…'}
            </span>
            <button
              type="button"
              className="settings-link-button"
              onClick={(event) => {
                event.stopPropagation();
                stop();
              }}
            >
              Cancel
            </button>
          </span>
        )
      ) : (
        <span className="settings-keycaps" data-testid={`hotkey-keys-${action}`}>
          {customized && (
            <button
              type="button"
              className="settings-icon-button"
              title="Reset to default"
              aria-label={`Reset ${name} to default`}
              onClick={(event) => {
                event.stopPropagation();
                store.getState().resetBinding(action);
              }}
            >
              <Icon name="arrow.counterclockwise" size={11} />
            </button>
          )}
          {displayKeys(binding).map((key) => (
            <kbd key={key} className="settings-keycap">
              {key}
            </kbd>
          ))}
        </span>
      )}
    </div>
  );
}

export function SettingsHotkeysView({ store = useKeyBindingStore }: { store?: KeyBindingStore }) {
  const anyCustomized = store((state) =>
    HOTKEY_ACTIONS.some((action) => state.overrides[action] !== undefined),
  );
  return (
    <div className="settings-pane" data-testid="settings-hotkeys">
      <div className="settings-title-row">
        <h2 className="settings-title">Hotkeys</h2>
        <span className="settings-spacer" />
        <button
          type="button"
          className="settings-link-button"
          disabled={!anyCustomized}
          onClick={() => {
            store.getState().resetAllBindings();
          }}
        >
          Reset All
        </button>
      </div>
      {HOTKEY_CATEGORIES.map((category) => (
        <SettingsGroup key={category} label={CATEGORY_DISPLAY_NAMES[category]}>
          {actionsFor(category).map((action, index) => (
            <div key={action}>
              {index > 0 && <hr className="settings-divider" />}
              <HotkeyRow action={action} store={store} />
            </div>
          ))}
        </SettingsGroup>
      ))}
    </div>
  );
}
