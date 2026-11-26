// TerminalTheme.swift usage on Windows (WIN-7.3): the xterm.js theme and font, read from the
// `--pp-terminal-*` tokens (ui/src/theme/tokens.css, WIN-5.8) so the terminal follows the app's
// light/dark appearance exactly as `TerminalTheme.apply(to:appearance:)` does on the Mac.
import type { ITheme } from '@xterm/xterm';
import { readToken, type TokenName } from '../../theme/tokens.ts';

/** xterm.js ANSI slot names in palette order (`TerminalTheme.ansiPalette` 0-15). */
const ANSI_KEYS = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightBlack',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite',
] as const satisfies readonly (keyof ITheme)[];

/** DEC-9 font stack, used when the tokens are not loaded (tests, a detached document). */
export const TERMINAL_FONT_FALLBACK = '"Cascadia Mono", Consolas, monospace';
/** `TerminalTheme.font` size, the default for the configurable terminal font size. */
export const TERMINAL_DEFAULT_FONT_SIZE = 13;

function token(name: TokenName, fallback: string, element: Element): string {
  const value = readToken(name, element);
  return value === '' ? fallback : value;
}

/** The terminal theme for the appearance currently applied to `element` (the root by default). */
export function terminalTheme(element: Element = document.documentElement): ITheme {
  const background = token('terminal-background', '#1c1c1f', element);
  const foreground = token('terminal-foreground', '#d9d9de', element);
  const theme: ITheme = {
    background,
    foreground,
    cursor: foreground,
    cursorAccent: background,
    // SwiftTerm draws its selection as a translucent foreground wash.
    selectionBackground: 'rgba(128, 128, 140, 0.4)',
  };
  ANSI_KEYS.forEach((key, index) => {
    const value = readToken(`terminal-ansi-${String(index)}` as TokenName, element);
    if (value !== '') theme[key] = value;
  });
  return theme;
}

/** The terminal font family (`--pp-terminal-font-family`, Cascadia Mono first). */
export function terminalFontFamily(element: Element = document.documentElement): string {
  return token('terminal-font-family', TERMINAL_FONT_FALLBACK, element);
}
