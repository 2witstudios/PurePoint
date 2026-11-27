#!/usr/bin/env node
// Checks docs/reference/windows-parity-matrix.md against the macOS app source (WIN-1.4).
//
// Fails (exit 1) when:
// - a Swift file under apps/purepoint-macos/purepoint-macos/ has no row, or a row names a file that no longer exists;
// - a test file under apps/purepoint-macos/purepoint-macosTests/ has no row, or a row names a missing file;
// - a row has a bad status, a non-excluded row does not name exactly one WIN leaf, or an excluded row gives no reason;
// - an SF Symbol used in the Swift sources has no Lucide icon in the symbol table.
// With --no-todo it also fails when any source or test row is still `todo`.
//
// Usage: node scripts/check-parity-matrix.mjs [--no-todo] [--repo <dir>]
// No dependencies; Node 18+.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const APP_DIR = 'apps/purepoint-macos/purepoint-macos';
export const TESTS_DIR = 'apps/purepoint-macos/purepoint-macosTests';
export const MATRIX = 'docs/reference/windows-parity-matrix.md';

const SOURCE_HEADER = ['Swift file', 'Lines', 'Windows location', 'Leaf', 'Status', 'Exclusion reason'];
const TEST_HEADER = ['Test file', 'Suites', 'Windows test', 'Leaf', 'Status', 'Note'];
const SYMBOL_HEADER = ['SF Symbol', 'Lucide icon', 'Used in'];
const SOURCE_STATUSES = new Set(['todo', 'ported', 'excluded']);
const TEST_STATUSES = new Set(['todo', 'ported', 'excluded', 'ios-only']);
const LEAF = /^WIN-\d+\.\d+$/;
const LUCIDE = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const NONE = new Set(['', '—', '-']);

// ---------------------------------------------------------------- matrix parsing

function splitRow(line) {
  const t = line.trim();
  if (!t.startsWith('|') || !t.endsWith('|')) return null;
  return t.slice(1, -1).split('|').map((c) => c.trim());
}

const unquote = (c) => c.replace(/^`(.*)`$/, '$1');

/** Returns the rows of the first table whose header row equals `header`, as objects keyed by header. */
export function parseTable(markdown, header) {
  const lines = markdown.split(/\r?\n/);
  const start = lines.findIndex((l) => {
    const cells = splitRow(l);
    return cells && cells.length === header.length && cells.every((c, i) => c === header[i]);
  });
  if (start < 0) return null;
  const rows = [];
  for (let i = start + 2; i < lines.length; i++) {
    const cells = splitRow(lines[i]);
    if (!cells) break;
    const row = { line: i + 1 };
    header.forEach((h, j) => { row[h] = cells[j] ?? ''; });
    row.cellCount = cells.length;
    rows.push(row);
  }
  return rows;
}

// ---------------------------------------------------------------- SF Symbol extraction

const SYMBOL_SHAPE = /^[a-z0-9]+(\.[a-z0-9]+)*$/;
// Helper functions in the Swift app whose first argument is an SF Symbol name.
export const SYMBOL_HELPERS = ['makeSymbolIcon', 'makeCountIcon', 'actionIcon', 'composerActionIcon', 'iconButton', 'symbolIcon', 'countIcon'];
const BS = '\\';

function stringEnd(s, i) {
  let j = i + 1;
  while (j < s.length && s[j] !== '"' && s[j] !== '\n') { if (s[j] === BS) j++; j++; }
  return j;
}

function stripComments(s) {
  let out = '';
  let i = 0;
  while (i < s.length) {
    if (s[i] === '"') { const j = stringEnd(s, i); out += s.slice(i, j + 1); i = j + 1; continue; }
    if (s.startsWith('//', i)) { while (i < s.length && s[i] !== '\n') i++; continue; }
    if (s.startsWith('/*', i)) { const e = s.indexOf('*/', i + 2); i = e < 0 ? s.length : e + 2; continue; }
    out += s[i++];
  }
  return out;
}

// String literals in the expression starting at i, up to the next top-level `,` `;` or closing bracket.
// A line break ends the expression unless the next line continues a ternary (`?` or `:`).
// Literals compared with == or != (such as `kind == "pr"`) are not symbols.
function expressionLiterals(s, i) {
  const lits = [];
  let depth = 0;
  for (; i < s.length; i++) {
    const c = s[i];
    if (c === '"') {
      const j = stringEnd(s, i);
      const before = s.slice(Math.max(0, i - 4), i);
      const after = s.slice(j + 1, j + 5);
      if (!/[=!]=\s*$/.test(before) && !/^\s*[=!]=/.test(after)) lits.push(s.slice(i + 1, j));
      i = j;
      continue;
    }
    if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c)) { if (depth === 0) break; depth--; }
    else if (depth === 0 && (c === ',' || c === ';')) break;
    else if (depth === 0 && c === '\n') {
      const next = s.slice(i + 1).match(/^\s*(\S)/);
      if (!next || !'?:'.includes(next[1])) break;
    }
  }
  return lits;
}

function braceBody(s, i) {
  let d = 0;
  let j = i;
  for (; j < s.length; j++) {
    if (s[j] === '{') d++;
    else if (s[j] === '}') { d--; if (d === 0) break; }
  }
  return s.slice(i, j + 1);
}

/**
 * SF Symbol names used in one Swift source: literals passed as `systemName:`, `systemImage:` or
 * `systemSymbolName:` (including ternary and `??` branches), labelled `icon:`/`symbol:` arguments,
 * values assigned to `...icon`/`...symbol` variables, returned from `icon`/`symbol` properties and
 * functions, and passed to the symbol helpers.
 */
export function extractSymbols(source) {
  const s = stripComments(source);
  const found = new Set();
  const add = (lits) => { for (const x of lits) if (SYMBOL_SHAPE.test(x)) found.add(x); };
  let m;
  const labelled = /\b(systemName|systemImage|systemSymbolName|icon|symbol|iconName|statusIcon|symbolName)\s*:\s*/g;
  while ((m = labelled.exec(s))) add(expressionLiterals(s, m.index + m[0].length));
  const assigned = /\b(?:let|var)\s+\w*(?:[Ii]con|[Ss]ymbol)\w*\s*(?::\s*[\w?]+\s*)?=\s*/g;
  while ((m = assigned.exec(s))) add(expressionLiterals(s, m.index + m[0].length));
  const bodies = /\b(?:var|func)\s+\w*(?:[Ii]con|[Ss]ymbol)\w*\b[^{=\n]*\{/g;
  while ((m = bodies.exec(s))) {
    const body = braceBody(s, m.index + m[0].length - 1);
    add([...body.matchAll(/"([^"\\\n]*)"/g)].map((x) => x[1]));
  }
  const helpers = new RegExp(`\\b(?:${SYMBOL_HELPERS.join('|')})\\(\\s*`, 'g');
  while ((m = helpers.exec(s))) add(expressionLiterals(s, m.index + m[0].length));
  return found;
}

// ---------------------------------------------------------------- checks

function listSwift(dir, recursive) {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) { if (recursive) walk(p); } else if (e.name.endsWith('.swift')) out.push(path.relative(dir, p).split(path.sep).join('/'));
    }
  };
  walk(dir);
  return out.sort();
}

function checkLeafAndStatus(row, kind, fileCol, statuses, reasonCol, errors) {
  const where = `${kind} row \`${unquote(row[fileCol])}\` (line ${row.line})`;
  const status = row.Status;
  const leaf = row.Leaf;
  if (!statuses.has(status)) { errors.push(`${where}: status "${status}" is not one of ${[...statuses].join(', ')}`); return; }
  const unowned = status === 'excluded' || status === 'ios-only';
  if (unowned) {
    if (NONE.has(row[reasonCol])) errors.push(`${where}: ${status} row gives no reason`);
    if (!NONE.has(leaf) && !LEAF.test(leaf)) errors.push(`${where}: leaf "${leaf}" is not WIN-<n>.<m>`);
  } else if (!LEAF.test(leaf)) {
    errors.push(`${where}: a ${status} row must name exactly one WIN leaf (got "${leaf}")`);
  }
}

/**
 * Checks the matrix. Returns { errors, todo, summary }.
 * @param {{ appDir: string, testsDir: string, matrixText: string, noTodo?: boolean }} opts
 */
export function checkParity({ appDir, testsDir, matrixText, noTodo = false }) {
  const errors = [];
  const sources = parseTable(matrixText, SOURCE_HEADER);
  const tests = parseTable(matrixText, TEST_HEADER);
  const symbols = parseTable(matrixText, SYMBOL_HEADER);
  if (!sources) errors.push(`source table not found (header: | ${SOURCE_HEADER.join(' | ')} |)`);
  if (!tests) errors.push(`test table not found (header: | ${TEST_HEADER.join(' | ')} |)`);
  if (!symbols) errors.push(`symbol table not found (header: | ${SYMBOL_HEADER.join(' | ')} |)`);
  if (errors.length) return { errors, todo: [], summary: {} };

  for (const [rows, header, kind] of [[sources, SOURCE_HEADER, 'source'], [tests, TEST_HEADER, 'test'], [symbols, SYMBOL_HEADER, 'symbol']]) {
    for (const r of rows) if (r.cellCount !== header.length) errors.push(`${kind} table line ${r.line}: ${r.cellCount} cells, expected ${header.length}`);
  }

  // Source files.
  const swift = listSwift(appDir, true);
  const swiftSet = new Set(swift);
  const seen = new Map();
  for (const r of sources) {
    const f = unquote(r['Swift file']);
    if (seen.has(f)) errors.push(`source row \`${f}\` (line ${r.line}) duplicates line ${seen.get(f)}`);
    seen.set(f, r.line);
    if (!swiftSet.has(f)) errors.push(`source row \`${f}\` (line ${r.line}) names a file that is not in ${APP_DIR}/`);
    checkLeafAndStatus(r, 'source', 'Swift file', SOURCE_STATUSES, 'Exclusion reason', errors);
    if (r.Status !== 'excluded' && NONE.has(r['Windows location'])) errors.push(`source row \`${f}\` (line ${r.line}) has no Windows location`);
  }
  for (const f of swift) if (!seen.has(f)) errors.push(`Swift file \`${f}\` is missing from the matrix`);

  // Test files (a file may have one row per suite).
  const testFiles = listSwift(testsDir, false);
  const testSet = new Set(testFiles);
  const testSeen = new Set();
  for (const r of tests) {
    const f = unquote(r['Test file']);
    testSeen.add(f);
    if (!testSet.has(f)) errors.push(`test row \`${f}\` (line ${r.line}) names a file that is not in ${TESTS_DIR}/`);
    checkLeafAndStatus(r, 'test', 'Test file', TEST_STATUSES, 'Note', errors);
    if ((r.Status === 'todo' || r.Status === 'ported') && NONE.has(r['Windows test'])) errors.push(`test row \`${f}\` (line ${r.line}) has no Windows test`);
  }
  for (const f of testFiles) if (!testSeen.has(f)) errors.push(`test file \`${f}\` is missing from the matrix`);

  // SF Symbols.
  const mapped = new Map();
  for (const r of symbols) {
    const sym = unquote(r['SF Symbol']);
    const icon = unquote(r['Lucide icon']);
    if (mapped.has(sym)) errors.push(`symbol row \`${sym}\` (line ${r.line}) is a duplicate`);
    if (!LUCIDE.test(icon)) errors.push(`symbol row \`${sym}\` (line ${r.line}): "${icon}" is not a kebab-case Lucide name`);
    mapped.set(sym, icon);
  }
  const used = new Map();
  for (const f of swift) for (const sym of extractSymbols(fs.readFileSync(path.join(appDir, f), 'utf8'))) if (!used.has(sym)) used.set(sym, f);
  for (const [sym, f] of [...used].sort()) if (!mapped.has(sym)) errors.push(`SF Symbol "${sym}" (used in \`${f}\`) has no Lucide icon in the matrix`);

  const todo = [
    ...sources.filter((r) => r.Status === 'todo').map((r) => unquote(r['Swift file'])),
    ...tests.filter((r) => r.Status === 'todo').map((r) => `${unquote(r['Test file'])}${r.Suites && r.Suites !== 'all' ? ` (${unquote(r.Suites)})` : ''}`),
  ];
  if (noTodo) for (const t of todo) errors.push(`--no-todo: \`${t}\` is still todo`);

  const count = (rows, s) => rows.filter((r) => r.Status === s).length;
  const summary = {
    sources: sources.length, sourcesTodo: count(sources, 'todo'), sourcesPorted: count(sources, 'ported'), sourcesExcluded: count(sources, 'excluded'),
    testRows: tests.length, testFiles: testFiles.length, testsIosOnly: count(tests, 'ios-only'),
    symbols: mapped.size, symbolsUsed: used.size,
  };
  return { errors, todo, summary };
}

// ---------------------------------------------------------------- CLI

function main(argv) {
  const noTodo = argv.includes('--no-todo');
  const repoIdx = argv.indexOf('--repo');
  const repo = repoIdx >= 0 ? path.resolve(argv[repoIdx + 1]) : path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const matrixPath = path.join(repo, MATRIX);
  if (!fs.existsSync(matrixPath)) { console.error(`check-parity-matrix: ${MATRIX} not found under ${repo}`); return 1; }
  const { errors, summary } = checkParity({
    appDir: path.join(repo, APP_DIR),
    testsDir: path.join(repo, TESTS_DIR),
    matrixText: fs.readFileSync(matrixPath, 'utf8'),
    noTodo,
  });
  const s = summary;
  if (s.sources !== undefined) {
    console.log(`parity matrix: ${s.sources} source files (${s.sourcesTodo} todo, ${s.sourcesPorted} ported, ${s.sourcesExcluded} excluded), ` +
      `${s.testFiles} test files in ${s.testRows} rows (${s.testsIosOnly} iOS-only), ${s.symbols} SF Symbols mapped (${s.symbolsUsed} used)`);
  }
  if (errors.length) {
    for (const e of errors) console.error(`error: ${e}`);
    console.error(`check-parity-matrix: ${errors.length} error(s)`);
    return 1;
  }
  console.log('check-parity-matrix: ok');
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
