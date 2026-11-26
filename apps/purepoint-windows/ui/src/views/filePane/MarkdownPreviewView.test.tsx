// WIN-8.6: the Markdown preview (Swift Views/FilePane/MarkdownPreviewView.swift): a sandboxed frame
// with no scripts, links that never navigate it (external ones go to the opener), and FilePaneView's
// Code / Preview picker.
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { memoryFileHost } from '../../services/fileHost.fixtures';
import { FileTabStore } from '../../state/fileTabStore';
import { stubScrollViewLayout } from '../../test/virtualLayout';
import { FilePaneView } from './FilePaneView';
import { markdownLinkAction, MarkdownPreviewView } from './MarkdownPreviewView';

stubScrollViewLayout('file-tree-outline', 240, 2000);

interface SpyGlobal {
  __PU_OPENER_SPY__?: string[];
}

afterEach(() => {
  delete (globalThis as SpyGlobal).__PU_OPENER_SPY__;
});

const ROOT = 'C:\\repo';
const DOC = `${ROOT}\\docs\\guide.md`;

describe('markdownLinkAction', () => {
  const base = `${ROOT}\\docs`;
  it('sends web and mail links outside the app', () => {
    expect(markdownLinkAction('https://example.com/a', base)).toEqual({
      kind: 'external',
      url: 'https://example.com/a',
    });
    expect(markdownLinkAction('mailto:a@b.c', base).kind).toBe('external');
  });

  it('opens relative links inside the document folder as files', () => {
    expect(markdownLinkAction('setup.md', base)).toEqual({
      kind: 'file',
      path: `${ROOT}\\docs\\setup.md`,
    });
    expect(markdownLinkAction('./sub/a%20b.md#part', base)).toEqual({
      kind: 'file',
      path: `${ROOT}\\docs\\sub\\a b.md`,
    });
    expect(markdownLinkAction('/srv/app/docs/x.md', '/srv/app/docs')).toEqual({
      kind: 'file',
      path: '/srv/app/docs/x.md',
    });
  });

  it('ignores links leaving the folder, inert anchors and other schemes', () => {
    expect(markdownLinkAction('../README.md', base).kind).toBe('none');
    expect(markdownLinkAction('C:\\Windows\\win.ini', base).kind).toBe('none');
    expect(markdownLinkAction('#', base)).toEqual({ kind: 'anchor', id: '' });
    expect(markdownLinkAction('#install', base)).toEqual({ kind: 'anchor', id: 'install' });
    expect(markdownLinkAction('file:///C:/x', base).kind).toBe('none');
    expect(markdownLinkAction('javascript:alert(1)', base).kind).toBe('none');
    expect(markdownLinkAction('', base).kind).toBe('none');
  });
});

describe('MarkdownPreviewView', () => {
  it('renders into a frame that cannot run script', () => {
    render(
      <MarkdownPreviewView
        markdown={'# Hi\n\n<script>alert(1)</script>'}
        documentPath={DOC}
        onOpenFile={() => undefined}
      />,
    );
    const frame = screen.getByTestId('markdown-preview');
    const sandbox = frame.getAttribute('sandbox') ?? '';
    expect(sandbox.split(/\s+/)).toEqual(['allow-same-origin']);
    const page = frame.getAttribute('srcdoc') ?? '';
    expect(page).toContain('<h1>Hi</h1>');
    expect(page).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
    expect(page).not.toContain('<script>');
    expect(page).toContain("default-src 'none'");
  });

  it('hands link clicks to the opener or the pane, never navigating the frame', () => {
    const opened: string[] = [];
    (globalThis as SpyGlobal).__PU_OPENER_SPY__ = opened;
    const onOpenFile = vi.fn();
    render(<MarkdownPreviewView markdown="x" documentPath={DOC} onOpenFile={onOpenFile} />);
    const frame = screen.getByTestId<HTMLIFrameElement>('markdown-preview');
    const inner = frame.contentDocument;
    if (inner === null) throw new Error('no frame document');
    inner.body.innerHTML =
      '<a id="web" href="https://example.com/"><span>web</span></a><a id="doc" href="setup.md">doc</a>';
    const click = (id: string) => {
      const event = new MouseEvent('click', { bubbles: true, cancelable: true });
      const target = inner.getElementById(id);
      (id === 'web' ? target?.firstElementChild : target)?.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(click('web')).toBe(true);
    expect(opened).toEqual(['https://example.com/']);
    expect(click('doc')).toBe(true);
    expect(onOpenFile).toHaveBeenCalledWith(`${ROOT}\\docs\\setup.md`);
  });
});

describe('FilePaneView Markdown', () => {
  it('previews a Markdown file first and toggles to its code', async () => {
    const host = memoryFileHost({ [DOC]: '# Guide\n\n[site](https://example.com)\n' });
    const session = new FileTabStore().session('ws', 1, DOC);
    render(
      <FilePaneView
        rootPath={ROOT}
        host={host}
        initialPath={DOC}
        session={session}
        isFocused={false}
        onPathChange={() => undefined}
      />,
    );
    const preview = await screen.findByTestId('markdown-preview');
    expect(preview.getAttribute('srcdoc')).toContain('<a href="https://example.com">site</a>');
    expect(screen.getByRole('radio', { name: 'Preview' })).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByRole('radio', { name: 'Code' }));
    await waitFor(() => {
      expect(document.querySelector('.cm-content')?.textContent).toContain('# Guide');
    });
    expect(screen.queryByTestId('markdown-preview')).toBeNull();
    expect(session.showPreview).toBe(false);
    act(() => {
      fireEvent.click(screen.getByRole('radio', { name: 'Preview' }));
    });
    expect(screen.getByTestId('markdown-preview')).toBeInTheDocument();
  });

  it('offers no picker for other files', async () => {
    const path = `${ROOT}\\a.ts`;
    const host = memoryFileHost({ [path]: 'let a = 1;' });
    render(
      <FilePaneView
        rootPath={ROOT}
        host={host}
        initialPath={path}
        session={new FileTabStore().session('ws', 1, path)}
        isFocused={false}
        onPathChange={() => undefined}
      />,
    );
    await screen.findByTestId('editor');
    expect(screen.queryByRole('radio', { name: 'Preview' })).toBeNull();
  });
});
