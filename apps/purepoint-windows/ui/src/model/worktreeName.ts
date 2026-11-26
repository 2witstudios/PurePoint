// Port of apps/purepoint-macos/purepoint-macos/Utilities/WorktreeNameNormalizer.swift (WIN-6.0).

const GRAPHEMES = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

/**
 * Normalizes a user-provided name into a valid worktree branch slug. Lowercases, keeps ASCII
 * alphanumerics, converts whitespace and underscores to hyphens, collapses consecutive hyphens,
 * and trims leading and trailing hyphens.
 */
export function normalizeWorktreeName(input: string): string {
  let result = '';
  // Swift walks Characters (grapheme clusters): `e` + a combining accent is one non-ASCII
  // character and is dropped whole, and CR LF is one whitespace character.
  for (const { segment } of GRAPHEMES.segment(input.toLowerCase())) {
    if (/^[a-z0-9]$/.test(segment)) result += segment;
    else if (segment === '_' || /^\s+$/u.test(segment)) result += '-';
  }
  return result.replace(/-{2,}/g, '-').replace(/^-+|-+$/g, '');
}
