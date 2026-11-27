import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  THEME_STORAGE_KEY,
  resolveAppearance,
  startThemeSync,
  useThemeStore,
} from './themeStore.ts';

type Listener = () => void;

function mockSystemDark(initial: boolean) {
  const listeners = new Set<Listener>();
  const query = {
    matches: initial,
    addEventListener: (_: string, l: Listener) => listeners.add(l),
    removeEventListener: (_: string, l: Listener) => listeners.delete(l),
  };
  vi.stubGlobal('matchMedia', () => query);
  return {
    set(dark: boolean) {
      query.matches = dark;
      for (const l of listeners) l();
    },
    listeners,
  };
}

const html = document.documentElement;

describe('theme store', () => {
  let stop: () => void = () => undefined;

  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState({ preference: 'system' });
  });

  afterEach(() => {
    stop();
    vi.unstubAllGlobals();
    delete html.dataset.theme;
    delete html.dataset.themePreference;
  });

  it('resolves preferences', () => {
    expect(resolveAppearance('system', true)).toBe('dark');
    expect(resolveAppearance('system', false)).toBe('light');
    expect(resolveAppearance('light', true)).toBe('light');
    expect(resolveAppearance('dark', false)).toBe('dark');
  });

  it('follows the system appearance live', () => {
    const system = mockSystemDark(false);
    stop = startThemeSync();
    expect(html.dataset.theme).toBe('light');
    expect(html.dataset.themePreference).toBe('system');
    system.set(true);
    expect(html.dataset.theme).toBe('dark');
  });

  it('applies and persists a Settings override', () => {
    const system = mockSystemDark(true);
    stop = startThemeSync();
    useThemeStore.getState().setPreference('light');
    expect(html.dataset.theme).toBe('light');
    system.set(false);
    useThemeStore.getState().setPreference('dark');
    expect(html.dataset.theme).toBe('dark');
    expect(JSON.parse(localStorage.getItem(THEME_STORAGE_KEY) ?? '{}')).toMatchObject({
      state: { preference: 'dark' },
    });
  });

  it('stops following when stopped', () => {
    const system = mockSystemDark(false);
    stop = startThemeSync();
    stop();
    expect(system.listeners.size).toBe(0);
    useThemeStore.getState().setPreference('dark');
    expect(html.dataset.theme).toBe('light');
  });

  it('works without matchMedia', () => {
    vi.stubGlobal('matchMedia', undefined);
    stop = startThemeSync();
    expect(html.dataset.theme).toBe('light');
  });
});
