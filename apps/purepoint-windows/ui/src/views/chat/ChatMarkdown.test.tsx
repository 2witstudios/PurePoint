import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { chatMarkdownFixtures } from '../../model/chatMarkdownFixtures.ts';
import { openExternal } from '../../services/opener.ts';
import { ChatMarkdown } from './ChatMarkdown.tsx';

vi.mock('../../services/opener.ts', () => ({ openExternal: vi.fn() }));

afterEach(() => {
  vi.mocked(openExternal).mockClear();
});

describe('ChatMarkdown', () => {
  it('renders the RenderPreview fixture with headings, lists, quote, table and code', () => {
    const { container } = render(<ChatMarkdown text={chatMarkdownFixtures.renderPreview} />);
    expect(
      screen.getByRole('heading', { level: 2, name: 'A clearer view of the work' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 3, name: 'Next steps' })).toBeInTheDocument();
    expect(screen.getByText('release plan').tagName).toBe('STRONG');
    expect(screen.getByText('pu status').tagName).toBe('CODE');
    expect(screen.getByRole('img', { name: 'Completed' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Unchecked' })).toBeInTheDocument();
    expect(container.querySelector('blockquote')).toHaveTextContent(
      'Keep the conversation readable.',
    );
    const table = screen.getByRole('table');
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((cell) => cell.textContent),
    ).toEqual(['Platform', 'Rendering', 'Status']);
    expect(within(table).getAllByRole('columnheader')[2]).toHaveClass(
      'chat-markdown-align-trailing',
    );
    expect(container.querySelector('pre code')).toHaveTextContent(
      'let conversation = PointGuard()',
    );
    // Line breaks inside a paragraph are preserved, not reflowed.
    const paragraphs = Array.from(container.querySelectorAll('p'), (p) => p.textContent);
    expect(paragraphs).toContain(
      "Here's the release plan, with pu status ready to run.\nThis line stays on its own line.",
    );
  });

  it('renders an unterminated streaming fence as code', () => {
    const { container } = render(<ChatMarkdown text={'Working:\n```sh\npu status'} />);
    expect(container.querySelector('pre code')?.textContent).toBe('pu status');
    expect(screen.getByText('sh')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy code' })).toBeInTheDocument();
  });

  it('shows raw HTML as text', () => {
    const { container } = render(
      <ChatMarkdown
        text={'<img src=x onerror=alert(1)> <b>bold</b>\n\n<script>alert(1)</script>'}
      />,
    );
    expect(container.querySelector('img, b, script')).toBeNull();
    expect(screen.getByText('<img src=x onerror=alert(1)> <b>bold</b>')).toBeInTheDocument();
    expect(screen.getByText('<script>alert(1)</script>')).toBeInTheDocument();
  });

  it('opens only http, https and mailto links, externally', () => {
    render(
      <ChatMarkdown
        text={
          '[safe](https://example.com) [mail](mailto:dev@example.com) [js](javascript:alert(1)) [file](file:///tmp/private)'
        }
      />,
    );
    expect(screen.getAllByRole('link').map((link) => link.textContent)).toEqual(['safe', 'mail']);
    expect(screen.getByText('js').closest('a')).toBeNull();
    expect(screen.getByText('file').closest('a')).toBeNull();
    fireEvent.click(screen.getByRole('link', { name: 'safe' }));
    expect(openExternal).toHaveBeenCalledWith('https://example.com');
  });

  it('loads an image only after Load image is clicked', () => {
    const { container } = render(
      <ChatMarkdown text="![Release preview](https://example.com/preview.png)" />,
    );
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText('example.com')).toBeInTheDocument();
    expect(screen.getByText('Release preview')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Load image' }));
    const picture = screen.getByRole('img', { name: 'Release preview' });
    expect(picture).toHaveAttribute('src', 'https://example.com/preview.png');
    fireEvent.error(picture);
    expect(screen.getByText('Image unavailable')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('link', { name: 'Open image' }));
    expect(openExternal).toHaveBeenCalledWith('https://example.com/preview.png');
  });

  it('caps tables at 100 rows and 12 columns with the notices', () => {
    const header = Array.from({ length: 13 }, (_, index) => `H${String(index)}`).join(' | ');
    const delimiter = Array<string>(13).fill('---').join(' | ');
    const row = Array<string>(13).fill('c').join(' | ');
    render(<ChatMarkdown text={[header, delimiter, ...Array<string>(150).fill(row)].join('\n')} />);
    const table = screen.getByRole('table');
    expect(within(table).getAllByRole('columnheader')).toHaveLength(12);
    expect(within(table).getAllByRole('row')).toHaveLength(1 + 99);
    expect(
      screen.getByText('Table preview limited to 100 rows and 12 columns.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Markdown preview limited to 256 blocks and 1,200 table cells.'),
    ).toBeInTheDocument();
  });

  it('copies code and shows Copied', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    render(<ChatMarkdown text={'```swift\nlet x = 1\n```'} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy code' }));
    expect(writeText).toHaveBeenCalledWith('let x = 1');
    expect(await screen.findByRole('button', { name: 'Code copied' })).toHaveTextContent('Copied');
  });
});
