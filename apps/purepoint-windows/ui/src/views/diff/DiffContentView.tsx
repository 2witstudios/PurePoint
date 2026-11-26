// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DiffContentNSView.swift (WIN-8.3).
//
// The diff body's rows: right-aligned old and new line numbers, then the code, on the line's
// addition/deletion/context colours, and the centred `···` row between hunks. The Mac draws the
// whole body as one NSTextView whose height it measures per width; here each line is its own
// row so DiffListView can virtualize thousands of them, and the row heights come from
// diffLayout.ts (estimates) and the rows themselves (measured). The code wraps like the Mac's
// text view.
import { memo } from 'react';
import type { HighlightedLine } from '../../services/codeHighlight.ts';
import type { DiffLine } from '../../services/git.ts';

/** Highlighted tokens, or the plain text while the grammar loads. */
export function HighlightedCode({
  text,
  tokens,
}: {
  text: string;
  tokens: HighlightedLine | null;
}) {
  if (tokens === null) return text;
  return tokens.map((token, index) =>
    token.classes === '' ? (
      token.text
    ) : (
      <span key={index} className={token.classes}>
        {token.text}
      </span>
    ),
  );
}

export const DiffLineRow = memo(function DiffLineRow({
  line,
  tokens,
}: {
  line: DiffLine;
  tokens: HighlightedLine | null;
}) {
  return (
    <div className={`diff-line ${line.type}`}>
      <span className="diff-line-no">{line.oldLineNo ?? ''}</span>
      <span className="diff-line-no">{line.newLineNo ?? ''}</span>
      <span className="diff-code">
        <HighlightedCode text={line.content} tokens={tokens} />
      </span>
    </div>
  );
});

export function DiffSeparatorRow() {
  return (
    <div className="diff-separator" aria-hidden="true">
      ···
    </div>
  );
}
