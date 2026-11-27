// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsAboutView.swift (WIN-8.11).
//
// Differences from Swift: the version is the app's (`getVersion`, tauri.conf.json) and the build
// is the commit it was built from (`__PUREPOINT_BUILD__`, vite.config.ts), Windows having no
// CFBundleVersion. Check for Updates… and its status line are the updater's About row
// (views/shell/UpdateDialog.tsx). The links are shown as text: the Mac's URLs are placeholders.
import { getVersion } from '@tauri-apps/api/app';
import { useEffect, useState } from 'react';
import logo from '../../../../src-tauri/icons/128x128.png';
import { AboutUpdateStatus } from '../shell/UpdateDialog.tsx';
import { SettingsGroup } from './SettingsGroup.tsx';

export const APP_BUILD: string = __PUREPOINT_BUILD__;

export function useAppVersion(load: () => Promise<string> = getVersion): string {
  const [version, setVersion] = useState('—');
  useEffect(() => {
    let live = true;
    load().then(
      (value) => {
        if (live) setVersion(value);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [load]);
  return version;
}

export function SettingsAboutView({ version }: { version: string }) {
  return (
    <div className="settings-pane settings-about" data-testid="settings-about">
      <div className="settings-about-header">
        <img
          className="settings-about-logo"
          src={logo}
          alt="PurePoint logo"
          width={64}
          height={64}
        />
        <h2 className="settings-about-name">PurePoint</h2>
        <span className="settings-secondary">Agent-first coding workspace</span>
      </div>

      <div className="settings-about-card">
        <SettingsGroup>
          <div className="settings-row">
            <span>Version</span>
            <span className="settings-spacer" />
            <span className="settings-mono settings-secondary" data-testid="about-version">
              {version}
            </span>
          </div>
          <hr className="settings-divider" />
          <div className="settings-row">
            <span>Build</span>
            <span className="settings-spacer" />
            <span className="settings-mono settings-secondary" data-testid="about-build">
              {APP_BUILD}
            </span>
          </div>
        </SettingsGroup>
      </div>

      <AboutUpdateStatus />

      <span className="settings-tertiary">© 2026 PurePoint</span>
    </div>
  );
}
