// Port of apps/purepoint-macos/purepoint-macos/Models/PRModel.swift (WIN-6.0).
//
// `PullRequestInfo` is what the git backend returns (`services/git.ts`, decoded from `gh pr list
// --json` in Rust); this names its parts and the `Identifiable` ids Swift derives.

import type { PullRequestInfo } from '../services/git';

export type { PullRequestInfo };
export type PRAuthor = PullRequestInfo['author'];
export type PRLabel = PullRequestInfo['labels'][number];

/** Swift `PullRequestInfo.id`. */
export function pullRequestId(pr: PullRequestInfo): number {
  return pr.number;
}

/** Swift `PRLabel.id`. */
export function prLabelId(label: PRLabel): string {
  return label.name;
}
