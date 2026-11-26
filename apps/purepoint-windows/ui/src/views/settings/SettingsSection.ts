// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsSection.swift (WIN-8.11).
// About is the Mac build's (it holds the update check), so it is always present on Windows.
import type { SfSymbolName } from '../../theme/icons.ts';

export const SETTINGS_SECTIONS = [
  'general',
  'pointGuard',
  'agents',
  'hotkeys',
  'display',
  'about',
] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export const SECTION_TITLES: Record<SettingsSection, string> = {
  general: 'General',
  pointGuard: 'Point Guard',
  agents: 'Agents',
  hotkeys: 'Hotkeys',
  display: 'Display',
  about: 'About',
};

export const SECTION_ICONS: Record<SettingsSection, SfSymbolName> = {
  general: 'gear',
  pointGuard: 'terminal',
  agents: 'cpu',
  hotkeys: 'keyboard',
  display: 'paintbrush',
  about: 'info.circle',
};
