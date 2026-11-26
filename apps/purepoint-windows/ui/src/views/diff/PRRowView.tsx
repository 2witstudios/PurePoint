// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/PRRowView.swift (WIN-8.3).
//
// A pull request in the review's PR list: `#number title`, draft badge and review state, then
// the author, head branch and +/−/files, then the labels in their GitHub colours.
import type { PullRequestInfo } from '../../services/git.ts';
import { Icon } from '../../theme/Icon.tsx';

/** Swift `labelColor(_:)`: a GitHub label's six-digit hex, gray when it is not one. */
export function labelColor(hex: string): string {
  const clean = hex.replace(/^#+|#+$/g, '');
  return /^[0-9a-fA-F]{6}$/.test(clean) ? `#${clean.toLowerCase()}` : 'gray';
}

function ReviewIndicator({ decision }: { decision: string | null | undefined }) {
  switch (decision) {
    case 'APPROVED':
      return (
        <Icon
          name="checkmark.circle.fill"
          size={12}
          className="pr-review approved"
          label="Approved"
        />
      );
    case 'CHANGES_REQUESTED':
      return (
        <Icon
          name="xmark.circle.fill"
          size={12}
          className="pr-review changes"
          label="Changes requested"
        />
      );
    case 'REVIEW_REQUIRED':
      return (
        <Icon name="circle" size={12} className="pr-review required" label="Review required" />
      );
    default:
      return null;
  }
}

export function PRRowView({
  pr,
  isSelected = false,
}: {
  pr: PullRequestInfo;
  isSelected?: boolean;
}) {
  return (
    <span className={`pr-row${isSelected ? ' selected' : ''}`}>
      <span className="pr-row-line">
        <span className="pr-number">{`#${String(pr.number)}`}</span>
        <span className="pr-title">{pr.title}</span>
        <span className="detail-spacer" />
        {pr.isDraft && <span className="pr-draft">Draft</span>}
        <ReviewIndicator decision={pr.reviewDecision} />
      </span>
      <span className="pr-row-line pr-meta">
        <span className="pr-author">
          <Icon name="person" size={11} />
          {pr.author.login}
        </span>
        <span className="pr-branch">
          <Icon name="arrow.triangle.branch" size={11} />
          {pr.headRefName}
        </span>
        <span className="detail-spacer" />
        {pr.additions > 0 && <span className="diff-card-added">{`+${String(pr.additions)}`}</span>}
        {pr.deletions > 0 && (
          <span className="diff-card-removed">{`-${String(pr.deletions)}`}</span>
        )}
        <span className="pr-files">{`${String(pr.changedFiles)} files`}</span>
      </span>
      {pr.labels.length > 0 && (
        <span className="pr-row-line pr-labels">
          {pr.labels.map((label) => (
            <span
              key={label.name}
              className="pr-label"
              style={{ background: labelColor(label.color) }}
            >
              {label.name}
            </span>
          ))}
        </span>
      )}
    </span>
  );
}
