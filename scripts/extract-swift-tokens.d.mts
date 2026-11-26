// Types for extract-swift-tokens.mjs, so the Windows app's TypeScript tests can import it.

export type SwiftColor =
  | { rgba: [number, number, number, number] }
  | { system: string; alpha?: number }
  | { ref: string };

interface TokenBase {
  id: string;
  enum: string;
  name: string;
  file: string;
  line: number;
  platform: 'all' | 'macos' | 'ios';
  source: string;
}

export type SwiftToken = TokenBase &
  (
    | { kind: 'metric'; value: number }
    | { kind: 'font'; family: string; fallback?: string; size: number; weight: string }
    | { kind: 'color'; light: SwiftColor; dark: SwiftColor; label?: string }
    | { kind: 'unknown' }
  );

export const THEME_DIR: string;
export const THEME_FILES: string[];
export function classify(expr: string, type?: string): Omit<SwiftToken, keyof TokenBase> | null;
export function extractFromSwift(text: string, file?: string): SwiftToken[];
export function extractSwiftTokens(files: { file: string; text: string }[]): SwiftToken[];
