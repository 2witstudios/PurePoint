// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsDisplayView.swift (WIN-8.11).
// Appearance applies live (theme/themeStore.ts) and so does the terminal font size, to every open
// terminal (views/terminal/terminalFontStore.ts); the grid gap is stored, as on the Mac.
import { useStore } from 'zustand';
import {
  APP_APPEARANCES,
  GRID_GAP_SLIDER,
  type SettingsStore,
  TERMINAL_FONT_SLIDER,
  settingsStore,
} from '../../state/settingsState.ts';
import { type ThemePreference, useThemeStore } from '../../theme/themeStore.ts';
import { terminalFontStore } from '../terminal/terminalFontStore.ts';
import { SettingsGroup } from './SettingsGroup.tsx';

export function SettingsDisplayView({
  store = settingsStore,
  fontStore = terminalFontStore,
}: {
  store?: SettingsStore;
  fontStore?: typeof terminalFontStore;
}) {
  const settings = useStore(store);
  const appearance = useThemeStore((state) => state.preference);
  const fontSize = useStore(fontStore, (state) => state.fontSize);

  return (
    <div className="settings-pane" data-testid="settings-display">
      <h2 className="settings-title">Display</h2>

      <SettingsGroup label="Appearance">
        <div className="settings-segmented" role="radiogroup" aria-label="Appearance">
          {APP_APPEARANCES.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={appearance === option.value}
              data-value={option.value}
              onClick={() => {
                settings.setAppearance(option.value satisfies ThemePreference);
              }}
            >
              {option.label}
            </button>
          ))}
        </div>
      </SettingsGroup>

      <SettingsGroup label="Terminal">
        <div className="settings-slider-row">
          <label htmlFor="settings-terminal-font-size">Font Size</label>
          <input
            id="settings-terminal-font-size"
            type="range"
            aria-label="Terminal font size"
            min={TERMINAL_FONT_SLIDER.min}
            max={TERMINAL_FONT_SLIDER.max}
            step={1}
            value={fontSize}
            onChange={(event) => {
              settings.setTerminalFontSize(Number(event.target.value));
            }}
          />
          <span className="settings-slider-value" data-testid="terminal-font-size-value">
            {`${String(fontSize)}pt`}
          </span>
        </div>
      </SettingsGroup>

      <SettingsGroup label="Layout">
        <div className="settings-slider-row">
          <label htmlFor="settings-grid-gap">Grid Gap</label>
          <input
            id="settings-grid-gap"
            type="range"
            aria-label="Grid gap"
            min={GRID_GAP_SLIDER.min}
            max={GRID_GAP_SLIDER.max}
            step={1}
            value={settings.gridGap}
            onChange={(event) => {
              settings.setGridGap(Number(event.target.value));
            }}
          />
          <span className="settings-slider-value">{`${String(settings.gridGap)}px`}</span>
        </div>
      </SettingsGroup>
    </div>
  );
}
