// Design tokens ported from apps/purepoint-macos/purepoint-macos/Theme/ (WIN-5.8).
//
// The values live in tokens.css as `--pp-*` custom properties; this module names them for TypeScript
// and records how each Swift token maps to one. tokens.test.ts runs scripts/extract-swift-tokens.mjs
// over the Swift theme files and fails when a Swift token has no CSS counterpart or a different value.
//
// Name mapping (swiftTokenCssVar):
//   PurePointTheme.<name>          -> --pp-<kebab name>            PurePointTheme.navRowHeight -> --pp-nav-row-height
//   Theme.<name>                   -> --pp-<kebab name>            Theme.sidebarBackground     -> --pp-sidebar-background
//   TerminalTheme.<name>           -> --pp-terminal-<kebab name>   TerminalTheme.background    -> --pp-terminal-background
//   TerminalTheme.ansiPalette.<n>  -> --pp-terminal-ansi-<n>       TerminalTheme.ansiPalette.9 -> --pp-terminal-ansi-9
//   EditorTheme.<name>             -> --pp-editor-<kebab name>     EditorTheme.keyword         -> --pp-editor-keyword
// A font token <var> also gets <var>-family, <var>-size and <var>-weight; <var> itself is a `font` shorthand.
//
// Values: Mac points map to CSS px 1:1 (a point is a px at 100% scale). sRGB components c in 0..1 become
// round(c * 255). Colors that differ by appearance are set per theme in tokens.css; NSColor system colors
// resolve through the --pp-system-* tokens below (SYSTEM_COLORS).

export type Appearance = 'light' | 'dark';

/** Font stacks (DEC-9): Cascadia Mono replaces Menlo for terminals and the editor; Segoe UI is the UI face. */
export const FONT_STACKS = {
  ui: "'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif",
  mono: "'Cascadia Mono', Consolas, monospace",
} as const;

/** SwiftUI `Font.Weight` to CSS `font-weight`. */
export const FONT_WEIGHTS: Readonly<Record<string, number>> = {
  ultraLight: 100,
  thin: 200,
  light: 300,
  regular: 400,
  medium: 500,
  semibold: 600,
  bold: 700,
  heavy: 800,
  black: 900,
};

/**
 * The NSColor system colors the theme files use, resolved to concrete values from the macOS 26 (Tahoe)
 * defaults in the sRGB space with the default (graphite-free, no increased contrast) appearance:
 *
 * | NSColor                  | CSS token                          | light                  | dark                        |
 * |--------------------------|------------------------------------|------------------------|-----------------------------|
 * | windowBackgroundColor    | --pp-system-window-background      | #ececec (white 0.925)  | #323232 (white 0.196)       |
 * | controlBackgroundColor   | --pp-system-control-background     | #ffffff                | #1e1e1e (white 0.118)       |
 * | tertiaryLabelColor       | --pp-system-tertiary-label         | rgba(0, 0, 0, 0.26)    | rgba(255, 255, 255, 0.25)   |
 * | black                    | --pp-system-black                  | #000000                | #000000                     |
 * | clear                    | --pp-system-clear                  | transparent            | transparent                 |
 *
 * `NSColor.black.withAlphaComponent(a)` is written out as `rgba(0, 0, 0, a)`.
 */
export const SYSTEM_COLORS: Readonly<
  Record<string, { cssVar: string; light: string; dark: string }>
> = {
  windowBackgroundColor: {
    cssVar: '--pp-system-window-background',
    light: '#ececec',
    dark: '#323232',
  },
  controlBackgroundColor: {
    cssVar: '--pp-system-control-background',
    light: '#ffffff',
    dark: '#1e1e1e',
  },
  tertiaryLabelColor: {
    cssVar: '--pp-system-tertiary-label',
    light: 'rgba(0, 0, 0, 0.26)',
    dark: 'rgba(255, 255, 255, 0.25)',
  },
  black: { cssVar: '--pp-system-black', light: '#000000', dark: '#000000' },
  clear: { cssVar: '--pp-system-clear', light: 'transparent', dark: 'transparent' },
};

/** Swift tokens with no Windows counterpart, and why. */
export const EXCLUDED_SWIFT_TOKENS: Readonly<Record<string, string>> = {
  'PurePointTheme.iPadWindowMinWidth':
    'iOS only (`#if os(iOS)`): the iPad split-view minimum has no Windows window.',
};

const ENUM_PREFIX: Readonly<Record<string, string>> = {
  PurePointTheme: '--pp-',
  Theme: '--pp-',
  TerminalTheme: '--pp-terminal-',
  EditorTheme: '--pp-editor-',
};

export const kebab = (name: string): string =>
  name
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .toLowerCase();

/** The CSS custom property for a Swift token id such as `Theme.cardBackground`; throws for an unknown enum. */
export function swiftTokenCssVar(id: string): string {
  const [enumName = '', ...rest] = id.split('.');
  const prefix = ENUM_PREFIX[enumName];
  if (prefix === undefined) throw new Error(`no CSS prefix for Swift enum ${enumName}`);
  if (enumName === 'TerminalTheme' && rest[0] === 'ansiPalette' && rest.length === 2) {
    return `--pp-terminal-ansi-${rest[1] ?? ''}`;
  }
  return prefix + kebab(rest.join('-'));
}

/** sRGB components (0..1) to the CSS color tokens.css uses: `#rrggbb`, or `rgba(r, g, b, a)` when translucent. */
export function cssColor([r, g, b, a]: readonly [number, number, number, number]): string {
  const byte = (c: number) => Math.round(Math.min(1, Math.max(0, c)) * 255);
  if (a >= 1) return `#${[r, g, b].map((c) => byte(c).toString(16).padStart(2, '0')).join('')}`;
  return `rgba(${String(byte(r))}, ${String(byte(g))}, ${String(byte(b))}, ${String(a)})`;
}

/** A color as scripts/extract-swift-tokens.mjs reports it for one appearance. */
export type SwiftColor =
  | { rgba: readonly [number, number, number, number] }
  | { system: string; alpha?: number }
  | { ref: string };

/** The CSS value tokens.css gives a Swift color: a literal, a --pp-system-* token or another token. */
export function swiftColorCss(color: SwiftColor): string {
  if ('rgba' in color) return cssColor(color.rgba);
  if ('ref' in color) return `var(${swiftTokenCssVar(color.ref)})`;
  if (color.alpha !== undefined) {
    if (color.system !== 'black')
      throw new Error(`no translucent mapping for NSColor.${color.system}`);
    return `rgba(0, 0, 0, ${String(color.alpha)})`;
  }
  const system = SYSTEM_COLORS[color.system];
  if (!system) throw new Error(`NSColor.${color.system} has no entry in SYSTEM_COLORS`);
  return `var(${system.cssVar})`;
}

/** Every custom property tokens.css defines, without the `--pp-` prefix. */
export const TOKEN_NAMES = [
  'font-ui',
  'font-mono',
  'system-window-background',
  'system-control-background',
  'system-tertiary-label',
  'system-black',
  'system-clear',
  'nav-row-height',
  'sidebar-row-height',
  'status-dot-size',
  'padding',
  'footer-height',
  'tree-indent',
  'sidebar-min-width',
  'sidebar-ideal-width',
  'sidebar-max-width',
  'window-min-width',
  'window-min-height',
  'window-default-width',
  'window-default-height',
  'settings-width',
  'settings-height',
  'settings-sidebar-width',
  'calendar-hour-height',
  'calendar-time-gutter-width',
  'calendar-header-height',
  'calendar-pill-height',
  'chat-content-width',
  'chat-horizontal-padding',
  'composer-corner-radius',
  'turn-spacing',
  'inline-spacing',
  'accent-bar-width',
  'nav-font-family',
  'nav-font-size',
  'nav-font-weight',
  'nav-font',
  'tree-font-family',
  'tree-font-size',
  'tree-font-weight',
  'tree-font',
  'small-font-family',
  'small-font-size',
  'small-font-weight',
  'small-font',
  'chat-font-family',
  'chat-font-size',
  'chat-font-weight',
  'chat-font',
  'chat-label-font-family',
  'chat-label-font-size',
  'chat-label-font-weight',
  'chat-label-font',
  'primary-text',
  'tab-bar-background',
  'status-bar-band',
  'chat-background',
  'chat-rail-background',
  'composer-background',
  'composer-shadow',
  'user-bubble-background',
  'point-guard-accent',
  'sidebar-background',
  'card-background',
  'card-header-background',
  'addition-background',
  'deletion-background',
  'addition-text',
  'deletion-text',
  'hunk-separator',
  'calendar-grid-line',
  'calendar-today-background',
  'terminal-font-family',
  'terminal-font-size',
  'terminal-font-weight',
  'terminal-font',
  'terminal-background',
  'terminal-foreground',
  'terminal-ansi-0',
  'terminal-ansi-1',
  'terminal-ansi-2',
  'terminal-ansi-3',
  'terminal-ansi-4',
  'terminal-ansi-5',
  'terminal-ansi-6',
  'terminal-ansi-7',
  'terminal-ansi-8',
  'terminal-ansi-9',
  'terminal-ansi-10',
  'terminal-ansi-11',
  'terminal-ansi-12',
  'terminal-ansi-13',
  'terminal-ansi-14',
  'terminal-ansi-15',
  'editor-keyword',
  'editor-string',
  'editor-comment',
  'editor-number',
  'editor-type',
  'editor-function',
  'editor-property',
  'editor-preprocessor',
  'editor-background',
  'editor-line-number-color',
  'editor-current-line-highlight',
  'editor-font-family',
  'editor-font-size',
  'editor-font-weight',
  'editor-font',
  'app-background',
  'app-text',
] as const;

export type TokenName = (typeof TOKEN_NAMES)[number];

/** `--pp-<name>`, for `style.setProperty` and friends. */
export const cssVarName = (name: TokenName): string => `--pp-${name}`;

/** `var(--pp-<name>)`, for inline styles. */
export const cssVar = (name: TokenName): string => `var(--pp-${name})`;

/**
 * The computed value of a token on `element` (the document root by default), for consumers that need
 * concrete values rather than CSS, such as the xterm.js theme.
 */
export function readToken(name: TokenName, element: Element = document.documentElement): string {
  return getComputedStyle(element).getPropertyValue(cssVarName(name)).trim();
}
