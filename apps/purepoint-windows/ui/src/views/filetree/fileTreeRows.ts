// The outline's visible rows (WIN-8.4): NSOutlineView's flattening of the expanded tree, which the
// virtualized list draws one row per entry. A search query (FileTreeSidebarView's "Search
// files...") keeps the rows whose name matches and the folders leading to them, among the folders
// listed so far; the Mac binds the field to `FileTreeState.searchQuery`.
import type { FileNode } from '../../model/fileNode';

export interface FileTreeRow {
  readonly node: FileNode;
  readonly depth: number;
  readonly expanded: boolean;
}

/** Outline row height and indentation (Swift `heightOfRowByItem`, `indentationPerLevel`). */
export const FILE_TREE_ROW_HEIGHT = 24;
export const FILE_TREE_INDENT = 12;

function matches(node: FileNode, query: string): boolean {
  if (node.name.toLowerCase().includes(query)) return true;
  return node.children.some((child) => matches(child, query));
}

export function fileTreeRows(
  nodes: readonly FileNode[],
  expandedPaths: ReadonlySet<string>,
  searchQuery = '',
): FileTreeRow[] {
  const query = searchQuery.trim().toLowerCase();
  const rows: FileTreeRow[] = [];
  const visit = (level: readonly FileNode[], depth: number) => {
    for (const node of level) {
      if (query !== '' && !matches(node, query)) continue;
      const expanded = node.isDirectory && expandedPaths.has(node.absolutePath);
      rows.push({ node, depth, expanded });
      if (expanded) visit(node.children, depth + 1);
    }
  };
  visit(nodes, 0);
  return rows;
}
