// The body of an expanded change in the workspace sidebar: one row per line with its number and
// +/− marker. WIN-8.3 replaces it with the syntax-highlighted InlineCodeView.
import type { Hunk } from '../../services/git.ts';

export function InlineDiffBody({ hunks }: { hunks: readonly Hunk[]; filename: string }) {
  return (
    <pre className="inline-diff-body">
      {hunks.map((hunk, index) => (
        <span key={index}>
          {index > 0 && '⋯\n'}
          {hunk.lines.map((line, i) => (
            <span key={i} className={`inline-diff-line ${line.type}`}>
              {`${String(line.newLineNo ?? line.oldLineNo ?? '')} ${
                line.type === 'addition' ? '+' : line.type === 'deletion' ? '−' : ' '
              } ${line.content}\n`}
            </span>
          ))}
        </span>
      ))}
    </pre>
  );
}
