// Front end of the git backend (WIN-8.1): `src-tauri/src/git/`, ported from
// Services/GitService.swift and Services/ProjectHost.swift. Every call runs git or gh in the
// backend; `machineId` selects a remote machine's host (wired by WIN-5.3; until then remote
// calls fail with "This project's machine is not available.").
import { invoke } from '@tauri-apps/api/core';

export type LineType = 'context' | 'addition' | 'deletion';

export interface DiffLine {
  type: LineType;
  /** The code without its `+`/`-`/` ` prefix. */
  content: string;
  oldLineNo: number | null;
  newLineNo: number | null;
}

export interface Hunk {
  /** e.g. `@@ -10,6 +10,8 @@ func login()`. */
  header: string;
  lines: DiffLine[];
}

/** A range of a patch snapshot the backend created; pages are read from it. */
export interface PatchSlice {
  snapshot: string;
  start: number;
  end: number;
}

export interface FileDiff {
  filename: string;
  /** `M`, `A`, `D`, `R`, `U` or `??`. */
  statusCode: string;
  added: number;
  removed: number;
  hunks: Hunk[];
  oldFilename: string | null;
  isBinary: boolean;
  previewError: string | null;
  previewDeferred: boolean;
  /** Conflict working-file patches compare against stage 2, or empty if ours deleted the path. */
  conflictBaseline: string | null;
  /** Set on overview (summaries-only) entries; the backend runs it for the first page. */
  patchArguments: string[] | null;
  patchSlice: PatchSlice | null;
  previewVersion: string;
}

export interface DiffData {
  files: FileDiff[];
}

export interface ReviewPage {
  slice: PatchSlice;
  hunks: Hunk[];
  nextOffset: number;
  oldLine: number;
  newLine: number;
  header: string;
  hasMore: boolean;
  isBinary: boolean;
  hasLongLines: boolean;
}

/** Lightweight resume state for the next page; it never retains hunk bodies. */
export interface ReviewPageCursor {
  slice: PatchSlice;
  nextOffset: number;
  oldLine: number;
  newLine: number;
  header: string;
  isBinary: boolean;
}

export interface GitCommitInfo {
  sha: string;
  subject: string;
  author: string;
  date: string;
}

export interface GitReviewSummary {
  commitCount: number;
  localFileCount: number;
  error: string | null;
}

export interface GitBranchReview {
  comparisonBase: string;
  availableBases: string[];
  files: FileDiff[];
  commits: GitCommitInfo[];
  error: string | null;
}

export interface GitLocalReview {
  staged: FileDiff[];
  unstaged: FileDiff[];
  untracked: FileDiff[];
  error: string | null;
  stagedError: string | null;
  unstagedError: string | null;
  untrackedError: string | null;
}

export interface PullRequestInfo {
  number: number;
  title: string;
  url: string;
  state: string;
  headRefName: string;
  baseRefName: string;
  author: { login: string; name?: string | null };
  labels: { name: string; color: string }[];
  reviewDecision?: string | null;
  additions: number;
  deletions: number;
  changedFiles: number;
  isDraft: boolean;
  createdAt: string;
  updatedAt: string;
}

export type GitErrorKind =
  'review' | 'workingTree' | 'tooLarge' | 'unsupported' | 'cancelled' | 'host' | 'io';

/** What a failed git command rejects with; `message` is what the Mac shows. */
export interface GitCommandError {
  kind: GitErrorKind;
  message: string;
}

export function isGitCommandError(value: unknown): value is GitCommandError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as GitCommandError).kind === 'string' &&
    typeof (value as GitCommandError).message === 'string'
  );
}

/** The message of any rejection from these calls. */
export function gitErrorMessage(error: unknown): string {
  if (isGitCommandError(error)) return error.message;
  return error instanceof Error ? error.message : String(error);
}

const machine = (machineId?: string | null) => ({ machineId: machineId ?? null });

/** Unique paths across the staged, unstaged and untracked groups. */
export function uniquePathCount(review: GitLocalReview): number {
  const all = [...review.staged, ...review.unstaged, ...review.untracked];
  return new Set(all.map((file) => file.filename)).size;
}

/** Large and untracked files start collapsed. */
export function shouldCollapse(file: FileDiff): boolean {
  return file.added + file.removed > 200 || file.statusCode === '??';
}

/** The cursor that loads the page after `page`. */
export function continuation(page: ReviewPage): ReviewPageCursor {
  const { slice, nextOffset, oldLine, newLine, header, isBinary } = page;
  return { slice, nextOffset, oldLine, newLine, header, isBinary };
}

/** The branch's patch since its merge base with `baseBranch` (or the inferred base). */
export function fetchBranchReview(
  root: string,
  baseBranch: string | null,
  summariesOnly = true,
  machineId?: string | null,
): Promise<GitBranchReview> {
  return invoke<GitBranchReview>('git_branch_review', {
    root,
    baseBranch,
    summariesOnly,
    ...machine(machineId),
  });
}

/** Staged, unstaged and untracked changes; each group reports its own error. */
export function fetchLocalReview(
  root: string,
  summariesOnly = true,
  machineId?: string | null,
): Promise<GitLocalReview> {
  return invoke<GitLocalReview>('git_local_review', { root, summariesOnly, ...machine(machineId) });
}

/** Commit count against the base and number of locally changed paths. */
export function reviewSummary(
  root: string,
  baseBranch: string | null,
  machineId?: string | null,
): Promise<GitReviewSummary> {
  return invoke<GitReviewSummary>('git_review_summary', {
    root,
    baseBranch,
    ...machine(machineId),
  });
}

/** One commit's patch (first parent for merges). */
export function fetchCommitDiff(
  root: string,
  sha: string,
  summariesOnly = true,
  machineId?: string | null,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>('git_commit_diff', { root, sha, summariesOnly, ...machine(machineId) });
}

/** Explicitly loads a deferred untracked file's preview. */
export function fetchUntrackedPreview(
  root: string,
  name: string,
  machineId?: string | null,
): Promise<FileDiff> {
  return invoke<FileDiff>('git_untracked_preview', { root, name, ...machine(machineId) });
}

/** One bounded page of a file's patch; pass `continuation(previous)` for the next one. */
export function fetchReviewPage(
  root: string,
  file: FileDiff,
  cursor: ReviewPageCursor | null = null,
  machineId?: string | null,
): Promise<ReviewPage> {
  return invoke<ReviewPage>('git_review_page', { root, file, cursor, ...machine(machineId) });
}

/** The editor's unstaged diff: unstaged and untracked files with full bodies. */
export function fetchUnstagedDiff(root: string, machineId?: string | null): Promise<DiffData> {
  return invoke<DiffData>('git_unstaged_diff', { root, ...machine(machineId) });
}

/** Every path that differs from HEAD (working-tree status), with counts. */
export function fetchWorkingTreeChanges(
  root: string,
  machineId?: string | null,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>('git_working_tree_changes', { root, ...machine(machineId) });
}

/** One working-tree file against HEAD; rejects with `tooLarge` past 1 MB. */
export function fetchWorkingTreeFileDiff(
  root: string,
  file: FileDiff,
  machineId?: string | null,
): Promise<FileDiff> {
  return invoke<FileDiff>('git_working_tree_file_diff', { root, file, ...machine(machineId) });
}

/** Open PRs through gh; rejects with gh's message (`gh not found`, sign-in hints, …). */
export function fetchPRList(
  cwd: string,
  branch: string | null,
  machineId?: string | null,
): Promise<PullRequestInfo[]> {
  return invoke<PullRequestInfo[]>('git_pr_list', { cwd, branch, ...machine(machineId) });
}

/** A PR's patch; `summariesOnly` defers bodies to `fetchReviewPage`. */
export function fetchPRDiff(
  cwd: string,
  prNumber: number,
  summariesOnly = true,
  machineId?: string | null,
): Promise<DiffData> {
  return invoke<DiffData>('git_pr_diff', { cwd, prNumber, summariesOnly, ...machine(machineId) });
}

/** Whether gh is installed and signed in (`gh auth status`). */
export function isGhAvailable(cwd: string, machineId?: string | null): Promise<boolean> {
  return invoke<boolean>('git_gh_available', { cwd, ...machine(machineId) });
}

/**
 * The PR tab's states, as DiffState derives them on the Mac: a loaded list, or gh
 * unavailable (missing, signed out, offline) with the error the tab shows and retries.
 */
export type PullRequestListState =
  | { ghAvailable: true; pullRequests: PullRequestInfo[]; error: null }
  | { ghAvailable: false; pullRequests: []; error: string };

export async function loadPullRequests(
  cwd: string,
  branch: string | null,
  machineId?: string | null,
): Promise<PullRequestListState> {
  try {
    const pullRequests = await fetchPRList(cwd, branch, machineId);
    return { ghAvailable: true, pullRequests, error: null };
  } catch (error) {
    return { ghAvailable: false, pullRequests: [], error: gitErrorMessage(error) };
  }
}
