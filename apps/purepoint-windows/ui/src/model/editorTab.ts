// Port of apps/purepoint-macos/purepoint-macos/Models/EditorTab.swift: `EditorLanguage.icon` (the
// file tree's row icons, WIN-8.4) and `EditorTab`, the editor's open file (WIN-8.5).
// `EditorLanguage.detect` is `./editorLanguage.ts`.
import type { SfSymbolName } from '../theme/icons';
import type { EditorLanguage } from './editorLanguage';

/** Swift `EditorLanguage.icon`. */
export function editorLanguageIcon(language: EditorLanguage): SfSymbolName {
  switch (language) {
    case 'swift':
      return 'swift';
    case 'rust':
      return 'gearshape.2';
    case 'javascript':
    case 'typescript':
    case 'tsx':
      return 'curlybraces';
    case 'python':
      return 'chevron.left.forwardslash.chevron.right';
    case 'markdown':
      return 'doc.text';
    case 'json':
    case 'yaml':
    case 'toml':
      return 'doc.badge.gearshape';
    case 'html':
    case 'xml':
      return 'globe';
    case 'css':
      return 'paintbrush';
    case 'shell':
      return 'terminal';
    case 'c':
    case 'cpp':
      return 'c.square';
    case 'go':
      return 'arrow.right.arrow.left';
    case 'ruby':
      return 'diamond';
    case 'php':
      return 'server.rack';
    case 'java':
      return 'cup.and.saucer';
    case 'csharp':
      return 'number';
    case 'scala':
      return 's.square';
    case 'haskell':
      return 'function';
    case 'lua':
      return 'moon';
    case 'plaintext':
      return 'doc';
  }
}

/** Swift `EditorTab`: the file an editor has open. */
export interface EditorTab {
  /** The absolute path. */
  readonly id: string;
  readonly name: string;
  readonly content: string;
  readonly isDirty: boolean;
  /** The modification time of the version shown (Swift `lastModified`). */
  readonly lastModified: number | null;
  readonly isBinary: boolean;
  readonly language: EditorLanguage;
  /** Shown but not editable: a remote file too large to save back (over 750 KB). */
  readonly isReadOnly: boolean;
}
