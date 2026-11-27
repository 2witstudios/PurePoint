// Tests for check-parity-matrix.mjs. Run with: node --test scripts/
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { checkParity, extractSymbols, parseTable, APP_DIR, TESTS_DIR, MATRIX } from './check-parity-matrix.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'check-parity-matrix.mjs');

function fixture({ swift = {}, tests = {} } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'parity-'));
  for (const [f, body] of Object.entries(swift)) {
    const p = path.join(root, APP_DIR, f);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, body);
  }
  fs.mkdirSync(path.join(root, TESTS_DIR), { recursive: true });
  for (const [f, body] of Object.entries(tests)) fs.writeFileSync(path.join(root, TESTS_DIR, f), body);
  return root;
}

function matrix({ sources = [], tests = [], symbols = [] }) {
  return [
    '# Windows Parity Matrix', '',
    '| Swift file | Lines | Windows location | Leaf | Status | Exclusion reason |', '|---|---|---|---|---|---|',
    ...sources.map(([f, loc, leaf, status, reason = '—']) => `| \`${f}\` | 1 | ${loc} | ${leaf} | ${status} | ${reason} |`), '',
    '| Test file | Suites | Windows test | Leaf | Status | Note |', '|---|---|---|---|---|---|',
    ...tests.map(([f, t, leaf, status, note = '—']) => `| \`${f}\` | all | ${t} | ${leaf} | ${status} | ${note} |`), '',
    '| SF Symbol | Lucide icon | Used in |', '|---|---|---|',
    ...symbols.map(([s, l]) => `| \`${s}\` | \`${l}\` | — |`), '',
  ].join('\n');
}

const SWIFT = {
  'ContentView.swift': 'struct ContentView { var body: some View { Image(systemName: "xmark") } }\n',
  'Models/Thing+iOS.swift': '#if os(iOS)\nlet x = 1\n#endif\n',
};
const TESTS = { 'ThingTests.swift': 'import Testing\n' };
const GOOD = {
  sources: [
    ['ContentView.swift', '`ui/src/views/ContentView.tsx`', 'WIN-7.1', 'todo'],
    ['Models/Thing+iOS.swift', '—', '—', 'excluded', '`+iOS` file'],
  ],
  tests: [['ThingTests.swift', '`ui/src/model/thing.test.ts`', 'WIN-6.1', 'todo']],
  symbols: [['xmark', 'x']],
};

function run(root, text, opts = {}) {
  return checkParity({ appDir: path.join(root, APP_DIR), testsDir: path.join(root, TESTS_DIR), matrixText: text, ...opts });
}

test('passes a complete matrix', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  const r = run(root, matrix(GOOD));
  assert.deepEqual(r.errors, []);
  assert.equal(r.summary.sources, 2);
  assert.equal(r.summary.sourcesExcluded, 1);
});

test('fails when a Swift file is missing from the matrix', () => {
  const root = fixture({ swift: { ...SWIFT, 'Views/New.swift': 'struct New {}\n' }, tests: TESTS });
  const r = run(root, matrix(GOOD));
  assert.ok(r.errors.some((e) => e.includes('`Views/New.swift` is missing')), r.errors.join('\n'));
});

test('fails when a row names a file that no longer exists', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  const m = matrix({ ...GOOD, sources: [...GOOD.sources, ['Gone.swift', '`ui/src/gone.ts`', 'WIN-7.1', 'todo']] });
  assert.ok(run(root, m).errors.some((e) => e.includes('`Gone.swift`') && e.includes('not in')));
});

test('fails when a non-excluded row lacks a leaf', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  const m = matrix({ ...GOOD, sources: [['ContentView.swift', '`ui/src/views/ContentView.tsx`', '—', 'todo'], GOOD.sources[1]] });
  assert.ok(run(root, m).errors.some((e) => e.includes('must name exactly one WIN leaf')));
});

test('fails when a row names two leaves', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  const m = matrix({ ...GOOD, sources: [['ContentView.swift', '`ui/src/views/ContentView.tsx`', 'WIN-7.1, WIN-7.2', 'ported'], GOOD.sources[1]] });
  assert.ok(run(root, m).errors.some((e) => e.includes('must name exactly one WIN leaf')));
});

test('fails when an excluded row gives no reason, or a status is unknown', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  const m = matrix({ ...GOOD, sources: [['ContentView.swift', '`x.tsx`', 'WIN-7.1', 'done'], ['Models/Thing+iOS.swift', '—', '—', 'excluded', '—']] });
  const errs = run(root, m).errors;
  assert.ok(errs.some((e) => e.includes('status "done"')));
  assert.ok(errs.some((e) => e.includes('gives no reason')));
});

test('fails when an SF Symbol in source has no Lucide icon', () => {
  const root = fixture({ swift: { ...SWIFT, 'Views/B.swift': 'let v = Label("Hi", systemImage: ok ? "checkmark.circle" : "circle")\n' }, tests: TESTS });
  const m = matrix({ ...GOOD, sources: [...GOOD.sources, ['Views/B.swift', '`ui/src/views/B.tsx`', 'WIN-7.1', 'todo']], symbols: [['xmark', 'x'], ['circle', 'circle']] });
  const errs = run(root, m).errors;
  assert.ok(errs.some((e) => e.includes('"checkmark.circle"')), errs.join('\n'));
  assert.ok(!errs.some((e) => e.includes('"circle"')));
});

test('fails when a test file is missing from the matrix', () => {
  const root = fixture({ swift: SWIFT, tests: { ...TESTS, 'OtherTests.swift': 'import Testing\n' } });
  assert.ok(run(root, matrix(GOOD)).errors.some((e) => e.includes('`OtherTests.swift` is missing')));
});

test('--no-todo fails on todo rows and passes once everything is ported or excluded', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  assert.ok(run(root, matrix(GOOD), { noTodo: true }).errors.some((e) => e.includes('--no-todo')));
  const done = matrix({
    ...GOOD,
    sources: [['ContentView.swift', '`ui/src/views/ContentView.tsx`', 'WIN-7.1', 'ported'], GOOD.sources[1]],
    tests: [['ThingTests.swift', '`ui/src/model/thing.test.ts`', 'WIN-6.1', 'ported']],
  });
  assert.deepEqual(run(root, done, { noTodo: true }).errors, []);
});

test('extractSymbols finds symbols through every supported form', () => {
  const src = `
    // Image(systemName: "commented.out")
    Image(systemName: expanded ? "chevron.down" : "chevron.right")
    NSImage(systemSymbolName: iconName, accessibilityDescription: "File") ?? NSImage(systemSymbolName: "doc", accessibilityDescription: "File")
    Label(ref.kind == "pr" ? "PR" : "x", systemImage: ref.kind == "pr" ? "arrow.triangle.pull" : "point.3.connected.trianglepath.dotted")
    let iconName = isDir ? "folder.fill" : fileIcon(for: name)
    let v = AgentVariant(id: "claude", icon: "cpu", subtitle: "Claude Code")
    var icon: String {
        switch self {
        case .a: return "gearshape.2"
        case .b: "doc.text"
        }
    }
    let i = makeSymbolIcon("desktopcomputer", description: "Machine", pointSize: 11)
    var symbol: String { "\\u{2318}" }
    Image(systemName: agent.icon ?? "terminal")
  `;
  const got = [...extractSymbols(src)].sort();
  assert.deepEqual(got, ['arrow.triangle.pull', 'chevron.down', 'chevron.right', 'cpu', 'desktopcomputer', 'doc', 'doc.text', 'folder.fill', 'gearshape.2', 'point.3.connected.trianglepath.dotted', 'terminal']);
});

test('parseTable reads rows under the matching header only', () => {
  const rows = parseTable(matrix(GOOD), ['SF Symbol', 'Lucide icon', 'Used in']);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]['Lucide icon'], '`x`');
  assert.equal(parseTable('no tables here', ['A']), null);
});

test('the CLI exits 1 on a broken matrix and 0 on a good one', () => {
  const root = fixture({ swift: SWIFT, tests: TESTS });
  fs.mkdirSync(path.join(root, path.dirname(MATRIX)), { recursive: true });
  fs.writeFileSync(path.join(root, MATRIX), matrix(GOOD));
  const ok = spawnSync(process.execPath, [SCRIPT, '--repo', root], { encoding: 'utf8' });
  assert.equal(ok.status, 0, ok.stderr);
  const noTodo = spawnSync(process.execPath, [SCRIPT, '--repo', root, '--no-todo'], { encoding: 'utf8' });
  assert.equal(noTodo.status, 1);
  fs.writeFileSync(path.join(root, APP_DIR, 'Extra.swift'), 'struct Extra {}\n');
  const bad = spawnSync(process.execPath, [SCRIPT, '--repo', root], { encoding: 'utf8' });
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /Extra\.swift/);
});

test('the real matrix passes without --no-todo', () => {
  const r = spawnSync(process.execPath, [SCRIPT], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
});
