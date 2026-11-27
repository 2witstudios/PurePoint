// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsView.swift (WIN-8.11): the
// section sidebar (with the version under it) and the selected pane. ContentView shows it in the
// Settings overlay (Swift `appState.showSettings`).
import { useState } from 'react';
import type { AppState } from '../../state/appState.ts';
import { agentConfigStateFor } from '../../state/agentConfigState.ts';
import { Icon } from '../../theme/Icon.tsx';
import {
  SECTION_ICONS,
  SECTION_TITLES,
  SETTINGS_SECTIONS,
  type SettingsSection,
} from './SettingsSection.ts';
import { SettingsAboutView, useAppVersion } from './SettingsAboutView.tsx';
import { SettingsAgentsView } from './SettingsAgentsView.tsx';
import { SettingsDisplayView } from './SettingsDisplayView.tsx';
import { SettingsGeneralView } from './SettingsGeneralView.tsx';
import { SettingsHotkeysView } from './SettingsHotkeysView.tsx';
import { SettingsPointGuardView } from './SettingsPointGuardView.tsx';
import { pointGuardSessionFor } from '../../state/pointGuardSession.ts';
import './settings.css';

export function SettingsView({
  app,
  initialSection = 'general',
  loadVersion,
}: {
  app: AppState;
  initialSection?: SettingsSection;
  loadVersion?: () => Promise<string>;
}) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const version = useAppVersion(loadVersion);
  const config = agentConfigStateFor(app);

  return (
    <div className="settings-view" data-testid="settings-view">
      <nav className="settings-sidebar" aria-label="Settings sections">
        <div className="settings-sidebar-title">Settings</div>
        {SETTINGS_SECTIONS.map((item) => (
          <button
            key={item}
            type="button"
            className="settings-sidebar-row"
            data-testid={`settings-section-${item}`}
            aria-current={section === item ? 'page' : undefined}
            onClick={() => {
              setSection(item);
            }}
          >
            <Icon name={SECTION_ICONS[item]} size={14} />
            <span>{SECTION_TITLES[item]}</span>
          </button>
        ))}
        <span className="settings-spacer" />
        <div className="settings-sidebar-version">{`v${version}`}</div>
      </nav>
      <div className="settings-content">
        {section === 'general' && <SettingsGeneralView app={app} />}
        {section === 'pointGuard' && (
          <SettingsPointGuardView
            config={config}
            service={pointGuardSessionFor(app).service}
            chat={pointGuardSessionFor(app).chat}
          />
        )}
        {section === 'agents' && <SettingsAgentsView app={app} config={config} />}
        {section === 'hotkeys' && <SettingsHotkeysView />}
        {section === 'display' && <SettingsDisplayView />}
        {section === 'about' && <SettingsAboutView version={version} />}
      </div>
    </div>
  );
}
