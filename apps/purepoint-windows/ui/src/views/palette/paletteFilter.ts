// The palette's filtering and row icons, from `CommandPaletteViewController.filterItems` and
// `CommandPaletteItem.icon` in apps/purepoint-macos/purepoint-macos/Views/CommandPalette/
// CommandPalettePanel.swift and Models/CommandPaletteItem.swift (WIN-7.5).
import {
  type CommandPaletteItem,
  displayName,
  isQueryOnly,
  itemIcon,
  searchableText,
} from '../../model/commandPalette.ts';
import { type EditorLanguage, detectEditorLanguage } from '../../model/editorLanguage.ts';
import { SF_SYMBOLS, type SfSymbolName } from '../../theme/icons.ts';

/** File items can number in the thousands; a query shows at most this many. */
export const MAX_FILE_RESULTS = 30;

/** `q` appears in `text` in order, not necessarily contiguously ("edst" matches "EditorState"). */
export function isSubsequence(q: string, text: string): boolean {
  let at = 0;
  for (const ch of q) {
    const found = text.indexOf(ch, at);
    if (found === -1) return false;
    at = found + ch.length;
  }
  return true;
}

/**
 * The rows for a query: with none, every item but the files; otherwise the items whose name
 * starts with it, then contain it, then whose searchable text contains it, then files matching
 * it as a subsequence, each group in palette order, with at most `MAX_FILE_RESULTS` files.
 */
export function filterPaletteItems(
  items: readonly CommandPaletteItem[],
  query: string,
): CommandPaletteItem[] {
  const q = query.toLowerCase().trim();
  if (q === '') return items.filter((item) => !isQueryOnly(item));
  const scored: { item: CommandPaletteItem; score: number; offset: number }[] = [];
  items.forEach((item, offset) => {
    const text = searchableText(item).toLowerCase();
    const name = displayName(item).toLowerCase();
    let score: number;
    if (name.startsWith(q)) score = 100;
    else if (name.includes(q)) score = 50;
    else if (text.includes(q)) score = 10;
    else if (isQueryOnly(item) && isSubsequence(q, text)) score = 5;
    else return;
    scored.push({ item, score, offset });
  });
  // Swift's `sorted` is stable here in practice; keep palette order within a score.
  scored.sort((a, b) => b.score - a.score || a.offset - b.offset);
  let fileCount = 0;
  return scored
    .map((entry) => entry.item)
    .filter((item) => {
      if (!isQueryOnly(item)) return true;
      fileCount += 1;
      return fileCount <= MAX_FILE_RESULTS;
    });
}

/** Swift `EditorLanguage.icon` (Models/EditorTab.swift), for file rows. */
const LANGUAGE_ICONS: Readonly<Record<EditorLanguage, SfSymbolName>> = {
  swift: 'swift',
  rust: 'gearshape.2',
  javascript: 'curlybraces',
  typescript: 'curlybraces',
  tsx: 'curlybraces',
  python: 'chevron.left.forwardslash.chevron.right',
  markdown: 'doc.text',
  json: 'doc.badge.gearshape',
  yaml: 'doc.badge.gearshape',
  toml: 'doc.badge.gearshape',
  html: 'globe',
  xml: 'globe',
  css: 'paintbrush',
  shell: 'terminal',
  c: 'c.square',
  cpp: 'c.square',
  go: 'arrow.right.arrow.left',
  ruby: 'diamond',
  php: 'server.rack',
  java: 'cup.and.saucer',
  csharp: 'number',
  scala: 's.square',
  haskell: 'function',
  lua: 'moon',
  plaintext: 'doc',
};

const isSfSymbolName = (name: string): name is SfSymbolName => Object.hasOwn(SF_SYMBOLS, name);

/** The row's icon; an agent definition naming a symbol the app has no icon for shows `cpu`. */
export function paletteIcon(item: CommandPaletteItem): SfSymbolName {
  const name = itemIcon(item, (fileName) => LANGUAGE_ICONS[detectEditorLanguage(fileName)]);
  return isSfSymbolName(name) ? name : 'cpu';
}
