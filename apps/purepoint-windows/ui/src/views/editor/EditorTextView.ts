// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/EditorTextView.swift (WIN-8.5): how the
// editor's text behaves. NSTextView is CodeMirror's content here, so this is the set of extensions
// that give it the Mac's behaviour: plain text with undo (`allowsUndo`), Find (`usesFindBar`,
// Ctrl+F), Ctrl+S to save (Swift's Cmd+S in `keyDown`), Tab inserting four spaces
// (`insertTab`), and no smart substitutions (CodeMirror makes none). Content changes reach
// `onContentChanged` (Swift `didChangeText`).
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search';
import { Annotation, type Extension } from '@codemirror/state';
import { drawSelection, EditorView, keymap } from '@codemirror/view';

/** Marks a transaction that loads content from the state rather than the user's typing. */
export const EXTERNAL_CONTENT = Annotation.define<boolean>();

export interface EditorTextViewCallbacks {
  onSave: () => void;
  onContentChanged: (content: string) => void;
}

/** Swift `insertTab`: four spaces at the selection. */
function insertFourSpaces(view: EditorView): boolean {
  if (view.state.readOnly) return false;
  view.dispatch(view.state.replaceSelection('    '), { scrollIntoView: true, userEvent: 'input' });
  return true;
}

export function editorTextViewExtensions(callbacks: () => EditorTextViewCallbacks): Extension {
  return [
    history(),
    drawSelection(),
    search({ top: true }),
    highlightSelectionMatches(),
    keymap.of([
      {
        key: 'Mod-s',
        preventDefault: true,
        run: () => {
          callbacks().onSave();
          return true;
        },
      },
      { key: 'Tab', run: insertFourSpaces },
      ...searchKeymap,
      ...historyKeymap,
      ...defaultKeymap,
    ]),
    EditorView.updateListener.of((update) => {
      // Content set from the state (a reload) is tagged so it is not reported back as an edit.
      if (!update.docChanged) return;
      if (update.transactions.every((tr) => tr.annotation(EXTERNAL_CONTENT) === true)) return;
      callbacks().onContentChanged(update.state.doc.toString());
    }),
  ];
}
