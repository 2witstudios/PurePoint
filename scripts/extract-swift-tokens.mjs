#!/usr/bin/env node
// Lists every design token the macOS theme files define (WIN-5.8), as JSON.
//
// Reads apps/purepoint-macos/purepoint-macos/Theme/{PurePointTheme,Theme,TerminalTheme,EditorTheme}.swift and
// reports each `static let`/`static var` (stored or computed) of a theme enum that is a color, font or metric,
// plus the ANSI palette tuples inside `TerminalTheme.ansiPalette(isDark:)`. A static whose value the parser does
// not understand is still reported, with `kind: "unknown"`, so ui/src/theme/tokens.test.ts fails on it instead
// of silently skipping a new token.
//
// Token shapes:
//   { id: "PurePointTheme.navRowHeight", kind: "metric", value: 26 }
//   { id: "PurePointTheme.navFont", kind: "font", family: "system", size: 12, weight: "medium" }
//   { id: "Theme.cardBackground", kind: "color", light: <color>, dark: <color> }
// where <color> is { rgba: [r, g, b, a] } (components 0-1, sRGB), { system: "windowBackgroundColor" },
// { system: "black", alpha: 0.045 } or { ref: "Theme.cardBackground" }.
// Every token also has `file`, `line`, `platform` ("all" | "macos" | "ios") and `source` (the Swift expression).
//
// Usage: node scripts/extract-swift-tokens.mjs [--repo <dir>]
// No dependencies; Node 18+. ui/src/theme/tokens.test.ts imports `extractSwiftTokens` from here.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const THEME_DIR = 'apps/purepoint-macos/purepoint-macos/Theme';
export const THEME_FILES = ['PurePointTheme.swift', 'Theme.swift', 'TerminalTheme.swift', 'EditorTheme.swift'];

const NUM = String.raw`-?\d+(?:\.\d+)?`;
const WEIGHTS = new Set(['ultraLight', 'thin', 'light', 'regular', 'medium', 'semibold', 'bold', 'heavy', 'black']);

const nums = (s) => s.split(',').map((x) => Number(x.trim()));

function rgba(tuple, alpha) {
  const c = nums(tuple);
  if (c.some(Number.isNaN) || (c.length !== 3 && c.length !== 4)) return null;
  return { rgba: [c[0], c[1], c[2], c.length === 4 ? c[3] : (alpha ?? 1)] };
}

/** Parses a single NSColor expression that is not one of the adaptive helpers. */
function parseColor(expr) {
  const e = expr.trim();
  let m = e.match(new RegExp(String.raw`^NSColor\((?:srgbRed|red):\s*(${NUM}),\s*green:\s*(${NUM}),\s*blue:\s*(${NUM}),\s*alpha:\s*(${NUM})\)$`));
  if (m) return { rgba: [m[1], m[2], m[3], m[4]].map(Number) };
  m = e.match(new RegExp(String.raw`^NSColor\(white:\s*(${NUM}),\s*alpha:\s*(${NUM})\)$`));
  if (m) return { rgba: [Number(m[1]), Number(m[1]), Number(m[1]), Number(m[2])] };
  m = e.match(/^(?:NSColor)?\.(\w+)\.withAlphaComponent\(([\d.]+)\)$/) ?? e.match(/^NSColor\.(\w+)\.withAlphaComponent\(([\d.]+)\)$/);
  if (m) return { system: m[1], alpha: Number(m[2]) };
  m = e.match(/^(?:NSColor)?\.(\w+)$/);
  if (m) return { system: m[1] };
  m = e.match(/^(Theme|EditorTheme|TerminalTheme|PurePointTheme)\.(\w+)$/);
  if (m) return { ref: `${m[1]}.${m[2]}` };
  return null;
}

/** Classifies a static's value expression. Returns the token fields (without id/file/line) or null. */
export function classify(expr, type) {
  const e = expr.trim();
  let m;
  // Metrics: a CGFloat/Double literal.
  if (new RegExp(`^${NUM}$`).test(e) && (!type || /^(CGFloat|Double|Float|Int)$/.test(type))) {
    return { kind: 'metric', value: Number(e) };
  }
  // Fonts.
  m = e.match(new RegExp(String.raw`^(?:Font)?\.system\(size:\s*(${NUM})(?:,\s*weight:\s*\.(\w+))?(?:,\s*design:\s*\.(\w+))?\)$`));
  if (m && (!m[2] || WEIGHTS.has(m[2]))) {
    return { kind: 'font', family: m[3] === 'monospaced' ? 'monospace' : 'system', size: Number(m[1]), weight: m[2] ?? 'regular' };
  }
  m = e.match(new RegExp(String.raw`^NSFont\(name:\s*"([^"]+)",\s*size:\s*(${NUM})\)(?:\s*\?\?\s*NSFont\.monospacedSystemFont\(ofSize:\s*${NUM},\s*weight:\s*\.(\w+)\))?$`));
  if (m) return { kind: 'font', family: m[1], fallback: m[3] ? 'monospace' : undefined, size: Number(m[2]), weight: m[3] ?? 'regular' };
  // Colors: the adaptive helpers of Theme.swift and EditorTheme.swift.
  m = e.match(/^adaptive\(\s*dark:\s*\(([^)]*)\),\s*light:\s*\(([^)]*)\)\s*\)$/);
  if (m) {
    const dark = rgba(m[1]);
    const light = rgba(m[2]);
    if (dark && light) return { kind: 'color', light, dark };
  }
  m = e.match(new RegExp(String.raw`^adaptiveGray\(\s*dark:\s*(${NUM}),\s*light:\s*(${NUM})\s*\)$`));
  if (m) {
    const g = (v) => ({ rgba: [Number(v), Number(v), Number(v), 1] });
    return { kind: 'color', light: g(m[2]), dark: g(m[1]) };
  }
  m = e.match(new RegExp(String.raw`^darkOnly\(\s*\(([^)]*)\)(?:,\s*alpha:\s*(${NUM}))?,\s*light:\s*(.+)\)$`));
  if (m) {
    const dark = rgba(m[1], m[2] === undefined ? 1 : Number(m[2]));
    const light = parseColor(m[3]);
    if (dark && light) return { kind: 'color', light, dark };
  }
  const single = parseColor(e);
  if (single && (single.rgba || single.ref || /^NSColor\b/.test(e) || /Color$/.test(type ?? ''))) {
    return { kind: 'color', light: single, dark: single };
  }
  return null;
}

/** True for a static's type or value that makes it a design token rather than, say, a String. */
function isTokenCandidate(type, expr) {
  if (type) return /^(CGFloat|Double|Float|Int|Font|NSFont|UIFont|Color|NSColor|UIColor)$/.test(type);
  return /^(-?\d|NSColor|NSFont|Font\.|\.system\(|adaptive|darkOnly|Theme\.|EditorTheme\.|TerminalTheme\.|PurePointTheme\.)/.test(expr.trim());
}

/**
 * Extracts the tokens of one Swift file's theme enums.
 * @param {string} text Swift source
 * @param {string} file file name for reporting
 */
export function extractFromSwift(text, file = '<swift>') {
  const lines = text.split(/\r?\n/);
  const tokens = [];
  const platformStack = []; // entries: "macos" | "ios" | "other"
  let depth = 0;
  let enumName = null;
  let enumDepth = -1;
  let func = null; // { name, depth, branch, isDarkFirst, darkList, lightList }

  const platform = () => {
    for (let i = platformStack.length - 1; i >= 0; i--) if (platformStack[i] !== 'other') return platformStack[i];
    return 'all';
  };

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const code = raw.replace(/\/\/.*$/, '').trimEnd();
    const t = code.trim();

    // Conditional compilation.
    let m = t.match(/^#if\s+os\((macOS|iOS)\)$/);
    if (m) { platformStack.push(m[1] === 'macOS' ? 'macos' : 'ios'); continue; }
    if (/^#if\b/.test(t)) { platformStack.push('other'); continue; }
    if (t === '#else') {
      const top = platformStack.pop();
      platformStack.push(top === 'macos' ? 'ios' : top === 'ios' ? 'macos' : 'other');
      continue;
    }
    if (t === '#endif') { platformStack.pop(); continue; }

    const enumMatch = t.match(/^(?:(?:nonisolated|public|internal|private|fileprivate)\s+)*enum\s+(\w+)\b/);
    if (enumMatch && enumName === null) { enumName = enumMatch[1]; enumDepth = depth; }

    if (enumName !== null && depth === enumDepth + 1) {
      const funcMatch = t.match(/^(?:(?:private|fileprivate|public|internal)\s+)?static\s+func\s+(\w+)\s*\(([^)]*)\)/);
      if (funcMatch) {
        func = { name: funcMatch[1], depth, condition: null, branch: null, lists: { true: [], false: [] } };
      }
      const st = t.match(/^(?:(?:private|fileprivate|public|internal|nonisolated)\s+)*static\s+(let|var)\s+(\w+)\s*(?::\s*([\w.?]+))?\s*(=\s*(.+)|\{\s*(.*?)\s*\}?)?$/);
      if (st && !funcMatch) {
        const [, , name, typeRaw, , assigned, computedInline] = st;
        const type = typeRaw?.replace(/\?$/, '');
        let expr = assigned ?? computedInline ?? '';
        // A computed property whose body is on the following lines: `static var x: CGFloat {` / `12` / `}`.
        if (assigned === undefined && t.endsWith('{') && !computedInline) {
          const body = [];
          for (let j = i + 1; j < lines.length && lines[j].trim() !== '}'; j++) body.push(lines[j].replace(/\/\/.*$/, '').trim());
          expr = body.join(' ').replace(/^return\s+/, '');
        }
        expr = expr.replace(/^return\s+/, '').replace(/\s*\}$/, '').trim();
        if (isTokenCandidate(type, expr)) {
          const parsed = classify(expr, type);
          tokens.push({
            id: `${enumName}.${name}`, enum: enumName, name, file, line: i + 1, platform: platform(), source: expr,
            ...(parsed ?? { kind: 'unknown' }),
          });
        }
      }
    }

    // Tuple palettes inside a static func: `cond ? [ (r, g, b), // n name ... ] : [ ... ]`.
    if (func) {
      const cond = t.match(/^(\w+)$/) ?? t.match(/=\s*(\w+)$/);
      if (cond && /dark/i.test(cond[1])) func.condition = cond[1];
      if (/^\?\s*\[$/.test(t)) func.branch = 'true';
      else if (/^:\s*\[$/.test(t)) func.branch = 'false';
      else if (/^\]$/.test(t)) func.branch = null;
      const tuple = raw.match(new RegExp(String.raw`^\s*\((${NUM}),\s*(${NUM}),\s*(${NUM})\),?\s*(?://\s*(\d+)\s*(.*))?$`));
      if (tuple && func.branch) {
        func.lists[func.branch].push({ rgba: [Number(tuple[1]), Number(tuple[2]), Number(tuple[3]), 1], index: tuple[4], label: tuple[5]?.trim(), line: i + 1 });
      }
    }

    for (const ch of code.replace(/"(?:[^"\\]|\\.)*"/g, '')) {
      if (ch === '{') depth++;
      else if (ch === '}') {
        depth--;
        if (func && depth === func.depth) {
          const darkFirst = func.condition !== null;
          const dark = darkFirst ? func.lists.true : func.lists.false;
          const light = darkFirst ? func.lists.false : func.lists.true;
          const n = Math.max(dark.length, light.length);
          for (let k = 0; k < n; k++) {
            const d = dark[k];
            const l = light[k];
            const index = d?.index ?? l?.index ?? String(k);
            tokens.push({
              id: `${enumName}.${func.name}.${index}`, enum: enumName, name: `${func.name}.${index}`, file, line: (d ?? l).line,
              platform: platform(), source: `${func.name}[${index}] ${d?.label ?? l?.label ?? ''}`.trim(),
              ...(d && l ? { kind: 'color', light: { rgba: l.rgba }, dark: { rgba: d.rgba }, label: d.label ?? l.label } : { kind: 'unknown' }),
            });
          }
          func = null;
        }
        if (enumName !== null && depth === enumDepth) { enumName = null; enumDepth = -1; }
      }
    }
  }
  return tokens;
}

/**
 * Extracts the tokens of several Swift files.
 * @param {{ file: string, text: string }[]} files
 */
export function extractSwiftTokens(files) {
  return files.flatMap(({ file, text }) => extractFromSwift(text, file));
}

function main(argv) {
  const repoFlag = argv.indexOf('--repo');
  const repo = repoFlag >= 0 ? path.resolve(argv[repoFlag + 1]) : path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const files = THEME_FILES.map((f) => ({ file: f, text: fs.readFileSync(path.join(repo, THEME_DIR, f), 'utf8') }));
  const tokens = extractSwiftTokens(files);
  const count = (k) => tokens.filter((t) => t.kind === k).length;
  process.stdout.write(`${JSON.stringify({ counts: { total: tokens.length, color: count('color'), font: count('font'), metric: count('metric'), unknown: count('unknown') }, tokens }, null, 2)}\n`);
  return count('unknown') > 0 ? 1 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
