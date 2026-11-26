// Port of apps/purepoint-macos/purepoint-macos/Views/FilePane/MarkdownPreviewView.swift (WIN-8.6):
// read-only rendered Markdown. The content is converted by `services/markdownHtml.ts` (which escapes
// all text, so raw HTML shows as text) and shown in a sandboxed iframe: no `allow-scripts` (the
// Mac's `allowsContentJavaScript = false`), no forms, popups or top navigation, and the page's own
// CSP allows no script or connection. `allow-same-origin` only lets this view handle the page's
// link clicks, which never navigate the frame (Swift's `decidePolicyFor`, `.cancel`): http(s) and
// mailto links open outside the app through the opener; a relative link inside the document's
// folder opens that file in the pane; an anchor scrolls the preview.
//
// Difference from Swift: relative images are not loaded (WebView2 gives the frame no file base),
// and remote images load only over https.
import { useEffect, useRef } from 'react';
import { joinPath, parentPath, pathSeparator, relativePath } from '../../model/fileNode';
import { markdownPage, renderMarkdown } from '../../services/markdownHtml';
import { openExternal } from '../../services/opener';
import { useThemeStore } from '../../theme/themeStore';

export type MarkdownLinkAction =
  | { readonly kind: 'external'; readonly url: string }
  | { readonly kind: 'file'; readonly path: string }
  | { readonly kind: 'anchor'; readonly id: string }
  | { readonly kind: 'none' };

/** Resolve `.` and `..` in a path with the given separator. */
function normalizePath(path: string, separator: '/' | '\\'): string {
  const parts: string[] = [];
  for (const part of path.split(/[\\/]/)) {
    if (part === '.' || (part === '' && parts.length > 0)) continue;
    if (part === '..') {
      if (parts.length > 1) parts.pop();
      continue;
    }
    parts.push(part);
  }
  return parts.join(separator);
}

/** What a click on `href` in the preview does (Swift's navigation policy). */
export function markdownLinkAction(href: string, baseDirectory: string | null): MarkdownLinkAction {
  const lower = href.toLowerCase();
  if (lower.startsWith('http://') || lower.startsWith('https://') || lower.startsWith('mailto:')) {
    return { kind: 'external', url: href };
  }
  if (href.startsWith('#')) return { kind: 'anchor', id: decodeURIComponent(href.slice(1)) };
  // Anything else with a scheme was made inert (`#`) by the renderer.
  if (href === '' || baseDirectory === null || /^[a-z][a-z0-9+.-]*:/i.test(href)) {
    return { kind: 'none' };
  }
  let target = href.split('#')[0]?.split('?')[0] ?? '';
  try {
    target = decodeURIComponent(target);
  } catch {
    return { kind: 'none' };
  }
  if (target === '') return { kind: 'none' };
  const separator = pathSeparator(baseDirectory);
  const absolute = /^([a-z]:)?[\\/]/i.test(target) ? target : joinPath(baseDirectory, target);
  const path = normalizePath(absolute, separator);
  // Only a file inside the document's folder (Swift: `path.hasPrefix(base)`).
  const inside = relativePath(baseDirectory, path);
  return inside === null || inside === '' ? { kind: 'none' } : { kind: 'file', path };
}

export interface MarkdownPreviewViewProps {
  markdown: string;
  /** The document's path: relative links resolve against its folder. */
  documentPath: string;
  onOpenFile: (path: string) => void;
}

function appearance(): 'light' | 'dark' {
  return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
}

export function MarkdownPreviewView({
  markdown,
  documentPath,
  onOpenFile,
}: MarkdownPreviewViewProps) {
  const frame = useRef<HTMLIFrameElement>(null);
  // Re-render on a theme change, so the page follows the app's appearance.
  useThemeStore((state) => state.preference);
  const page = markdownPage(renderMarkdown(markdown), appearance());
  const baseDirectory = parentPath(documentPath);
  const onOpen = useRef(onOpenFile);
  useEffect(() => {
    onOpen.current = onOpenFile;
  });

  useEffect(() => {
    const element = frame.current;
    if (element === null) return;
    let attached: Document | null = null;
    const onClick = (event: MouseEvent) => {
      // The frame's nodes belong to its own realm, so no `instanceof Element` here.
      const target = event.target as Partial<Element> | null;
      const anchor = typeof target?.closest === 'function' ? target.closest('a') : null;
      if (anchor === null) return;
      // The frame never navigates.
      event.preventDefault();
      const action = markdownLinkAction(anchor.getAttribute('href') ?? '', baseDirectory);
      if (action.kind === 'external') openExternal(action.url);
      else if (action.kind === 'file') onOpen.current(action.path);
      else if (action.kind === 'anchor') {
        attached?.getElementById(action.id)?.scrollIntoView();
      }
    };
    const attach = () => {
      attached?.removeEventListener('click', onClick, true);
      attached = element.contentDocument;
      attached?.addEventListener('click', onClick, true);
    };
    attach();
    element.addEventListener('load', attach);
    return () => {
      element.removeEventListener('load', attach);
      attached?.removeEventListener('click', onClick, true);
    };
  }, [baseDirectory]);

  return (
    <iframe
      ref={frame}
      className="markdown-preview"
      title="Markdown preview"
      data-testid="markdown-preview"
      sandbox="allow-same-origin"
      referrerPolicy="no-referrer"
      srcDoc={page}
    />
  );
}
