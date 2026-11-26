// Port of apps/purepoint-macos/purepoint-macos/Models/FileNode.swift (WIN-8.4): a node of the file
// tree. Swift's `FileTreeNode` is a reference type the outline view mutates in place (an expanded
// folder's `children` filled on expansion); here a node is an immutable value and FileTreeState
// publishes a new tree on every refresh, which is what React compares. Swift's unused value-type
// `FileNode` (relative-path `id`, optional `children`) has no counterpart.

export interface FileNode {
  readonly name: string;
  readonly absolutePath: string;
  /** From the tree's root, `/`-separated on every host (Swift `relativePath`). */
  readonly relativePath: string;
  readonly isDirectory: boolean;
  /** An expanded folder's children; empty for files and collapsed folders. */
  readonly children: readonly FileNode[];
}

/** Swift `isExpandable`. */
export function isExpandable(node: FileNode): boolean {
  return node.isDirectory;
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base' });

/** Swift `FileTreeNode.sorted`: folders first, then `localizedCaseInsensitiveCompare` by name. */
export function sortedFileNodes(nodes: readonly FileNode[]): FileNode[] {
  return [...nodes].sort((a, b) => {
    if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
    return collator.compare(a.name, b.name);
  });
}

function lastSeparator(path: string): number {
  return Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
}

function trimSeparators(path: string): string {
  return path.replace(/[\\/]+$/, '');
}

/** The last path component, for `\` and `/` paths. */
export function pathName(path: string): string {
  const trimmed = trimSeparators(path);
  const index = lastSeparator(trimmed);
  return index < 0 ? trimmed : trimmed.slice(index + 1);
}

/** The parent directory of a `\` or `/` path (Swift `deletingLastPathComponent`). */
export function parentPath(path: string): string {
  const trimmed = trimSeparators(path);
  const index = lastSeparator(trimmed);
  if (index < 0) return '';
  // Keep the separator of a drive or filesystem root (`C:\`, `/`).
  const parent = trimmed.slice(0, index);
  return parent === '' || /^[A-Za-z]:$/.test(parent) ? trimmed.slice(0, index + 1) : parent;
}

/** `path`'s separator: `/` for a POSIX path (a remote machine), else `\`. */
export function pathSeparator(path: string): '/' | '\\' {
  return path.includes('\\') || /^[A-Za-z]:/.test(path) ? '\\' : '/';
}

/** `child` under `directory`, with the directory's separator. */
export function joinPath(directory: string, child: string): string {
  const separator = pathSeparator(directory);
  return directory.endsWith(separator) ? directory + child : directory + separator + child;
}

/** `path` relative to `root`, `/`-separated; null when it is not under `root`. */
export function relativePath(root: string, path: string): string | null {
  const normalize = (value: string) => trimSeparators(value.replace(/\\/g, '/'));
  const base = normalize(root);
  const full = normalize(path);
  // Windows paths compare case-insensitively; a remote POSIX machine's do not.
  const caseless = pathSeparator(root) === '\\';
  const a = caseless ? base.toLowerCase() : base;
  const b = caseless ? full.toLowerCase() : full;
  if (b === a) return '';
  return b.startsWith(`${a}/`) ? full.slice(base.length + 1) : null;
}

/** Whether two paths name the same file (case-insensitively on Windows, either separator). */
export function samePath(a: string, b: string): boolean {
  return relativePath(a, b) === '';
}
