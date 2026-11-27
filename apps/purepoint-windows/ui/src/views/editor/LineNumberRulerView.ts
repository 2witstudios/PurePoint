// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/LineNumberRulerView.swift (WIN-8.5): the
// line-number gutter. Swift draws an NSRulerView beside the text, right-aligned numbers in the
// editor font at EditorTheme's `lineNumberColor`, the current line's number emphasized, the
// gutter widening with the line count; CodeMirror's `lineNumbers` gutter does the same, themed
// from the same tokens (`--pp-editor-line-number-color`, `--pp-editor-current-line-highlight`).
import type { Extension } from '@codemirror/state';
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  lineNumbers,
} from '@codemirror/view';

const rulerTheme = EditorView.theme({
  '.cm-gutters': {
    backgroundColor: 'var(--pp-editor-background)',
    color: 'var(--pp-editor-line-number-color)',
    border: 'none',
    borderRight: '1px solid var(--editor-ruler-divider)',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    padding: '0 8px 0 10px',
    minWidth: '28px',
    textAlign: 'right',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent',
    color: 'var(--pp-app-text)',
  },
  '.cm-activeLine': {
    backgroundColor: 'var(--pp-editor-current-line-highlight)',
  },
});

export function lineNumberRuler(): Extension {
  return [lineNumbers(), highlightActiveLineGutter(), highlightActiveLine(), rulerTheme];
}
