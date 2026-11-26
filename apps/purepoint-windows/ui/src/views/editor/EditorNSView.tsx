// Port of apps/purepoint-macos/purepoint-macos/Views/Editor/EditorNSView.swift (WIN-8.5): the editor
// itself and its SwiftUI bridge (`EditorContentRepresentable`). A CodeMirror 6 view with the
// line-number gutter (LineNumberRulerView), the text behaviour of EditorTextView and the language's
// highlighting (syntaxHighlight.ts), in the editor font and colours of EditorTheme.
//
// As in Swift's `updateNSView`, new `content` replaces the text only when it differs from what the
// view holds (a reload), so typing never resets the cursor; a replacement is not reported back as
// an edit.
import { Compartment, EditorState as CodeMirrorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { useEffect, useRef } from 'react';
import type { EditorLanguage } from '../../model/editorLanguage';
import { editorHighlighting, editorLanguageExtension } from '../../services/syntaxHighlight';
import { EXTERNAL_CONTENT, editorTextViewExtensions } from './EditorTextView';
import { lineNumberRuler } from './LineNumberRulerView';
import '../diff/highlight.css';
import './editor.css';

export interface EditorContentProps {
  content: string;
  language: EditorLanguage;
  isEditable: boolean;
  onContentChanged: (content: string) => void;
  onSave: () => void;
  /** Focus the text when it mounts (a file just opened in the focused pane). */
  autoFocus?: boolean;
}

const editorTheme = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--pp-editor-background)',
    color: 'var(--pp-app-text)',
  },
  '.cm-scroller': { font: 'var(--pp-editor-font)', lineHeight: '1.45' },
  '.cm-content': { padding: '6px 0', caretColor: 'var(--pp-app-text)' },
  '&.cm-focused': { outline: 'none' },
});

export function EditorNSView({
  content,
  language,
  isEditable,
  onContentChanged,
  onSave,
  autoFocus = false,
}: EditorContentProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const compartments = useRef({ language: new Compartment(), editable: new Compartment() });
  const callbacks = useRef({ onContentChanged, onSave });
  useEffect(() => {
    callbacks.current = { onContentChanged, onSave };
  });

  // One view per mount; content, language and editability are updated in place below.
  useEffect(() => {
    const parent = host.current;
    if (parent === null) return;
    const { language: languageSlot, editable: editableSlot } = compartments.current;
    const created = new EditorView({
      parent,
      state: CodeMirrorState.create({
        doc: content,
        extensions: [
          lineNumberRuler(),
          editorTextViewExtensions(() => callbacks.current),
          editorHighlighting,
          editorTheme,
          languageSlot.of([]),
          editableSlot.of([
            EditorView.editable.of(isEditable),
            CodeMirrorState.readOnly.of(!isEditable),
          ]),
        ],
      }),
    });
    view.current = created;
    if (autoFocus) created.focus();
    return () => {
      created.destroy();
      view.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- created once; updated below
  }, []);

  useEffect(() => {
    const current = view.current;
    if (current === null || current.state.doc.toString() === content) return;
    current.dispatch({
      changes: { from: 0, to: current.state.doc.length, insert: content },
      annotations: EXTERNAL_CONTENT.of(true),
    });
  }, [content]);

  useEffect(() => {
    view.current?.dispatch({
      effects: compartments.current.editable.reconfigure([
        EditorView.editable.of(isEditable),
        CodeMirrorState.readOnly.of(!isEditable),
      ]),
    });
  }, [isEditable]);

  useEffect(() => {
    let cancelled = false;
    void editorLanguageExtension(language).then((extension) => {
      if (cancelled) return;
      view.current?.dispatch({ effects: compartments.current.language.reconfigure(extension) });
    });
    return () => {
      cancelled = true;
    };
  }, [language]);

  return (
    <div
      ref={host}
      className="editor-view"
      data-testid="editor"
      data-language={language}
      data-editable={isEditable}
    />
  );
}
