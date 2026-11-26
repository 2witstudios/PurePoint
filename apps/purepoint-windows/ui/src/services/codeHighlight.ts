// Syntax highlighting for the diff and inline code bodies (WIN-8.3), built to be reused by the
// editor (WIN-8.5).
//
// Choice: CodeMirror 6's highlighters (Lezer grammars from `@codemirror/lang-*`, the
// `@codemirror/legacy-modes` stream parsers for the rest), not Shiki. The editor will be
// CodeMirror, so diff, inline code and editor share one grammar per language and one theme;
// Lezer parses synchronously in a few milliseconds per 200-line review page with no WASM, so a
// page highlights inside the long-task budget; and each language loads on first use as its own
// chunk.
//
// Reuse in the editor: `loadLanguage(language)` resolves the CodeMirror `Language` to pass to
// `new LanguageSupport(...)`, and `PP_HIGHLIGHTER` goes to `syntaxHighlighting(PP_HIGHLIGHTER)`.
// Both emit the `pp-tok-*` classes styled in `views/diff/highlight.css` from the EditorTheme
// tokens (`--pp-editor-keyword`, …), so every surface colours code the same way.
//
// Swift: Services/SyntaxHighlightManager.swift (tree-sitter) is WIN-8.5's row; the categories
// below are EditorTheme's (keyword, string, comment, number, type, function, property,
// preprocessor).
import type { Language, StreamParser } from '@codemirror/language';
import { highlightTree, tagHighlighter, tags as t, type Highlighter } from '@lezer/highlight';
import type { EditorLanguage } from '../model/editorLanguage';

/** The highlighter: Lezer tags to EditorTheme's categories. */
export const PP_HIGHLIGHTER: Highlighter = tagHighlighter([
  {
    tag: [
      t.keyword,
      t.controlKeyword,
      t.definitionKeyword,
      t.moduleKeyword,
      t.operatorKeyword,
      t.modifier,
      t.bool,
      t.null,
      t.self,
      t.heading,
    ],
    class: 'pp-tok-keyword',
  },
  {
    tag: [t.string, t.special(t.string), t.regexp, t.character, t.escape, t.docString],
    class: 'pp-tok-string',
  },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], class: 'pp-tok-comment' },
  { tag: [t.number, t.integer, t.float, t.atom, t.unit], class: 'pp-tok-number' },
  {
    tag: [t.typeName, t.className, t.namespace, t.standard(t.typeName), t.tagName],
    class: 'pp-tok-type',
  },
  {
    tag: [
      t.function(t.variableName),
      t.function(t.propertyName),
      t.function(t.definition(t.variableName)),
      t.macroName,
      t.link,
      t.url,
    ],
    class: 'pp-tok-function',
  },
  { tag: [t.propertyName, t.attributeName, t.labelName], class: 'pp-tok-property' },
  {
    tag: [t.meta, t.processingInstruction, t.annotation, t.documentMeta],
    class: 'pp-tok-preprocessor',
  },
  { tag: t.emphasis, class: 'pp-tok-emphasis' },
  { tag: t.strong, class: 'pp-tok-strong' },
]);

type Loader = () => Promise<Language>;

async function legacy<State>(load: () => Promise<StreamParser<State>>): Promise<Language> {
  const [{ StreamLanguage }, mode] = await Promise.all([import('@codemirror/language'), load()]);
  return StreamLanguage.define(mode);
}

const LOADERS: Readonly<Record<EditorLanguage, Loader | null>> = {
  javascript: () => import('@codemirror/lang-javascript').then((m) => m.javascriptLanguage),
  typescript: () => import('@codemirror/lang-javascript').then((m) => m.typescriptLanguage),
  tsx: () => import('@codemirror/lang-javascript').then((m) => m.tsxLanguage),
  rust: () => import('@codemirror/lang-rust').then((m) => m.rustLanguage),
  python: () => import('@codemirror/lang-python').then((m) => m.pythonLanguage),
  markdown: () => import('@codemirror/lang-markdown').then((m) => m.markdownLanguage),
  json: () => import('@codemirror/lang-json').then((m) => m.jsonLanguage),
  yaml: () => import('@codemirror/lang-yaml').then((m) => m.yamlLanguage),
  html: () => import('@codemirror/lang-html').then((m) => m.htmlLanguage),
  css: () => import('@codemirror/lang-css').then((m) => m.cssLanguage),
  c: () => import('@codemirror/lang-cpp').then((m) => m.cppLanguage),
  cpp: () => import('@codemirror/lang-cpp').then((m) => m.cppLanguage),
  go: () => import('@codemirror/lang-go').then((m) => m.goLanguage),
  php: () => import('@codemirror/lang-php').then((m) => m.phpLanguage),
  java: () => import('@codemirror/lang-java').then((m) => m.javaLanguage),
  xml: () => import('@codemirror/lang-xml').then((m) => m.xmlLanguage),
  swift: () => legacy(() => import('@codemirror/legacy-modes/mode/swift').then((m) => m.swift)),
  toml: () => legacy(() => import('@codemirror/legacy-modes/mode/toml').then((m) => m.toml)),
  shell: () => legacy(() => import('@codemirror/legacy-modes/mode/shell').then((m) => m.shell)),
  ruby: () => legacy(() => import('@codemirror/legacy-modes/mode/ruby').then((m) => m.ruby)),
  csharp: () => legacy(() => import('@codemirror/legacy-modes/mode/clike').then((m) => m.csharp)),
  scala: () => legacy(() => import('@codemirror/legacy-modes/mode/clike').then((m) => m.scala)),
  haskell: () =>
    legacy(() => import('@codemirror/legacy-modes/mode/haskell').then((m) => m.haskell)),
  lua: () => legacy(() => import('@codemirror/legacy-modes/mode/lua').then((m) => m.lua)),
  plaintext: null,
};

const loaded = new Map<EditorLanguage, Language | null>();
const loading = new Map<EditorLanguage, Promise<Language | null>>();

/** The CodeMirror language for `language` (null for plain text), loading it on first use. */
export function loadLanguage(language: EditorLanguage): Promise<Language | null> {
  const ready = loaded.get(language);
  if (ready !== undefined) return Promise.resolve(ready);
  let pending = loading.get(language);
  if (pending === undefined) {
    const loader = LOADERS[language];
    pending = (loader === null ? Promise.resolve(null) : loader())
      .catch((error: unknown) => {
        console.warn(`PurePoint: could not load the ${language} grammar`, error);
        return null;
      })
      .then((result) => {
        loaded.set(language, result);
        loading.delete(language);
        return result;
      });
    loading.set(language, pending);
  }
  return pending;
}

/** The language if it has loaded: undefined while it has not (call `loadLanguage`). */
export function loadedLanguage(language: EditorLanguage): Language | null | undefined {
  return loaded.get(language);
}

/** A run of one line: its text and the `pp-tok-*` classes (empty for plain text). */
export interface HighlightToken {
  readonly text: string;
  readonly classes: string;
}

export type HighlightedLine = readonly HighlightToken[];

function plain(lines: readonly string[]): HighlightedLine[] {
  return lines.map((line) => (line === '' ? [] : [{ text: line, classes: '' }]));
}

/**
 * Highlight consecutive source lines as one document (so strings and comments that span lines
 * colour correctly) and split the result back into lines. Plain text when `language` is null.
 */
export function highlightLines(
  lines: readonly string[],
  language: Language | null,
  highlighter: Highlighter = PP_HIGHLIGHTER,
): HighlightedLine[] {
  if (language === null || lines.length === 0) return plain(lines);
  const text = lines.join('\n');
  const starts: number[] = [];
  let offset = 0;
  for (const line of lines) {
    starts.push(offset);
    offset += line.length + 1;
  }
  const result: HighlightToken[][] = lines.map(() => []);
  // Styled ranges arrive in document order and never overlap.
  let lineIndex = 0;
  let position = 0;
  const emit = (from: number, to: number, classes: string) => {
    while (from < to) {
      while (lineIndex + 1 < lines.length && from >= (starts[lineIndex + 1] ?? Infinity)) {
        lineIndex += 1;
      }
      const lineStart = starts[lineIndex] ?? 0;
      const lineEnd = lineStart + (lines[lineIndex]?.length ?? 0);
      const end = Math.min(to, lineEnd);
      if (end > from) {
        result[lineIndex]?.push({ text: text.slice(from, end), classes });
      }
      // Skip the newline between lines.
      from = end === lineEnd ? lineEnd + 1 : end;
    }
  };
  let tree;
  try {
    tree = language.parser.parse(text);
  } catch {
    return plain(lines);
  }
  highlightTree(tree, highlighter, (from, to, classes) => {
    if (from > position) emit(position, from, '');
    emit(from, to, classes);
    position = to;
  });
  if (position < text.length) emit(position, text.length, '');
  return result;
}
