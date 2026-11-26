// Port of apps/purepoint-macos/purepoint-macos/Services/MarkdownHTMLRenderer.swift (WIN-8.6): a small,
// dependency-free Markdown → HTML converter for the file pane's preview. Covers the constructs docs
// actually use (headings, fences, lists, quotes, rules, tables, inline code/emphasis/links/images).
// All text is HTML-escaped before inline markup is applied, so file content can never inject
// markup or script: raw HTML in the file shows as text.
//
// Differences from Swift: `\w` in the emphasis guards is Unicode-aware (NSRegularExpression's is;
// JavaScript's is ASCII without the class written out), and `page` uses the Windows UI and
// monospace faces.

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function headingLevel(line: string): number | null {
  const hashes = /^#*/.exec(line)?.[0].length ?? 0;
  if (hashes < 1 || hashes > 6 || line[hashes] !== ' ') return null;
  return hashes;
}

function isRule(line: string): boolean {
  const compact = line.replace(/ /g, '');
  const first = compact[0];
  if (compact.length < 3 || first === undefined || !'-*_'.includes(first)) return false;
  return compact.split(first).join('') === '';
}

function isTableDelimiter(line: string): boolean {
  const t = line.trim();
  if (!t.includes('-') || !t.includes('|')) return false;
  return /^[|\-: ]*$/.test(t);
}

function cells(line: string): string[] {
  let t = line.trim();
  if (t.startsWith('|')) t = t.slice(1);
  if (t.endsWith('|')) t = t.slice(0, -1);
  return t.split('|').map((cell) => cell.trim());
}

function listMarker(line: string): { ordered: boolean; text: string } | null {
  const t = line.trim();
  for (const bullet of ['- ', '* ', '+ ']) {
    if (t.startsWith(bullet)) return { ordered: false, text: t.slice(2) };
  }
  const digits = /^[0-9]*/.exec(t)?.[0] ?? '';
  if (digits.length > 0 && digits.length <= 9) {
    const rest = t.slice(digits.length);
    if (rest.startsWith('. ') || rest.startsWith(') '))
      return { ordered: true, text: rest.slice(2) };
  }
  return null;
}

/**
 * Only http(s), mailto and scheme-less (relative/anchor) URLs survive; anything else (notably
 * `javascript:`) becomes an inert anchor.
 */
export function safeUrl(url: string): string {
  const lower = url.toLowerCase();
  if (lower.startsWith('http://') || lower.startsWith('https://') || lower.startsWith('mailto:')) {
    return url;
  }
  const colon = lower.indexOf(':');
  if (colon >= 0 && !lower.slice(0, colon).includes('/')) return '#';
  return url;
}

/**
 * An image source the preview may load: only an absolute `https:` URL. `file:` and UNC sources
 * would make Windows authenticate to whatever host the document names (an NTLM hash leak), and
 * relative ones would resolve against the app itself, so those images show their alt text.
 */
export function safeImageUrl(url: string): string | null {
  if (!url.toLowerCase().startsWith('https://')) return null;
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'https:' && parsed.hostname !== '' ? url : null;
  } catch {
    return null;
  }
}

function replace(text: string, pattern: RegExp, build: (groups: string[]) => string): string {
  return text.replace(pattern, (...args: unknown[]) => {
    // The groups sit between the match and its offset.
    const end = args.findIndex((arg) => typeof arg === 'number');
    const groups = args.slice(0, end).map((group) => (typeof group === 'string' ? group : ''));
    return build(groups);
  });
}

const WORD = '[\\p{L}\\p{M}\\p{N}_]';

/** Inline markup on already-untrusted text: escape first, then apply patterns. */
export function renderInline(raw: string): string {
  const codeSpans: string[] = [];
  let text = escapeHtml(raw);

  // Pull code spans out first so their contents are not further processed.
  text = replace(text, /`([^`]+)`/g, (g) => {
    codeSpans.push(`<code>${g[1] ?? ''}</code>`);
    return `\u0000${String(codeSpans.length - 1)}\u0000`;
  });
  text = replace(text, /!\[([^\]]*)\]\(([^)\s]+)\)/g, (g) => {
    const src = safeImageUrl(g[2] ?? '');
    return src === null ? (g[1] ?? '') : `<img alt="${g[1] ?? ''}" src="${src}">`;
  });
  text = replace(
    text,
    /\[([^\]]+)\]\(([^)\s]+)\)/g,
    (g) => `<a href="${safeUrl(g[2] ?? '')}">${g[1] ?? ''}</a>`,
  );
  text = replace(text, /\*\*(.+?)\*\*/g, (g) => `<strong>${g[1] ?? ''}</strong>`);
  text = replace(text, /__(.+?)__/g, (g) => `<strong>${g[1] ?? ''}</strong>`);
  text = replace(
    text,
    new RegExp(`(?<!(?:${WORD}|\\*))\\*(?!\\s)(.+?)(?<!\\s)\\*(?!(?:${WORD}|\\*))`, 'gu'),
    (g) => `<em>${g[1] ?? ''}</em>`,
  );
  text = replace(
    text,
    new RegExp(`(?<!${WORD})_(?!\\s)(.+?)(?<!\\s)_(?!${WORD})`, 'gu'),
    (g) => `<em>${g[1] ?? ''}</em>`,
  );
  text = replace(text, /~~(.+?)~~/g, (g) => `<del>${g[1] ?? ''}</del>`);

  codeSpans.forEach((span, index) => {
    text = text.split(`\u0000${String(index)}\u0000`).join(span);
  });
  return text;
}

/** Swift `MarkdownHTMLRenderer.render`: the body HTML of a Markdown document. */
export function renderMarkdown(markdown: string): string {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  let html = '';
  let paragraph: string[] = [];
  let i = 0;
  const line = (index: number) => lines[index] ?? '';

  const flushParagraph = () => {
    if (paragraph.length === 0) return;
    html += `<p>${renderInline(paragraph.join(' '))}</p>\n`;
    paragraph = [];
  };

  while (i < lines.length) {
    const current = line(i);
    const trimmed = current.trim();

    // Fenced code block
    if (trimmed.startsWith('```') || trimmed.startsWith('~~~')) {
      flushParagraph();
      const fence = trimmed.slice(0, 3);
      const lang = trimmed.slice(3).trim();
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !line(i).trim().startsWith(fence)) {
        code.push(line(i));
        i += 1;
      }
      i += 1; // closing fence (or EOF)
      const cls = lang === '' ? '' : ` class="language-${escapeHtml(lang)}"`;
      html += `<pre><code${cls}>${escapeHtml(code.join('\n'))}</code></pre>\n`;
      continue;
    }

    if (trimmed === '') {
      flushParagraph();
      i += 1;
      continue;
    }

    // Heading
    const level = headingLevel(trimmed);
    if (level !== null) {
      flushParagraph();
      const text = trimmed.slice(level).trim();
      html += `<h${String(level)}>${renderInline(text)}</h${String(level)}>\n`;
      i += 1;
      continue;
    }

    // Horizontal rule
    if (isRule(trimmed)) {
      flushParagraph();
      html += '<hr>\n';
      i += 1;
      continue;
    }

    // Blockquote
    if (trimmed.startsWith('>')) {
      flushParagraph();
      const quoted: string[] = [];
      while (i < lines.length && line(i).trim().startsWith('>')) {
        let t = line(i).trim().slice(1);
        if (t.startsWith(' ')) t = t.slice(1);
        quoted.push(t);
        i += 1;
      }
      html += `<blockquote>${renderMarkdown(quoted.join('\n'))}</blockquote>\n`;
      continue;
    }

    // Table: header row followed by a delimiter row
    if (trimmed.includes('|') && i + 1 < lines.length && isTableDelimiter(line(i + 1))) {
      flushParagraph();
      const header = cells(trimmed);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && line(i).includes('|') && line(i).trim() !== '') {
        rows.push(cells(line(i)));
        i += 1;
      }
      html += '<table><thead><tr>' + header.map((c) => `<th>${renderInline(c)}</th>`).join('');
      html += '</tr></thead><tbody>';
      for (const row of rows) {
        html += '<tr>' + row.map((c) => `<td>${renderInline(c)}</td>`).join('') + '</tr>';
      }
      html += '</tbody></table>\n';
      continue;
    }

    // Lists (flat; indentation inside a list continues the previous item)
    const marker = listMarker(current);
    if (marker !== null) {
      flushParagraph();
      const items: string[] = [];
      for (let m = listMarker(line(i)); i < lines.length && m?.ordered === marker.ordered;) {
        items.push(m.text);
        i += 1;
        m = i < lines.length ? listMarker(line(i)) : null;
      }
      const tag = marker.ordered ? 'ol' : 'ul';
      html +=
        `<${tag}>` + items.map((item) => `<li>${renderInline(item)}</li>`).join('') + `</${tag}>\n`;
      continue;
    }

    paragraph.push(trimmed);
    i += 1;
  }
  flushParagraph();
  return html;
}

/**
 * Swift `MarkdownHTMLRenderer.page`: a full page around the rendered body, themed for light and
 * dark. `colorScheme` follows the app's theme (Settings can override the system's). The CSP allows
 * no script, no frame, no form target and no connection; images load only over https.
 */
export function markdownPage(body: string, colorScheme: 'light' | 'dark' = 'light'): string {
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src https:; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'; object-src 'none'">
<meta name="color-scheme" content="${colorScheme}">
<style>
:root { color-scheme: ${colorScheme}; }
body { font: 14px/1.6 "Segoe UI Variable Text", "Segoe UI", sans-serif; margin: 0; padding: 16px 20px;
       color: CanvasText; background: Canvas; overflow-wrap: anywhere; }
h1,h2,h3,h4,h5,h6 { line-height: 1.25; margin: 1.2em 0 .5em; }
h1 { font-size: 1.7em; border-bottom: 1px solid rgba(128,128,128,.3); padding-bottom: .3em; }
h2 { font-size: 1.4em; border-bottom: 1px solid rgba(128,128,128,.2); padding-bottom: .2em; }
code { font: 12.5px "Cascadia Mono", Consolas, monospace; background: rgba(128,128,128,.18);
       padding: .1em .35em; border-radius: 4px; }
pre { background: rgba(128,128,128,.14); padding: 12px; border-radius: 6px; overflow-x: auto; }
pre code { background: none; padding: 0; }
blockquote { margin: 0 0 1em; padding: 0 1em; border-left: 3px solid rgba(128,128,128,.4); opacity: .85; }
table { border-collapse: collapse; } th, td { border: 1px solid rgba(128,128,128,.35); padding: 4px 10px; }
a { color: LinkText; } img { max-width: 100%; } hr { border: 0; border-top: 1px solid rgba(128,128,128,.3); }
</style></head><body>${body}</body></html>`;
}
