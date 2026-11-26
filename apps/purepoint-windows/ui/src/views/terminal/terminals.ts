// The app's terminal cache (WIN-7.3): one `TerminalViewCache` of `TerminalPaneController`s, the
// hidden element that parks hidden terminals, and live font-size and appearance changes.
// `configureTerminals` is how the app tells it agents' live status (Swift `agentStatus`).
import { TerminalPaneController, type TerminalPaneOptions } from './terminalPaneView.ts';
import { terminalFontStore } from './terminalFontStore.ts';
import { TerminalViewCache } from './terminalViewCache.ts';

export interface TerminalHost {
  readonly cache: TerminalViewCache<TerminalPaneController>;
  /** Hidden terminals live here, out of layout. */
  readonly parking: HTMLElement;
}

interface TerminalConfig {
  isAgentAlive: (agentId: string) => boolean;
  machineIdFor: (agentId: string) => string | undefined;
}

const config: TerminalConfig = {
  isAgentAlive: () => false,
  machineIdFor: () => undefined,
};

/** Live agent status and owning machine, from the app state. */
export function configureTerminals(next: Partial<TerminalConfig>): void {
  Object.assign(config, next);
}

let host: TerminalHost | null = null;

function createParking(): HTMLElement {
  const parking = document.createElement('div');
  parking.className = 'terminal-parking';
  parking.setAttribute('aria-hidden', 'true');
  parking.style.display = 'none';
  document.body.appendChild(parking);
  return parking;
}

/** Build a host with its own cache; the app uses `terminalHost()`. */
export function createTerminalHost(
  options: Omit<TerminalPaneOptions, 'onGone' | 'onBackgroundOutput'> = {},
): TerminalHost {
  const parking = createParking();
  const cache: TerminalViewCache<TerminalPaneController> = new TerminalViewCache((agentId) => {
    const machineId = config.machineIdFor(agentId);
    return new TerminalPaneController(agentId, {
      fontSize: terminalFontStore.getState().fontSize,
      isAgentAlive: () => config.isAgentAlive(agentId),
      onBackgroundOutput: () => {
        cache.markUnseenOutput(agentId);
      },
      onGone: () => {
        cache.agentGone(agentId);
      },
      ...(machineId === undefined ? {} : { machineId }),
      ...options,
    });
  });
  terminalFontStore.subscribe((state, previous) => {
    if (state.fontSize === previous.fontSize) return;
    cache.forEach((view) => {
      view.setFontSize(state.fontSize);
    });
  });
  // Appearance: themeStore writes <html data-theme>; the tokens change under it.
  if (typeof MutationObserver !== 'undefined') {
    new MutationObserver(() => {
      cache.forEach((view) => {
        view.applyTheme();
      });
    }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }
  return { cache, parking };
}

export function terminalHost(): TerminalHost {
  host ??= createTerminalHost();
  installStatsHook(host);
  return host;
}

/** One terminal as the end-to-end tests see it. */
export interface TerminalProbe {
  agentId: string;
  shown: boolean;
  renderer: string;
  cols: number | null;
  rows: number | null;
  fontFamily: string | null;
  fontSize: number | null;
  bytesReceived: number;
  bytesParsed: number;
  checksum: number;
}

interface StatsHookWindow {
  __puTerminals?: {
    list(): TerminalProbe[];
    text(agentId: string): string | null;
    inputLog(agentId: string): readonly string[] | null;
    locate(agentId: string, needle: string): { x: number; y: number } | null;
    resetStats(agentId: string): void;
  };
}

/**
 * Read-only view of the terminals for the end-to-end tests (WebDriver reads it); resetting the
 * byte counters is its only effect. Nothing here sends input or opens sessions.
 */
function installStatsHook(target: TerminalHost): void {
  const w = window as StatsHookWindow;
  if (w.__puTerminals !== undefined) return;
  const { cache } = target;
  w.__puTerminals = {
    list: () => {
      const rows: TerminalProbe[] = [];
      cache.forEach((view, agentId) => {
        rows.push({
          agentId,
          shown: view.isShown,
          renderer: view.surface.renderer,
          cols: view.surface.cols ?? null,
          rows: view.surface.rows ?? null,
          fontFamily: view.surface.fontFamily ?? null,
          fontSize: view.surface.fontSize ?? null,
          ...view.stats,
        });
      });
      return rows;
    },
    text: (agentId) => cache.get(agentId)?.surface.bufferText?.() ?? null,
    inputLog: (agentId) => cache.get(agentId)?.inputLog ?? null,
    locate: (agentId, needle) => cache.get(agentId)?.surface.locate?.(needle) ?? null,
    resetStats: (agentId) => {
      cache.get(agentId)?.resetStats();
    },
  };
}
