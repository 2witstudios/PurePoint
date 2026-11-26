// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/ToolCallCardView.swift (WIN-9.5): one
// tool call as a disclosure — its name and status, expanding to its input and output in a
// horizontally scrolling monospaced block. `PiToolDetail` (the Point Guard transcript's per-tool
// row, from PiPointGuardChatView.swift) is the same card over a bridge tool activity.
import { useState } from 'react';
import type { ToolUseStatus } from '../../model/content.ts';

export interface ToolCallCardProps {
  name: string | null;
  input: string | null;
  output: string | null;
  isError: boolean;
  status: ToolUseStatus;
  /** Called before the card expands or collapses (the transcript re-anchors its scroll). */
  onLayoutChange?: () => void;
}

export function toolStatusLabel(isError: boolean, status: ToolUseStatus): string {
  if (isError) return 'Failed';
  switch (status) {
    case 'running':
      return 'Running';
    case 'completed':
      return 'Done';
    case 'failed':
      return 'Failed';
  }
}

function Detail({ label, text, error }: { label: string; text: string; error: boolean }) {
  return (
    <div className="pg-tool-detail">
      <div className="pg-tool-detail-label">{label}</div>
      <pre className="pg-tool-output" data-error={error}>
        {text}
      </pre>
    </div>
  );
}

export function ToolCallCardView({
  name,
  input,
  output,
  isError,
  status,
  onLayoutChange,
}: ToolCallCardProps) {
  const [expanded, setExpanded] = useState(false);
  const failed = isError || status === 'failed';
  return (
    <details
      className="pg-tool-card"
      data-failed={failed}
      open={expanded}
      onToggle={(event) => {
        const open = event.currentTarget.open;
        if (open === expanded) return;
        onLayoutChange?.();
        setExpanded(open);
      }}
    >
      <summary
        className="pg-tool-summary"
        aria-description="Expand to inspect tool input and output"
      >
        <span className="pg-tool-name">{name ?? 'Result'}</span>
        <span className="pg-tool-status">{toolStatusLabel(isError, status)}</span>
      </summary>
      <div className="pg-tool-body">
        {input !== null && input !== '' && <Detail label="Input" text={input} error={false} />}
        {output !== null && output !== '' && (
          <Detail label="Output" text={output} error={isError} />
        )}
        {(input ?? '') === '' && (output ?? '') === '' && (
          <div className="pg-tool-empty">No output</div>
        )}
      </div>
    </details>
  );
}
