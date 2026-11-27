import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import type { Appearance } from './tokens.ts';

/** The Settings appearance choice: follow Windows, or force one appearance. */
export type ThemePreference = 'system' | 'light' | 'dark';

export const THEME_STORAGE_KEY = 'purepoint.theme';
const DARK_QUERY = '(prefers-color-scheme: dark)';

interface ThemeState {
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set) => ({
      preference: 'system',
      setPreference: (preference) => {
        set({ preference });
      },
    }),
    {
      name: THEME_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      partialize: ({ preference }) => ({ preference }),
    },
  ),
);

function systemQuery(): MediaQueryList | null {
  return typeof window.matchMedia === 'function' ? window.matchMedia(DARK_QUERY) : null;
}

/** The appearance a preference selects, given whether Windows is in dark mode. */
export function resolveAppearance(preference: ThemePreference, systemDark: boolean): Appearance {
  if (preference === 'system') return systemDark ? 'dark' : 'light';
  return preference;
}

/** Writes the resolved appearance and the preference onto <html> (`data-theme`, `data-theme-preference`). */
export function applyTheme(
  preference: ThemePreference,
  systemDark: boolean,
  root: HTMLElement = document.documentElement,
): Appearance {
  const appearance = resolveAppearance(preference, systemDark);
  root.dataset.theme = appearance;
  root.dataset.themePreference = preference;
  return appearance;
}

/**
 * Keeps <html data-theme> in step with the stored preference and the system appearance, live.
 * Returns a function that stops following both.
 */
export function startThemeSync(root: HTMLElement = document.documentElement): () => void {
  const query = systemQuery();
  const update = () => {
    applyTheme(useThemeStore.getState().preference, query?.matches ?? false, root);
  };
  update();
  const unsubscribe = useThemeStore.subscribe(update);
  query?.addEventListener('change', update);
  return () => {
    unsubscribe();
    query?.removeEventListener('change', update);
  };
}
