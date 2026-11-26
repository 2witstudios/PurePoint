// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/InlineCodeView.swift (WIN-8.3).
//
// A selectable, syntax-highlighted code body with no scroll view of its own (the workspace
// sidebar owns scrolling): source only, one row per line, with the line number and +/− marker
// drawn in a 42px gutter so they cannot confuse the highlighter, and `⋯` between hunks. Rows
// keep the diff colours; long lines wrap at the body's width.
import { useEffect, useMemo, useState } from 'react';
import { detectEditorLanguage, type EditorLanguage } from '../../model/editorLanguage.ts';
import { highlightLines, loadedLanguage, loadLanguage } from '../../services/codeHighlight.ts';
import type { Hunk } from '../../services/git.ts';
import { HighlightedCode } from './DiffContentView.tsx';
import { inlineCodeRows, inlineGutter } from './diffTextRendering.ts';
import './diff.css';

export interface InlineCodeViewProps {
  hunks: readonly Hunk[];
  /** The language; detected from `filename` when not given. */
  language?: EditorLanguage;
  filename?: string;
}

export function InlineCodeView({ hunks, language, filename = '' }: InlineCodeViewProps) {
  const resolved = language ?? detectEditorLanguage(filename);
  const [grammar, setGrammar] = useState(() => loadedLanguage(resolved));
  const [grammarFor, setGrammarFor] = useState(resolved);
  if (grammarFor !== resolved) {
    setGrammarFor(resolved);
    setGrammar(loadedLanguage(resolved));
  }
  useEffect(() => {
    let current = true;
    void loadLanguage(resolved).then((loaded) => {
      if (current) setGrammar(loaded);
    });
    return () => {
      current = false;
    };
  }, [resolved]);

  const rows = useMemo(() => inlineCodeRows(hunks), [hunks]);
  const tokens = useMemo(
    () =>
      grammar === undefined
        ? null
        : highlightLines(
            rows.map((row) => row.text),
            grammar,
          ),
    [rows, grammar],
  );

  return (
    <div className="inline-code" data-view="InlineCodeView">
      {rows.map((row, index) => {
        const gutter = inlineGutter(row.line);
        return (
          <div key={index} className={`inline-code-row ${row.line?.type ?? 'gap'}`}>
            <span className="inline-code-number">{gutter.number}</span>
            <span className={`inline-code-marker ${row.line?.type ?? ''}`}>{gutter.marker}</span>
            <span className="inline-code-text">
              {row.line === null ? (
                row.text
              ) : (
                <HighlightedCode text={row.text} tokens={tokens?.[index] ?? null} />
              )}
            </span>
          </div>
        );
      })}
    </div>
  );
}
