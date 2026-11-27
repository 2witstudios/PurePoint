// Port of apps/purepoint-macos/purepoint-macosTests/MarkdownHTMLRendererTests.swift (WIN-8.6): every
// Swift case, then the escaping guarantees the preview relies on.
import { describe, expect, it } from 'vitest';
import {
  escapeHtml,
  markdownPage,
  renderInline,
  renderMarkdown,
  safeImageUrl,
  safeUrl,
} from './markdownHtml';

describe('MarkdownHTMLRenderer (Swift cases)', () => {
  it('renders heading and paragraph', () => {
    const html = renderMarkdown('# Title\n\nhello **bold** and `code`');
    expect(html).toContain('<h1>Title</h1>');
    expect(html).toContain('<strong>bold</strong>');
    expect(html).toContain('<code>code</code>');
  });

  it('escapes raw HTML', () => {
    const html = renderMarkdown('<script>alert(1)</script>');
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
  });

  it('neutralizes javascript links', () => {
    const html = renderMarkdown('[x](javascript:alert(1))');
    expect(html).not.toContain('javascript:');
  });

  it('keeps fenced code verbatim', () => {
    const html = renderMarkdown('```swift\nlet a = **1**\n```');
    expect(html).toContain('<pre><code class="language-swift">let a = **1**</code></pre>');
  });

  it('renders lists and tables', () => {
    const html = renderMarkdown('- a\n- b\n\n| h |\n|---|\n| c |');
    expect(html).toContain('<ul><li>a</li><li>b</li></ul>');
    expect(html).toContain('<th>h</th>');
    expect(html).toContain('<td>c</td>');
  });

  it('does not emphasize code span contents', () => {
    const html = renderMarkdown('`*x*`');
    expect(html).toContain('<code>*x*</code>');
  });
});

describe('markdownHtml escaping', () => {
  it('escapes the four HTML-significant characters', () => {
    expect(escapeHtml('<a href="x">&</a>')).toBe('&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;');
  });

  it('escapes raw HTML everywhere text appears', () => {
    const attack = '<img src=x onerror=alert(1)>';
    for (const markdown of [
      attack,
      `# ${attack}`,
      `> ${attack}`,
      `- ${attack}`,
      `| ${attack} |\n|---|\n| ${attack} |`,
      `\`\`\`\n${attack}\n\`\`\``,
      `\`${attack}\``,
      `**${attack}**`,
    ]) {
      const html = renderMarkdown(markdown);
      expect(html, markdown).not.toContain('<img src=x');
      expect(html, markdown).toContain('&lt;img src=x onerror=alert(1)&gt;');
    }
  });

  it('keeps quotes from breaking out of an attribute', () => {
    const html = renderInline('[x](https://a.example/"onmouseover="alert(1))');
    expect(html).not.toMatch(/href="[^"]*"onmouseover/);
    expect(html).toContain('&quot;');
    const image = renderInline('![a" onerror="x](https://a.example/i.png)');
    expect(image).toBe('<img alt="a&quot; onerror=&quot;x" src="https://a.example/i.png">');
  });

  it('allows only http(s), mailto and scheme-less URLs', () => {
    expect(safeUrl('https://example.com')).toBe('https://example.com');
    expect(safeUrl('HTTP://example.com')).toBe('HTTP://example.com');
    expect(safeUrl('mailto:a@b.c')).toBe('mailto:a@b.c');
    expect(safeUrl('docs/guide.md')).toBe('docs/guide.md');
    expect(safeUrl('#section')).toBe('#section');
    expect(safeUrl('./a:b.md')).toBe('./a:b.md');
    for (const bad of [
      'javascript:alert(1)',
      'JavaScript:x',
      'data:text/html,x',
      'file:///C:/x',
      'vbscript:x',
    ]) {
      expect(safeUrl(bad), bad).toBe('#');
    }
    expect(renderInline('![x](javascript:alert(1))')).toBe('x)');
  });

  it('loads preview images over https only, showing the alt text otherwise', () => {
    expect(safeImageUrl('https://a.example/i.png')).toBe('https://a.example/i.png');
    for (const bad of [
      'http://a.example/i.png',
      'file://server/share/i.png',
      'file:///C:/Users/me/i.png',
      String.raw`\\server\share\i.png`,
      '//server/share/i.png',
      'docs/i.png',
      'data:image/png;base64,AAAA',
      'javascript:alert(1)',
      String.raw`https:\\server\i.png`,
    ]) {
      expect(safeImageUrl(bad), bad).toBeNull();
      expect(renderInline(`![alt](${bad})`), bad).not.toContain('<img');
    }
    expect(renderInline('![alt](file://server/share/i.png)')).toBe('alt');
    expect(markdownPage('')).toContain('img-src https:;');
  });

  it('applies inline markup', () => {
    expect(renderInline('*em* _em_ ~~del~~ __strong__')).toBe(
      '<em>em</em> <em>em</em> <del>del</del> <strong>strong</strong>',
    );
    // Intra-word markers are not emphasis.
    expect(renderInline('snake_case_name and 2*3*4')).toBe('snake_case_name and 2*3*4');
    expect(renderInline('[docs](docs/a.md)')).toBe('<a href="docs/a.md">docs</a>');
  });

  it('renders the remaining blocks', () => {
    expect(renderMarkdown('---')).toBe('<hr>\n');
    expect(renderMarkdown('1. one\n2) two')).toBe('<ol><li>one</li><li>two</li></ol>\n');
    expect(renderMarkdown('> quoted\n> more')).toBe(
      '<blockquote><p>quoted more</p>\n</blockquote>\n',
    );
    expect(renderMarkdown('####### seven')).toBe('<p>####### seven</p>\n');
    expect(renderMarkdown('a\r\nb')).toBe('<p>a b</p>\n');
    expect(renderMarkdown('~~~\nx\n')).toBe('<pre><code>x\n</code></pre>\n');
  });

  it('wraps the body in a page that allows no script', () => {
    const page = markdownPage('<p>x</p>', 'dark');
    expect(page).toContain("default-src 'none'");
    expect(page).not.toContain('script-src');
    expect(page).toContain('color-scheme: dark');
    expect(page).toContain('<body><p>x</p></body>');
  });
});
