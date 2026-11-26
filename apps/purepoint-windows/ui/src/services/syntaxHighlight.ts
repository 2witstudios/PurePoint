// Port of apps/purepoint-macos/purepoint-macos/Services/SyntaxHighlightManager.swift (WIN-8.5):
// the editor's highlighting. Swift runs tree-sitter grammars over the text view and colours the
// captures with EditorTheme; here the editor is CodeMirror 6, so a language is the CodeMirror
// grammar `codeHighlight.ts` loads (the same one the diff and inline code views use) and the
// colours are the `pp-tok-*` classes of `views/diff/highlight.css` (EditorTheme's categories).
// CodeMirror reparses incrementally on each edit, which is what Swift's debounced re-highlight
// does by hand.
import { syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import type { EditorLanguage } from '../model/editorLanguage';
import { loadLanguage, PP_HIGHLIGHTER } from './codeHighlight';

/**
 * The languages Swift's `languageConfiguration(for:)` has a tree-sitter grammar for (C and Java
 * return nil there; plain text has none). Every one has a CodeMirror grammar here, and C and Java
 * are highlighted too.
 */
export const SYNTAX_HIGHLIGHT_LANGUAGES: readonly EditorLanguage[] = [
  'swift',
  'rust',
  'javascript',
  'typescript',
  'tsx',
  'python',
  'json',
  'shell',
  'html',
  'css',
  'yaml',
  'toml',
  'markdown',
  'cpp',
  'go',
  'ruby',
  'php',
  'csharp',
  'scala',
  'haskell',
  'lua',
  'xml',
];

/** The highlighter, for the editor's extensions. */
export const editorHighlighting: Extension = syntaxHighlighting(PP_HIGHLIGHTER);

/** The editor extension for `language`: its grammar, or nothing for plain text. */
export async function editorLanguageExtension(language: EditorLanguage): Promise<Extension> {
  const grammar = await loadLanguage(language);
  return grammar?.extension ?? [];
}
