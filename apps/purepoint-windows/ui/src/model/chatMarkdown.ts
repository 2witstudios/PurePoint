// Port of apps/shared/ChatMarkdown/ChatMarkdown.swift (WIN-9.6).
//
// Presentation blocks for Point Guard chat. Parsing is independent of React and
// tolerates unfinished fences while a reply streams. Raw HTML remains text. The
// block parser mirrors the Swift one line for line so both apps agree on every
// reply; the inline parser stands in for Foundation's
// `AttributedString(markdown:)` with `.inlineOnlyPreservingWhitespace`.

export type ChatTableAlignment = 'leading' | 'center' | 'trailing';

export type ChatMarkdownBlock =
  | { kind: 'paragraph'; text: string }
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'listItem'; depth: number; marker: string; checked: boolean | null; text: string }
  | { kind: 'listContinuation'; depth: number; blocks: ChatMarkdownBlock[] }
  | { kind: 'quote'; blocks: ChatMarkdownBlock[] }
  | { kind: 'code'; language: string; text: string }
  | { kind: 'table'; headers: string[]; alignments: ChatTableAlignment[]; rows: string[][] }
  | { kind: 'rule' }
  | { kind: 'image'; alt: string; url: string };

export type ChatInline =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'emphasis' | 'strong' | 'strikethrough'; children: ChatInline[] }
  /** `href` is null when the destination is not http, https or mailto: shown as text. */
  | { kind: 'link'; href: string | null; children: ChatInline[] };

export interface ChatMarkdownDocument {
  blocks: ChatMarkdownBlock[];
  inlineContent: Map<string, ChatInline[]>;
}

export interface ChatMarkdownOptions {
  /** Stands in for Swift task cancellation: a canceled streaming revision yields nothing. */
  isCancelled?: () => boolean;
}

export const MAX_BLOCKS = 256;
export const MAX_TABLE_CELLS = 1_200;
export const MAX_TABLE_ROWS = 100;
export const MAX_TABLE_COLUMNS = 12;
export const DOCUMENT_LIMIT_NOTICE =
  'Markdown preview limited to 256 blocks and 1,200 table cells.';
export const TABLE_LIMIT_NOTICE = 'Table preview limited to 100 rows and 12 columns.';
const MAX_NESTING = 32;
const SAFE_SCHEMES = new Set(['http', 'https', 'mailto']);

// Constructors, so tests and callers read like the Swift enum cases.
export const block = {
  paragraph: (text: string): ChatMarkdownBlock => ({ kind: 'paragraph', text }),
  heading: (level: number, text: string): ChatMarkdownBlock => ({ kind: 'heading', level, text }),
  listItem: (
    depth: number,
    marker: string,
    checked: boolean | null,
    text: string,
  ): ChatMarkdownBlock => ({ kind: 'listItem', depth, marker, checked, text }),
  listContinuation: (depth: number, blocks: ChatMarkdownBlock[]): ChatMarkdownBlock => ({
    kind: 'listContinuation',
    depth,
    blocks,
  }),
  quote: (blocks: ChatMarkdownBlock[]): ChatMarkdownBlock => ({ kind: 'quote', blocks }),
  code: (language: string, text: string): ChatMarkdownBlock => ({ kind: 'code', language, text }),
  table: (
    headers: string[],
    alignments: ChatTableAlignment[],
    rows: string[][],
  ): ChatMarkdownBlock => ({ kind: 'table', headers, alignments, rows }),
  rule: (): ChatMarkdownBlock => ({ kind: 'rule' }),
  image: (alt: string, url: string): ChatMarkdownBlock => ({ kind: 'image', alt, url }),
};

// ---------------------------------------------------------------- character helpers

// Swift's `.whitespaces`: horizontal whitespace only (no line breaks).
const HORIZONTAL_SPACE = /[\t\p{Zs}]/u;
const isHorizontalSpace = (c: string) => HORIZONTAL_SPACE.test(c);

function trimWhitespace(s: string): string {
  const chars = Array.from(s);
  let start = 0;
  let end = chars.length;
  while (start < end && isHorizontalSpace(chars[start] ?? '')) start++;
  while (end > start && isHorizontalSpace(chars[end - 1] ?? '')) end--;
  return chars.slice(start, end).join('');
}

function prefixWhile(s: string, predicate: (c: string) => boolean): string {
  let out = '';
  for (const c of s) {
    if (!predicate(c)) break;
    out += c;
  }
  return out;
}

const isBlank = (s: string) => trimWhitespace(s) === '';

function indentation(line: string): number {
  let columns = 0;
  for (const c of line) {
    if (c === ' ') columns += 1;
    else if (c === '\t') columns += 4;
    else break;
  }
  return columns;
}

function removingIndent(line: string, columns: number): string {
  let removed = 0;
  let index = 0;
  while (removed < columns && index < line.length) {
    const c = line[index];
    if (c !== ' ' && c !== '\t') break;
    removed += c === '\t' ? 4 : 1;
    index++;
  }
  return line.slice(index);
}

// ---------------------------------------------------------------- block syntax

interface Fence {
  character: string;
  count: number;
  language: string;
}

function fence(line: string): Fence | null {
  const first = line[0];
  if (first !== '`' && first !== '~') return null;
  const count = prefixWhile(line, (c) => c === first).length;
  if (count < 3) return null;
  return { character: first, count, language: trimWhitespace(line.slice(count)) };
}

function closesFence(trimmed: string, current: Fence): boolean {
  const run = prefixWhile(trimmed, (c) => c === current.character).length;
  return run >= current.count && isBlank(trimmed.slice(run));
}

function isRule(line: string): boolean {
  const compact = line.replace(/\s/gu, '');
  const first = compact[0];
  return (
    compact.length >= 3 &&
    first !== undefined &&
    '-*_'.includes(first) &&
    Array.from(compact).every((c) => c === first)
  );
}

// Only https: a `file:` or UNC image would make Windows authenticate to the host it names (an NTLM
// hash leak), and plain http is readable and alterable on the way. The app's CSP allows https
// images only, and each still loads only when the reader clicks Load image.
const IMAGE = /^!\[([^\]]*)\]\((https:\/\/[^\s]+)\)$/iu;

function image(line: string): { alt: string; url: string } | null {
  const match = IMAGE.exec(line);
  if (!match) return null;
  const url = match[2] ?? '';
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== 'https:' || parsed.hostname === '') return null;
  } catch {
    return null;
  }
  return { alt: match[1] ?? '', url };
}

interface ListItemSyntax {
  indent: number;
  marker: string;
  checked: boolean | null;
  text: string;
}

function listItem(line: string): ListItemSyntax | null {
  const t = trimWhitespace(line);
  let marker = '•';
  let content: string;
  if (t.startsWith('- ') || t.startsWith('+ ') || t.startsWith('* ')) {
    content = t.slice(2);
  } else {
    // Character.isNumber accepts any Unicode number, not only ASCII digits.
    const digits = prefixWhile(t, (c) => /\p{N}/u.test(c));
    const rest = t.slice(digits.length);
    if (
      digits === '' ||
      Array.from(digits).length > 9 ||
      !(rest.startsWith('. ') || rest.startsWith(') '))
    ) {
      return null;
    }
    marker = digits + '.';
    content = rest.slice(2);
  }
  let checked: boolean | null = null;
  const lowered = content.toLowerCase();
  if (content.startsWith('[ ] ') || lowered.startsWith('[x] ')) {
    checked = lowered.startsWith('[x]');
    content = content.slice(4);
  }
  return { indent: indentation(line), marker, checked, text: content };
}

/** Split pipes outside code spans; escaped pipes belong to their cell. */
export function cells(source: string, options: ChatMarkdownOptions = {}): string[] {
  const cancelled = options.isCancelled ?? (() => false);
  let line = trimWhitespace(source);
  if (line.startsWith('|')) line = line.slice(1);
  if (line.endsWith('|') && !line.endsWith('\\|')) line = line.slice(0, -1);
  const characters = Array.from(line);
  const result: string[] = [];
  let cell = '';
  let escaped = false;
  let delimiter = 0;
  let index = 0;
  // Index the last run of each length once. Looking for a matching close
  // must not rescan the suffix for every unmatched opening delimiter.
  const lastRun = new Map<number, number>();
  let cursor = 0;
  while (cursor < characters.length) {
    if (cancelled()) return [];
    if (characters[cursor] === '`') {
      const start = cursor;
      while (cursor < characters.length && characters[cursor] === '`') cursor++;
      lastRun.set(cursor - start, start);
    } else {
      cursor++;
    }
  }
  while (index < characters.length) {
    if (cancelled()) return [];
    const character = characters[index] ?? '';
    index++;
    if (escaped) {
      if (character !== '|') cell += '\\';
      cell += character;
      escaped = false;
      continue;
    }
    if (character === '\\' && delimiter === 0) {
      escaped = true;
      continue;
    }
    if (character === '`') {
      const start = index - 1;
      while (index < characters.length && characters[index] === '`') index++;
      const length = index - start;
      if (delimiter === length) delimiter = 0;
      else if (delimiter === 0 && (lastRun.get(length) ?? -1) >= index) delimiter = length;
      cell += '`'.repeat(length);
      continue;
    }
    if (character === '|' && delimiter === 0) {
      result.push(trimWhitespace(cell));
      cell = '';
    } else {
      cell += character;
    }
  }
  if (escaped) cell += '\\';
  result.push(trimWhitespace(cell));
  return result;
}

function tableDelimiter(source: string, options: ChatMarkdownOptions): ChatTableAlignment[] | null {
  const parts = cells(source, options);
  const valid =
    parts.length > 0 &&
    parts.every(
      (part) =>
        Array.from(part).filter((c) => c === '-').length >= 3 &&
        Array.from(part).every((c) => c === '-' || c === ':'),
    );
  if (!valid) return null;
  return parts.map((part) =>
    part.endsWith(':') ? (part.startsWith(':') ? 'center' : 'trailing') : 'leading',
  );
}

// ---------------------------------------------------------------- block parser

interface ParseBudget {
  remaining: number;
  cells: number;
  truncated: boolean;
}

export function parse(source: string, options: ChatMarkdownOptions = {}): ChatMarkdownBlock[] {
  const cancelled = options.isCancelled ?? (() => false);
  const budget: ParseBudget = { remaining: MAX_BLOCKS, cells: MAX_TABLE_CELLS, truncated: false };
  // Object fields, so the closure's writes stay visible to flow analysis.
  const limit = { blocks: MAX_BLOCKS, cells: MAX_TABLE_CELLS, truncated: false };
  function bounded(blocks: ChatMarkdownBlock[]): ChatMarkdownBlock[] {
    const result: ChatMarkdownBlock[] = [];
    for (const item of blocks) {
      if (limit.blocks <= 0) {
        limit.truncated = true;
        break;
      }
      limit.blocks -= 1;
      switch (item.kind) {
        case 'quote':
          result.push(block.quote(bounded(item.blocks)));
          break;
        case 'listContinuation':
          result.push(block.listContinuation(item.depth, bounded(item.blocks)));
          break;
        case 'table': {
          const columns = item.headers.length;
          if (columns === 0) continue;
          if (limit.cells < columns) {
            limit.truncated = true;
            continue;
          }
          limit.cells -= columns;
          const count = Math.min(item.rows.length, Math.floor(limit.cells / columns));
          limit.cells -= count * columns;
          if (count < item.rows.length) limit.truncated = true;
          result.push(block.table(item.headers, item.alignments, item.rows.slice(0, count)));
          break;
        }
        default:
          result.push(item);
      }
    }
    return result;
  }
  const parsed = parseNested(source, 0, budget, options);
  if (cancelled()) return [];
  const blocks = bounded(parsed);
  if (limit.truncated || budget.truncated) blocks.push(block.paragraph(DOCUMENT_LIMIT_NOTICE));
  return blocks;
}

function parseNested(
  source: string,
  nesting: number,
  budget: ParseBudget,
  options: ChatMarkdownOptions,
): ChatMarkdownBlock[] {
  const cancelled = options.isCancelled ?? (() => false);
  if (cancelled()) return [];
  // Replies are untrusted content. Keep parser and React recursion bounded.
  if (source === '') return [];
  if (budget.remaining <= 0) {
    budget.truncated = true;
    return [];
  }
  if (nesting >= MAX_NESTING) {
    budget.remaining -= 1;
    return [block.paragraph(source)];
  }
  const lines = source.replaceAll('\r\n', '\n').replaceAll('\r', '\n').split('\n');
  const blocks: ChatMarkdownBlock[] = [];
  let paragraph: string[] = [];
  let orderedCounters = new Map<number, number>();
  let i = 0;
  const line = (index: number) => lines[index] ?? '';
  function append(item: ChatMarkdownBlock) {
    if (budget.remaining <= 0) {
      budget.truncated = true;
      return;
    }
    budget.remaining -= 1;
    blocks.push(item);
  }
  function flush() {
    if (paragraph.length > 0) {
      append(block.paragraph(paragraph.join('\n')));
      paragraph = [];
    }
  }
  function flushForBlock(): boolean {
    flush();
    if (budget.remaining <= 0) {
      budget.truncated = true;
      return false;
    }
    return true;
  }
  while (i < lines.length) {
    if (cancelled()) return [];
    const current = line(i);
    const trimmed = trimWhitespace(current);
    if (trimmed === '') {
      flush();
      i += 1;
      continue;
    }
    if (budget.remaining <= 0) {
      budget.truncated = true;
      break;
    }
    if (listItem(current) === null) orderedCounters.clear();

    const opening = fence(trimmed);
    if (opening) {
      if (!flushForBlock()) break;
      i += 1;
      const code: string[] = [];
      while (i < lines.length) {
        if (cancelled()) return [];
        if (closesFence(trimWhitespace(line(i)), opening)) {
          i += 1;
          break;
        }
        code.push(line(i));
        i += 1;
      }
      append(block.code(opening.language, code.join('\n')));
      continue;
    }

    const hashes = prefixWhile(trimmed, (c) => c === '#').length;
    if (hashes >= 1 && hashes <= 6 && trimmed[hashes] === ' ') {
      if (!flushForBlock()) break;
      append(block.heading(hashes, trimmed.slice(hashes + 1)));
      i += 1;
      continue;
    }

    if (isRule(trimmed)) {
      orderedCounters.clear();
      if (!flushForBlock()) break;
      append(block.rule());
      i += 1;
      continue;
    }

    if (trimmed.startsWith('>')) {
      if (!flushForBlock()) break;
      const quoted: string[] = [];
      while (i < lines.length) {
        if (cancelled()) return [];
        const candidate = trimWhitespace(line(i));
        if (!candidate.startsWith('>')) break;
        let content = candidate.slice(1);
        if (content.startsWith(' ')) content = content.slice(1);
        quoted.push(content);
        i += 1;
      }
      budget.remaining -= 1; // Reserve the quote container before its children.
      const nested = parseNested(quoted.join('\n'), nesting + 1, budget, options);
      blocks.push(block.quote(nested));
      continue;
    }

    if (i + 1 < lines.length && trimmed.includes('|')) {
      const alignments = tableDelimiter(line(i + 1), options);
      if (alignments && alignments.length === cells(trimmed, options).length) {
        if (!flushForBlock()) break;
        const headers = cells(trimmed, options).slice(0, MAX_TABLE_COLUMNS);
        if (headers.length === 0) return [];
        const isRowLine = (index: number) => line(index).includes('|') && !isBlank(line(index));
        if (budget.cells < headers.length) {
          budget.truncated = true;
          i += 2;
          while (i < lines.length && isRowLine(i)) {
            if (cancelled()) return [];
            i += 1;
          }
          continue;
        }
        budget.cells -= headers.length;
        const rowLimit = Math.min(MAX_TABLE_ROWS, Math.floor(budget.cells / headers.length));
        i += 2;
        const rows: string[][] = [];
        let rowCount = 0;
        while (i < lines.length && isRowLine(i)) {
          if (cancelled()) return [];
          if (rows.length < rowLimit) {
            const row = cells(line(i), options).slice(0, headers.length);
            while (row.length < headers.length) row.push('');
            rows.push(row);
          }
          rowCount += 1;
          i += 1;
        }
        budget.cells -= rows.length * headers.length;
        if (rowCount > rowLimit && rowLimit < MAX_TABLE_ROWS) budget.truncated = true;
        append(block.table(headers, alignments.slice(0, headers.length), rows));
        if (rowCount > MAX_TABLE_ROWS || alignments.length > MAX_TABLE_COLUMNS) {
          append(block.paragraph(TABLE_LIMIT_NOTICE));
        }
        continue;
      }
    }

    const picture = image(trimmed);
    if (picture) {
      if (!flushForBlock()) break;
      append(block.image(picture.alt, picture.url));
      i += 1;
      continue;
    }

    const item = listItem(current);
    if (item) {
      if (!flushForBlock()) break;
      i += 1;
      const continuation: string[] = [];
      let activeFence: Fence | null = null;
      while (i < lines.length) {
        if (cancelled()) return [];
        const next = line(i);
        const nextTrimmed = trimWhitespace(next);
        if (activeFence) {
          if (closesFence(nextTrimmed, activeFence)) activeFence = null;
        } else if (nextTrimmed === '') {
          let lookahead = i + 1;
          while (lookahead < lines.length && isBlank(line(lookahead))) lookahead += 1;
          if (!(
            lookahead < lines.length &&
            indentation(line(lookahead)) > item.indent &&
            listItem(line(lookahead)) === null
          )) {
            break;
          }
        } else {
          if (!(indentation(next) > item.indent && listItem(next) === null)) break;
          activeFence = fence(nextTrimmed);
        }
        continuation.push(next);
        i += 1;
      }
      const depth = Math.min(Math.floor(item.indent / 2), 8);
      orderedCounters = new Map([...orderedCounters].filter(([key]) => key <= depth));
      let marker = item.marker;
      const digits = item.marker.slice(0, -1);
      if (/^[0-9]+$/.test(digits)) {
        // Swift's Int(String) only accepts ASCII digits.
        const number = orderedCounters.get(depth) ?? Number(digits);
        marker = `${String(number)}.`;
        orderedCounters.set(depth, number + 1);
      } else {
        orderedCounters.delete(depth);
      }
      const firstContent = continuation.find((candidate) => !isBlank(candidate));
      const margin = firstContent === undefined ? 0 : indentation(firstContent);
      const body = continuation.map((candidate) => removingIndent(candidate, margin)).join('\n');
      append(block.listItem(depth, marker, item.checked, item.text));
      if (body !== '') {
        const merged = (continued: string) =>
          block.listItem(depth, marker, item.checked, item.text + '\n' + continued);
        if (budget.remaining < 2) {
          // Prose merges into the existing item without adding blocks.
          // A bounded probe distinguishes it from structural content.
          const probe: ParseBudget = { remaining: 2, cells: budget.cells, truncated: false };
          const contents = parseNested(body, nesting + 1, probe, options);
          const only = contents[0];
          if (contents.length === 1 && only?.kind === 'paragraph' && !probe.truncated) {
            blocks[blocks.length - 1] = merged(only.text);
          } else {
            budget.truncated = true;
          }
          continue;
        }
        budget.remaining -= 1; // Reserve the continuation container.
        const contents = parseNested(body, nesting + 1, budget, options);
        const only = contents[0];
        if (contents.length === 1 && only?.kind === 'paragraph') {
          blocks[blocks.length - 1] = merged(only.text);
          budget.remaining += 2; // Fold the paragraph and container into the item.
        } else if (contents.length === 0) {
          budget.remaining += 1;
        } else {
          blocks.push(block.listContinuation(Math.min(depth + 1, 8), contents));
        }
      }
      continue;
    }

    // Setext headings, common in saved assistant replies.
    if (i + 1 < lines.length) {
      const underline = Array.from(trimWhitespace(line(i + 1)));
      if (
        underline.length >= 3 &&
        (underline.every((c) => c === '=') || underline.every((c) => c === '-'))
      ) {
        if (!flushForBlock()) break;
        append(block.heading(underline[0] === '=' ? 1 : 2, trimmed));
        i += 2;
        continue;
      }
    }
    paragraph.push(current);
    i += 1;
  }
  flush();
  return blocks;
}

// ---------------------------------------------------------------- document

/** Blocks plus the inline content of every paragraph, heading, list item and table cell. */
export function document(source: string, options: ChatMarkdownOptions = {}): ChatMarkdownDocument {
  const cancelled = options.isCancelled ?? (() => false);
  const blocks = parse(source, options);
  const inlineContent = new Map<string, ChatInline[]>();
  function prepare(text: string) {
    if (cancelled() || inlineContent.has(text)) return;
    inlineContent.set(text, parseInline(text));
  }
  function visit(items: ChatMarkdownBlock[]) {
    for (const item of items) {
      if (cancelled()) return;
      switch (item.kind) {
        case 'paragraph':
        case 'heading':
        case 'listItem':
          prepare(item.text);
          break;
        case 'quote':
        case 'listContinuation':
          visit(item.blocks);
          break;
        case 'table':
          for (const cell of item.headers) prepare(cell);
          for (const row of item.rows) for (const cell of row) prepare(cell);
          break;
        case 'code':
        case 'rule':
        case 'image':
          break;
      }
    }
  }
  visit(blocks);
  return { blocks, inlineContent };
}

/**
 * Serial preparation, mirroring the Swift actor: a revision whose signal is
 * aborted before or during parsing yields an empty document.
 */
export function prepareChatMarkdown(source: string, signal?: AbortSignal): ChatMarkdownDocument {
  return document(source, { isCancelled: () => signal?.aborted ?? false });
}

// ---------------------------------------------------------------- inline parser

/** Only these schemes are clickable; any other link is rendered as its text. */
export function isSafeLinkDestination(url: string): boolean {
  const scheme = /^([A-Za-z][A-Za-z0-9+.-]*):/.exec(url)?.[1];
  return scheme !== undefined && SAFE_SCHEMES.has(scheme.toLowerCase());
}

/** The plain characters of inline content, like `String(attributed.characters)`. */
export function inlineText(nodes: readonly ChatInline[]): string {
  return nodes.map((node) => ('children' in node ? inlineText(node.children) : node.text)).join('');
}

const ASCII_PUNCTUATION = /[!-/:-@[-`{-~]/;
const PUNCTUATION = /[\p{P}\p{S}]/u;
const WHITESPACE = /\s/u;
const AUTOLINK = /^<([A-Za-z][A-Za-z0-9+.-]{1,31}:[^<>\p{Cc} ]*)>/u;
const EMAIL_AUTOLINK =
  /^<([a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)>/;
const ENTITY = /^&(?:#[xX]([0-9a-fA-F]{1,6})|#([0-9]{1,7})|([A-Za-z][A-Za-z0-9]{1,31}));/;
const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: String.fromCharCode(0xa0),
  copy: '©',
  reg: '®',
  trade: '™',
  hellip: '…',
  mdash: '—',
  ndash: '–',
};

interface Piece {
  node: ChatInline;
  prev: Piece | null;
  next: Piece | null;
}

interface Delimiter {
  piece: Piece;
  character: string;
  count: number;
  originalCount: number;
  canOpen: boolean;
  canClose: boolean;
  prev: Delimiter | null;
  next: Delimiter | null;
}

interface Bracket {
  piece: Piece;
  image: boolean;
  active: boolean;
  delimiterBefore: Delimiter | null;
  prev: Bracket | null;
}

/**
 * CommonMark inline syntax as Foundation reads it: code spans, emphasis, strong,
 * GFM strikethrough, inline links, autolinks, escapes and entities. Raw HTML stays
 * literal text. Images inside a line keep only their alt text (they never load).
 */
export function parseInline(source: string): ChatInline[] {
  const head: Piece = { node: { kind: 'text', text: '' }, prev: null, next: null };
  let tail = head;
  let delimiters: Delimiter | null = null;
  let brackets: Bracket | null = null;
  let text = '';

  function pushPiece(node: ChatInline): Piece {
    const piece: Piece = { node, prev: tail, next: null };
    tail.next = piece;
    tail = piece;
    return piece;
  }
  function flushText() {
    if (text !== '') {
      pushPiece({ kind: 'text', text });
      text = '';
    }
  }
  function removePiece(piece: Piece) {
    if (piece.prev) piece.prev.next = piece.next;
    if (piece.next) piece.next.prev = piece.prev;
    if (tail === piece) tail = piece.prev ?? head;
  }
  function removeDelimiter(delimiter: Delimiter) {
    if (delimiter.prev) delimiter.prev.next = delimiter.next;
    if (delimiter.next) delimiter.next.prev = delimiter.prev;
    if (delimiters === delimiter) delimiters = delimiter.prev;
  }
  const charBefore = (index: number) => {
    if (index <= 0) return '\n';
    const code = source.codePointAt(index - 1) ?? 0;
    if (code >= 0xdc00 && code <= 0xdfff && index >= 2) {
      return String.fromCodePoint(source.codePointAt(index - 2) ?? 0);
    }
    return String.fromCodePoint(code);
  };
  const charAt = (index: number) =>
    index >= source.length ? '\n' : String.fromCodePoint(source.codePointAt(index) ?? 0);

  function collect(from: Piece | null, until: Piece | null): ChatInline[] {
    const nodes: ChatInline[] = [];
    for (let piece = from; piece && piece !== until; piece = piece.next) nodes.push(piece.node);
    return nodes;
  }

  function processEmphasis(bottom: Delimiter | null) {
    // Find the first delimiter above the bottom.
    let closer: Delimiter | null = delimiters === bottom ? null : delimiters;
    while (closer && closer.prev !== bottom) closer = closer.prev;
    const openersBottom = new Map<string, Delimiter | null>();
    while (closer) {
      if (!closer.canClose) {
        closer = closer.next;
        continue;
      }
      const key = `${closer.character}${String(closer.originalCount % 3)}${String(closer.canOpen)}`;
      const floor = openersBottom.has(key) ? (openersBottom.get(key) ?? null) : bottom;
      let opener = closer.prev;
      let found = false;
      while (opener && opener !== bottom && opener !== floor) {
        if (opener.character === closer.character && opener.canOpen) {
          if (closer.character === '~') {
            found = opener.count === closer.count;
          } else {
            const oddMatch =
              (closer.canOpen || opener.canClose) &&
              (opener.originalCount + closer.originalCount) % 3 === 0 &&
              !(opener.originalCount % 3 === 0 && closer.originalCount % 3 === 0);
            found = !oddMatch;
          }
          if (found) break;
        }
        opener = opener.prev;
      }
      const next: Delimiter | null = closer.next;
      if (found && opener) {
        const used =
          closer.character === '~' ? closer.count : closer.count >= 2 && opener.count >= 2 ? 2 : 1;
        opener.count -= used;
        closer.count -= used;
        const openerNode = opener.piece.node as { kind: 'text'; text: string };
        const closerNode = closer.piece.node as { kind: 'text'; text: string };
        openerNode.text = openerNode.text.slice(used);
        closerNode.text = closerNode.text.slice(used);
        const kind =
          closer.character === '~' ? 'strikethrough' : used === 2 ? 'strong' : 'emphasis';
        const wrapper: Piece = {
          node: { kind, children: collect(opener.piece.next, closer.piece) },
          prev: opener.piece,
          next: closer.piece,
        };
        opener.piece.next = wrapper;
        closer.piece.prev = wrapper;
        // Delimiters between the two are now inside the wrapper.
        opener.next = closer;
        closer.prev = opener;
        if (opener.count === 0) {
          removePiece(opener.piece);
          removeDelimiter(opener);
        }
        if (closer.count === 0) {
          removePiece(closer.piece);
          removeDelimiter(closer);
          closer = next;
        }
      } else {
        openersBottom.set(key, closer.prev);
        if (!closer.canOpen) removeDelimiter(closer);
        closer = next;
      }
    }
    // Everything above the bottom is now literal text.
    while (delimiters && delimiters !== bottom) removeDelimiter(delimiters);
  }

  function linkTail(start: number): { destination: string; end: number } | null {
    // `start` points just past `](`.
    let index = start;
    const skipSpace = () => {
      while (index < source.length && WHITESPACE.test(source[index] ?? '')) index++;
    };
    skipSpace();
    let destination = '';
    if (source[index] === '<') {
      index++;
      while (index < source.length && source[index] !== '>') {
        const c = source[index] ?? '';
        if (c === '\n' || c === '<') return null;
        if (c === '\\' && ASCII_PUNCTUATION.test(source[index + 1] ?? '')) {
          destination += source[index + 1] ?? '';
          index += 2;
          continue;
        }
        destination += c;
        index++;
      }
      if (source[index] !== '>') return null;
      index++;
    } else {
      let depth = 0;
      while (index < source.length) {
        const c = source[index] ?? '';
        if (WHITESPACE.test(c) || c.charCodeAt(0) < 0x20) break;
        if (c === '\\' && ASCII_PUNCTUATION.test(source[index + 1] ?? '')) {
          destination += source[index + 1] ?? '';
          index += 2;
          continue;
        }
        if (c === '(') depth++;
        if (c === ')') {
          if (depth === 0) break;
          depth--;
        }
        destination += c;
        index++;
      }
      if (depth !== 0) return null;
    }
    const beforeTitle = index;
    skipSpace();
    const quote = source[index];
    if (index > beforeTitle && (quote === '"' || quote === "'" || quote === '(')) {
      const close = quote === '(' ? ')' : quote;
      index++;
      while (index < source.length && source[index] !== close) {
        if (source[index] === '\\') index++;
        index++;
      }
      if (source[index] !== close) return null;
      index++;
      skipSpace();
    }
    if (source[index] !== ')') return null;
    return { destination: decodeEntities(destination), end: index + 1 };
  }

  let index = 0;
  while (index < source.length) {
    const c = source[index] ?? '';
    if (c === '\\') {
      const next = source[index + 1] ?? '';
      if (ASCII_PUNCTUATION.test(next)) {
        text += next;
        index += 2;
        continue;
      }
      if (next === '\n') {
        text += '\n';
        index += 2;
        continue;
      }
      text += c;
      index++;
      continue;
    }
    if (c === '`') {
      let end = index;
      while (source[end] === '`') end++;
      const length = end - index;
      let search = end;
      let closing = -1;
      while (search < source.length) {
        const found = source.indexOf('`', search);
        if (found < 0) break;
        let runEnd = found;
        while (source[runEnd] === '`') runEnd++;
        if (runEnd - found === length) {
          closing = found;
          break;
        }
        search = runEnd;
      }
      if (closing < 0) {
        text += '`'.repeat(length);
        index = end;
        continue;
      }
      let code = source.slice(end, closing).replaceAll('\n', ' ');
      if (code.length >= 2 && code.startsWith(' ') && code.endsWith(' ') && code.trim() !== '') {
        code = code.slice(1, -1);
      }
      flushText();
      pushPiece({ kind: 'code', text: code });
      index = closing + length;
      continue;
    }
    if (c === '*' || c === '_' || c === '~') {
      let end = index;
      while (source[end] === c) end++;
      const count = end - index;
      const run = source.slice(index, end);
      if (c === '~' && count > 2) {
        text += run;
        index = end;
        continue;
      }
      const before = charBefore(index);
      const after = charAt(end);
      const afterSpace = WHITESPACE.test(after);
      const beforeSpace = WHITESPACE.test(before);
      const afterPunct = PUNCTUATION.test(after);
      const beforePunct = PUNCTUATION.test(before);
      const left = !afterSpace && (!afterPunct || beforeSpace || beforePunct);
      const right = !beforeSpace && (!beforePunct || afterSpace || afterPunct);
      let canOpen = left;
      let canClose = right;
      if (c === '_') {
        canOpen = left && (!right || beforePunct);
        canClose = right && (!left || afterPunct);
      }
      flushText();
      const piece = pushPiece({ kind: 'text', text: run });
      if (canOpen || canClose) {
        const delimiter: Delimiter = {
          piece,
          character: c,
          count,
          originalCount: count,
          canOpen,
          canClose,
          prev: delimiters,
          next: null,
        };
        if (delimiters) delimiters.next = delimiter;
        delimiters = delimiter;
      }
      index = end;
      continue;
    }
    if (c === '[' || (c === '!' && source[index + 1] === '[')) {
      const isImage = c === '!';
      flushText();
      const piece = pushPiece({ kind: 'text', text: isImage ? '![' : '[' });
      brackets = {
        piece,
        image: isImage,
        active: true,
        delimiterBefore: delimiters,
        prev: brackets,
      };
      index += isImage ? 2 : 1;
      continue;
    }
    if (c === ']') {
      const opener: Bracket | null = brackets;
      if (!opener) {
        text += c;
        index++;
        continue;
      }
      if (!opener.active) {
        brackets = opener.prev;
        text += c;
        index++;
        continue;
      }
      const link = source[index + 1] === '(' ? linkTail(index + 2) : null;
      if (!link) {
        brackets = opener.prev;
        text += c;
        index++;
        continue;
      }
      flushText();
      processEmphasis(opener.delimiterBefore);
      const children = collect(opener.piece.next, null);
      // Replace the opener and everything after it with the link (or the alt text).
      opener.piece.next = null;
      tail = opener.piece;
      if (opener.image) {
        opener.piece.node = { kind: 'text', text: '' };
        for (const child of children) pushPiece(child);
      } else {
        opener.piece.node = {
          kind: 'link',
          href: isSafeLinkDestination(link.destination) ? link.destination : null,
          children,
        };
        // Links may not contain links.
        for (let earlier = opener.prev; earlier; earlier = earlier.prev) {
          if (!earlier.image) earlier.active = false;
        }
      }
      brackets = opener.prev;
      index = link.end;
      continue;
    }
    if (c === '<') {
      const rest = source.slice(index);
      const uri = AUTOLINK.exec(rest);
      const email = uri ? null : EMAIL_AUTOLINK.exec(rest);
      if (uri || email) {
        const target = (uri ?? email)?.[1] ?? '';
        const href = uri ? target : `mailto:${target}`;
        flushText();
        pushPiece({
          kind: 'link',
          href: isSafeLinkDestination(href) ? href : null,
          children: [{ kind: 'text', text: target }],
        });
        index += (uri ?? email)?.[0].length ?? 1;
        continue;
      }
      text += c;
      index++;
      continue;
    }
    if (c === '&') {
      const entity = ENTITY.exec(source.slice(index, index + 40));
      if (entity) {
        const decoded = decodeEntity(entity);
        if (decoded !== null) {
          text += decoded;
          index += entity[0].length;
          continue;
        }
      }
    }
    text += c;
    index++;
  }
  flushText();
  processEmphasis(null);
  return mergeText(collect(head.next, null));
}

function decodeEntity(match: RegExpExecArray): string | null {
  const [, hex, decimal, name] = match;
  const code = hex ? parseInt(hex, 16) : decimal ? parseInt(decimal, 10) : null;
  if (code !== null) {
    return code === 0 || code > 0x10ffff ? String.fromCharCode(0xfffd) : String.fromCodePoint(code);
  }
  return name ? (NAMED_ENTITIES[name] ?? null) : null;
}

function decodeEntities(source: string): string {
  return source.replace(
    /&(?:#[xX][0-9a-fA-F]{1,6}|#[0-9]{1,7}|[A-Za-z][A-Za-z0-9]{1,31});/g,
    (raw) => {
      const match = ENTITY.exec(raw);
      return (match && decodeEntity(match)) ?? raw;
    },
  );
}

function mergeText(nodes: ChatInline[]): ChatInline[] {
  const result: ChatInline[] = [];
  for (const node of nodes) {
    if (node.kind === 'text') {
      if (node.text === '') continue;
      const last = result[result.length - 1];
      if (last?.kind === 'text') {
        result[result.length - 1] = { kind: 'text', text: last.text + node.text };
        continue;
      }
      result.push({ kind: 'text', text: node.text });
    } else if ('children' in node) {
      result.push({ ...node, children: mergeText(node.children) });
    } else {
      result.push(node);
    }
  }
  return result;
}
