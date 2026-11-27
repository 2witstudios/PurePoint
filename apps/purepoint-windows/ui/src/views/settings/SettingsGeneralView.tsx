// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsGeneralView.swift (WIN-8.11).
//
// Differences from Swift: the notification sound toggles are not ported (see state/settingsState.ts),
// and the palette shortcut is Ctrl+Shift+N.
import { useEffect, useMemo } from 'react';
import { useStore } from 'zustand';
import {
  type CommandPaletteItem,
  buildItems,
  categoryLabel,
  displayName,
  itemIcon,
  itemId,
} from '../../model/commandPalette.ts';
import type { AgentDefinition, SwarmDefinition } from '../../model/agentsHubModels.ts';
import { type AgentVariant, variantsWithWorktree } from '../../model/agentVariant.ts';
import type { AppState } from '../../state/appState.ts';
import { agentConfigStateFor } from '../../state/agentConfigState.ts';
import { agentsHubStateFor } from '../../state/agentsHubState.ts';
import {
  SIGNAL_KINDS,
  SIGNAL_KIND_LABELS,
  type SettingsStore,
  settingsStore,
} from '../../state/settingsState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { SF_SYMBOLS, type SfSymbolName } from '../../theme/icons.ts';
import { agentDefInfo, swarmDefInfo } from '../palette/openPalette.ts';
import { SettingsCaption, SettingsGroup, SettingsToggle } from './SettingsGroup.tsx';

/** Swift `paletteItems`: what the palette lists, in the saved order. */
export function paletteItems(
  agents: readonly AgentDefinition[],
  swarms: readonly SwarmDefinition[],
  order: readonly string[],
  addedAgents: readonly AgentVariant[] = [],
): CommandPaletteItem[] {
  return buildItems({
    builtInVariants: variantsWithWorktree(addedAgents),
    agents: agents.map(agentDefInfo),
    swarms: swarms.map(swarmDefInfo),
    preferredOrder: order,
  });
}

function symbol(name: string): SfSymbolName {
  return name in SF_SYMBOLS ? (name as SfSymbolName) : 'cpu';
}

export function SettingsGeneralView({
  app,
  store = settingsStore,
}: {
  app: AppState;
  store?: SettingsStore;
}) {
  const settings = useStore(store);
  const hub = agentsHubStateFor(app);
  const agents = useStore(hub.store, (state) => state.agents);
  const swarms = useStore(hub.store, (state) => state.swarms);
  const roots = useStore(app.store, () => app.libraryProjectRoots.join('\n'));
  const addedAgents = useStore(agentConfigStateFor(app).store, (state) => state.addedAgents);

  useEffect(() => {
    void hub.loadAll(roots === '' ? [] : roots.split('\n'));
  }, [hub, roots]);

  const items = useMemo(
    () => paletteItems(agents, swarms, settings.commandPaletteOrder, addedAgents),
    [agents, swarms, settings.commandPaletteOrder, addedAgents],
  );

  return (
    <div className="settings-pane" data-testid="settings-general">
      <h2 className="settings-title">General</h2>

      <SettingsGroup>
        <SettingsToggle
          label="Restore projects on launch"
          checked={settings.restoreProjectsOnLaunch}
          onChange={settings.setRestoreProjectsOnLaunch}
        />
        <hr className="settings-divider" />
        <SettingsToggle
          label="Use point guard names"
          checked={settings.usePointGuardNames}
          onChange={settings.setUsePointGuardNames}
        />
        <SettingsCaption>
          Turn off to name each agent or terminal before launching it.
        </SettingsCaption>
        <hr className="settings-divider" />
        <SettingsToggle
          label="Launch at login"
          checked={settings.launchAtLogin}
          onChange={settings.setLaunchAtLogin}
        />
      </SettingsGroup>

      <SettingsGroup label="Agent notifications">
        {SIGNAL_KINDS.map((kind) => (
          <SettingsToggle
            key={kind}
            label={`${SIGNAL_KIND_LABELS[kind]} notifications`}
            checked={settings.signalNotifications[kind]}
            onChange={(enabled) => {
              settings.setSignalNotifications(kind, enabled);
            }}
          />
        ))}
        <SettingsCaption>
          Alerts become available for qualified agent signals and show while PurePoint is in the
          background. Notifications play the Windows notification sound.
        </SettingsCaption>
      </SettingsGroup>

      <SettingsGroup
        label={
          <>
            <span>Command Palette Order</span>
            <button
              type="button"
              className="settings-link-button"
              disabled={settings.commandPaletteOrder.length === 0}
              onClick={() => {
                settings.setCommandPaletteOrder([]);
              }}
            >
              Reset
            </button>
          </>
        }
      >
        <ul className="settings-list" aria-label="Command palette order">
          {items.map((item, index) => {
            const name = displayName(item);
            const category = categoryLabel(item);
            return (
              <li key={itemId(item)} className="settings-list-row" data-id={itemId(item)}>
                <Icon name={symbol(itemIcon(item))} className="settings-secondary" />
                <span>{name}</span>
                {category !== null && <span className="settings-caption-inline">{category}</span>}
                <span className="settings-spacer" />
                <button
                  type="button"
                  className="settings-icon-button"
                  aria-label={`Move ${name} up`}
                  title={`Move ${name} up`}
                  disabled={index === 0}
                  onClick={() => {
                    settings.moveCommandPaletteItem(itemId(item), -1, items);
                  }}
                >
                  <Icon name="chevron.up" size={14} />
                </button>
                <button
                  type="button"
                  className="settings-icon-button"
                  aria-label={`Move ${name} down`}
                  title={`Move ${name} down`}
                  disabled={index === items.length - 1}
                  onClick={() => {
                    settings.moveCommandPaletteItem(itemId(item), 1, items);
                  }}
                >
                  <Icon name="chevron.down" size={14} />
                </button>
              </li>
            );
          })}
        </ul>
      </SettingsGroup>
      <SettingsCaption>
        Use the arrows to change the order in Ctrl+Shift+N. The first entry is selected by default.
        Custom commands enabled in the Agents Hub appear here too.
      </SettingsCaption>
    </div>
  );
}
