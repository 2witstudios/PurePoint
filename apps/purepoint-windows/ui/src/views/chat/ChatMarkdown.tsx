// Port of apps/shared/ChatMarkdown/ChatMarkdownView.swift (WIN-9.6).
import { type CSSProperties, type ReactNode, useMemo, useState } from 'react';
import {
  type ChatInline,
  type ChatMarkdownBlock,
  type ChatTableAlignment,
  document as prepareDocument,
  parseInline,
} from '../../model/chatMarkdown.ts';
import { openExternal } from '../../services/opener.ts';
import './ChatMarkdown.css';

/** Selectable Point Guard chat Markdown with the shared ChatMarkdown rules. */
export function ChatMarkdown({ text }: { text: string }) {
  const prepared = useMemo(() => prepareDocument(text), [text]);
  const inline = (source: string) => prepared.inlineContent.get(source) ?? parseInline(source);
  return (
    <div className="chat-markdown">
      <Blocks blocks={prepared.blocks} inline={inline} />
    </div>
  );
}

/**
 * Inline-only Markdown with whitespace preserved (Foundation's `.inlineOnlyPreservingWhitespace`),
 * as the project channel renders message text (WIN-8.10).
 */
export function ChatInlineMarkdown({ text, className }: { text: string; className?: string }) {
  const nodes = useMemo(() => parseInline(text), [text]);
  return <span className={className}>{renderInline(nodes)}</span>;
}

type InlineLookup = (source: string) => ChatInline[];

function Blocks({ blocks, inline }: { blocks: ChatMarkdownBlock[]; inline: InlineLookup }) {
  return (
    <div className="chat-markdown-blocks">
      {blocks.map((block, index) => (
        // Blocks have no identity beyond their position, as in the Swift view.
        <Block key={index} block={block} inline={inline} />
      ))}
    </div>
  );
}

function Block({ block, inline }: { block: ChatMarkdownBlock; inline: InlineLookup }): ReactNode {
  switch (block.kind) {
    case 'paragraph':
      return <p className="chat-markdown-paragraph">{renderInline(inline(block.text))}</p>;
    case 'heading': {
      const Tag = `h${String(Math.min(block.level, 6))}` as 'h1';
      return (
        <Tag className={`chat-markdown-heading chat-markdown-heading-${String(block.level)}`}>
          {renderInline(inline(block.text))}
        </Tag>
      );
    }
    case 'listItem':
      return (
        <div
          className="chat-markdown-list-item"
          style={{ '--chat-markdown-depth': block.depth } as CSSProperties}
        >
          {block.checked === null ? (
            <span className="chat-markdown-marker" aria-hidden="true">
              {block.marker}
            </span>
          ) : (
            <span
              className="chat-markdown-marker chat-markdown-check"
              role="img"
              aria-label={block.checked ? 'Completed' : 'Unchecked'}
              data-checked={block.checked}
            >
              {block.checked ? '☑' : '☐'}
            </span>
          )}
          <div className="chat-markdown-paragraph">{renderInline(inline(block.text))}</div>
        </div>
      );
    case 'listContinuation':
      return (
        <div
          className="chat-markdown-list-continuation"
          style={{ '--chat-markdown-depth': block.depth } as CSSProperties}
        >
          <Blocks blocks={block.blocks} inline={inline} />
        </div>
      );
    case 'quote':
      return (
        <blockquote className="chat-markdown-quote">
          <Blocks blocks={block.blocks} inline={inline} />
        </blockquote>
      );
    case 'code':
      return <ChatMarkdownCode language={block.language} code={block.text} />;
    case 'rule':
      return <hr className="chat-markdown-rule" />;
    case 'table':
      return (
        <div className="chat-markdown-table-scroll">
          <table className="chat-markdown-table">
            <thead>
              <TableRow
                cells={block.headers}
                alignments={block.alignments}
                header
                inline={inline}
              />
            </thead>
            <tbody>
              {block.rows.map((row, index) => (
                <TableRow
                  key={index}
                  cells={row}
                  alignments={block.alignments}
                  header={false}
                  inline={inline}
                />
              ))}
            </tbody>
          </table>
        </div>
      );
    case 'image':
      return <ChatMarkdownImage alt={block.alt} url={block.url} />;
  }
}

function TableRow({
  cells,
  alignments,
  header,
  inline,
}: {
  cells: string[];
  alignments: ChatTableAlignment[];
  header: boolean;
  inline: InlineLookup;
}) {
  const Cell = header ? 'th' : 'td';
  return (
    <tr>
      {cells.map((cell, index) => (
        <Cell key={index} className={`chat-markdown-align-${alignments[index] ?? 'leading'}`}>
          {renderInline(inline(cell))}
        </Cell>
      ))}
    </tr>
  );
}

function renderInline(nodes: ChatInline[]): ReactNode[] {
  return nodes.map((node, index) => {
    switch (node.kind) {
      case 'text':
        return node.text;
      case 'code':
        return (
          <code key={index} className="chat-markdown-inline-code">
            {node.text}
          </code>
        );
      case 'emphasis':
        return <em key={index}>{renderInline(node.children)}</em>;
      case 'strong':
        return <strong key={index}>{renderInline(node.children)}</strong>;
      case 'strikethrough':
        return <del key={index}>{renderInline(node.children)}</del>;
      case 'link':
        return node.href === null ? (
          <span key={index}>{renderInline(node.children)}</span>
        ) : (
          <ExternalLink key={index} href={node.href} className="chat-markdown-link">
            {renderInline(node.children)}
          </ExternalLink>
        );
    }
  });
}

function ExternalLink({
  href,
  className,
  children,
}: {
  href: string;
  className: string;
  children: ReactNode;
}) {
  return (
    <a
      href={href}
      className={className}
      rel="noopener noreferrer"
      onClick={(event) => {
        event.preventDefault();
        openExternal(href);
      }}
    >
      {children}
    </a>
  );
}

type ImagePhase = 'idle' | 'loading' | 'loaded' | 'failed';

/** Assistant content never loads images on its own: the reader asks for each one. */
function ChatMarkdownImage({ alt, url }: { alt: string; url: string }) {
  const [phase, setPhase] = useState<ImagePhase>('idle');
  const host = hostOf(url);
  return (
    <figure className="chat-markdown-image">
      {phase === 'idle' ? (
        <>
          <button
            type="button"
            className="chat-markdown-image-load"
            title={`Requests an image from ${host ?? 'the linked host'}`}
            onClick={() => {
              setPhase('loading');
            }}
          >
            Load image
          </button>
          {host !== null && <span className="chat-markdown-caption">{host}</span>}
        </>
      ) : phase === 'failed' ? (
        <span className="chat-markdown-caption">Image unavailable</span>
      ) : (
        <img
          className="chat-markdown-image-content"
          src={url}
          alt={alt === '' ? 'Image' : alt}
          data-phase={phase}
          onLoad={() => {
            setPhase('loaded');
          }}
          onError={() => {
            setPhase('failed');
          }}
        />
      )}
      {alt !== '' && <figcaption className="chat-markdown-caption">{alt}</figcaption>}
      <ExternalLink href={url} className="chat-markdown-link chat-markdown-caption">
        Open image
      </ExternalLink>
    </figure>
  );
}

function hostOf(url: string): string | null {
  try {
    const host = new URL(url).hostname;
    return host === '' ? null : host;
  } catch {
    return null;
  }
}

/** A code panel with its language and a Copy button. */
export function ChatMarkdownCode({ language, code }: { language: string; code: string }) {
  const [copiedCode, setCopiedCode] = useState<string | null>(null);
  // Copied resets when the streamed code changes underneath it.
  const copied = copiedCode === code;
  return (
    <div className="chat-markdown-code">
      <div className="chat-markdown-code-header">
        <span className="chat-markdown-code-language">{language === '' ? 'Code' : language}</span>
        <button
          type="button"
          className="chat-markdown-code-copy"
          aria-label={copied ? 'Code copied' : 'Copy code'}
          onClick={() => {
            navigator.clipboard.writeText(code).then(
              () => {
                setCopiedCode(code);
              },
              (error: unknown) => {
                console.error('PurePoint: copying code failed', error);
              },
            );
          }}
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre className="chat-markdown-code-body">
        <code>{code}</code>
      </pre>
    </div>
  );
}
