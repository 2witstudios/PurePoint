import { describe, expect, it } from 'vitest';
import {
  extractSwiftTokens,
  type SwiftToken,
} from '../../../../../scripts/extract-swift-tokens.mjs';
import editorThemeSwift from '../../../../purepoint-macos/purepoint-macos/Theme/EditorTheme.swift?raw';
import purePointThemeSwift from '../../../../purepoint-macos/purepoint-macos/Theme/PurePointTheme.swift?raw';
import terminalThemeSwift from '../../../../purepoint-macos/purepoint-macos/Theme/TerminalTheme.swift?raw';
import themeSwift from '../../../../purepoint-macos/purepoint-macos/Theme/Theme.swift?raw';
import tokensCss from './tokens.css?raw';
import {
  EXCLUDED_SWIFT_TOKENS,
  FONT_WEIGHTS,
  SYSTEM_COLORS,
  TOKEN_NAMES,
  swiftColorCss,
  swiftTokenCssVar,
} from './tokens.ts';

const tokens = extractSwiftTokens([
  { file: 'PurePointTheme.swift', text: purePointThemeSwift },
  { file: 'Theme.swift', text: themeSwift },
  { file: 'TerminalTheme.swift', text: terminalThemeSwift },
  { file: 'EditorTheme.swift', text: editorThemeSwift },
]);
const ported = tokens.filter((t) => !(t.id in EXCLUDED_SWIFT_TOKENS));

const css = tokensCss.replace(/\/\*[\s\S]*?\*\//g, '');

function block(selector: RegExp): Map<string, string> {
  const match = css.match(new RegExp(`${selector.source}\\s*\\{([^}]*)\\}`));
  if (!match?.[1]) throw new Error(`tokens.css has no block for ${selector.source}`);
  const decls = new Map<string, string>();
  for (const decl of match[1].split(';')) {
    const m = decl.trim().match(/^(--[\w-]+)\s*:\s*([\s\S]+)$/);
    // Collapse the line breaks Prettier adds inside long values.
    const value = m?.[2]?.replace(/\s+/g, ' ').replace(/\( /g, '(').replace(/ \)/g, ')').trim();
    if (m?.[1] && value) decls.set(m[1], value);
  }
  return decls;
}

const light = block(/(?:^|\n):root/);
const dark = block(/:root\[data-theme='dark'\]/);
const mediaDark = block(/:root:not\(\[data-theme='light'\]\)/);
const valueIn = (appearance: 'light' | 'dark', name: string) =>
  appearance === 'dark' ? (dark.get(name) ?? light.get(name)) : light.get(name);

describe('Swift theme tokens', () => {
  it('extracts every token kind from the four theme files', () => {
    const kinds = new Set(tokens.map((t) => t.kind));
    expect(kinds).toEqual(new Set(['metric', 'font', 'color']));
    expect(tokens.filter((t) => t.kind === 'unknown').map((t) => t.id)).toEqual([]);
    expect(tokens.filter((t) => t.id.startsWith('TerminalTheme.ansiPalette.'))).toHaveLength(16);
  });

  it('only excludes tokens that exist', () => {
    const ids = new Set(tokens.map((t) => t.id));
    for (const id of Object.keys(EXCLUDED_SWIFT_TOKENS)) expect(ids).toContain(id);
  });

  it('maps every Swift token to a distinct CSS custom property', () => {
    const names = ported.map((t) => swiftTokenCssVar(t.id));
    expect(new Set(names).size).toBe(names.length);
    expect(swiftTokenCssVar('Theme.sidebarBackground')).toBe('--pp-sidebar-background');
    expect(swiftTokenCssVar('TerminalTheme.ansiPalette.12')).toBe('--pp-terminal-ansi-12');
  });

  it.each(ported.map((t): [string, SwiftToken] => [t.id, t]))(
    '%s has a CSS counterpart',
    (_id, token) => {
      const name = swiftTokenCssVar(token.id);
      switch (token.kind) {
        case 'metric':
          expect(light.get(name)).toBe(`${String(token.value)}px`);
          break;
        case 'font':
          expect(light.get(`${name}-size`)).toBe(`${String(token.size)}px`);
          expect(light.get(`${name}-weight`)).toBe(String(FONT_WEIGHTS[token.weight]));
          expect(light.get(`${name}-family`)).toBe(
            token.family === 'system' ? 'var(--pp-font-ui)' : 'var(--pp-font-mono)',
          );
          expect(light.get(name)).toBe(`var(${name}-weight) var(${name}-size) var(${name}-family)`);
          break;
        case 'color':
          expect(valueIn('light', name)).toBe(swiftColorCss(token.light));
          expect(valueIn('dark', name)).toBe(swiftColorCss(token.dark));
          break;
        case 'unknown':
          throw new Error(
            `${token.id} (${token.file}:${String(token.line)}) is not a recognised token: ${token.source}`,
          );
      }
    },
  );
});

describe('tokens.css', () => {
  it('defines the same dark overrides for data-theme and prefers-color-scheme', () => {
    expect(mediaDark).toEqual(dark);
  });

  it('only overrides tokens in dark that it defines in light', () => {
    for (const name of dark.keys()) expect(light.has(name)).toBe(true);
  });

  it('resolves every NSColor system color in both appearances', () => {
    for (const { cssVar, light: l, dark: d } of Object.values(SYSTEM_COLORS)) {
      expect(valueIn('light', cssVar)).toBe(l);
      expect(valueIn('dark', cssVar)).toBe(d);
    }
  });

  it('lists every custom property in TOKEN_NAMES', () => {
    expect(new Set(TOKEN_NAMES.map((n) => `--pp-${n}`))).toEqual(new Set(light.keys()));
  });

  it('uses Cascadia Mono 13px for terminals and the editor and Segoe UI for text (DEC-9)', () => {
    expect(light.get('--pp-font-mono')).toBe("'Cascadia Mono', Consolas, monospace");
    expect(light.get('--pp-font-ui')).toBe(
      "'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif",
    );
    expect(light.get('--pp-terminal-font-size')).toBe('13px');
    expect(light.get('--pp-editor-font-family')).toBe('var(--pp-font-mono)');
    expect(light.get('--pp-editor-font-size')).toBe('var(--pp-terminal-font-size)');
  });

  it('never references an undefined custom property', () => {
    for (const value of light.values()) {
      for (const [, ref] of value.matchAll(/var\((--[\w-]+)\)/g))
        expect(light.has(ref ?? '')).toBe(true);
    }
  });
});
