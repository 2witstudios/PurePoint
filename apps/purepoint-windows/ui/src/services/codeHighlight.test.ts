// WIN-8.3: the shared highlighter (CodeMirror grammars, EditorTheme classes).
import { describe, expect, it } from 'vitest';
import { highlightLines, loadLanguage, loadedLanguage } from './codeHighlight';
import { detectEditorLanguage } from '../model/editorLanguage';

const textOf = (line: readonly { text: string }[]) => line.map((token) => token.text).join('');

describe('codeHighlight', () => {
  it('splits a highlighted document back into its lines', async () => {
    const language = await loadLanguage('typescript');
    expect(language).not.toBeNull();
    const lines = ['const a = "x";', '', '/* one', 'two */ let b = 2;'];
    const result = highlightLines(lines, language);
    expect(result.map(textOf)).toEqual(lines);
    expect(result[0]?.find((token) => token.text === 'const')?.classes).toBe('pp-tok-keyword');
    expect(result[0]?.find((token) => token.text === '"x"')?.classes).toBe('pp-tok-string');
    // A block comment that spans lines colours both halves.
    expect(result[2]?.[0]).toEqual({ text: '/* one', classes: 'pp-tok-comment' });
    expect(result[3]?.[0]).toEqual({ text: 'two */', classes: 'pp-tok-comment' });
    expect(result[3]?.find((token) => token.text === '2')?.classes).toBe('pp-tok-number');
  });

  it('loads stream-mode grammars and leaves plain text alone', async () => {
    const swift = await loadLanguage('swift');
    expect(loadedLanguage('swift')).toBe(swift);
    const [line] = highlightLines(['let x = 1 // note'], swift);
    expect(textOf(line ?? [])).toBe('let x = 1 // note');
    expect(line?.some((token) => token.classes === 'pp-tok-comment')).toBe(true);
    expect(await loadLanguage('plaintext')).toBeNull();
    expect(highlightLines(['a', ''], null)).toEqual([[{ text: 'a', classes: '' }], []]);
  });
});

describe('detectEditorLanguage', () => {
  it('matches EditorLanguage.detect(from:)', () => {
    expect(detectEditorLanguage('src/App.tsx')).toBe('tsx');
    expect(detectEditorLanguage('Cargo.lock')).toBe('toml');
    expect(detectEditorLanguage('dir\\Makefile')).toBe('shell');
    expect(detectEditorLanguage('README.MD')).toBe('markdown');
    expect(detectEditorLanguage('.env')).toBe('plaintext');
    expect(detectEditorLanguage('schema.graphql')).toBe('plaintext');
    expect(detectEditorLanguage('main.rs')).toBe('rust');
  });
});
