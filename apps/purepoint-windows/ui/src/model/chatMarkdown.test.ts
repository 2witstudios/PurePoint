// Port of apps/shared/ChatMarkdown/ChatMarkdownTests.swift (run there by check-markdown.py).
// Case names match the Swift test methods.
import { describe, expect, it } from 'vitest';
import {
  type ChatMarkdownBlock,
  DOCUMENT_LIMIT_NOTICE,
  TABLE_LIMIT_NOTICE,
  block,
  cells,
  document,
  inlineText,
  parse,
  parseInline,
  prepareChatMarkdown,
} from './chatMarkdown.ts';
import { chatMarkdownFixtures } from './chatMarkdownFixtures.ts';

const { paragraph, heading, listItem, listContinuation, quote, code, table, rule, image } = block;

describe('ChatMarkdownTests', () => {
  it('testGivenMultilineReplyShouldPreserveLinesAndSeparateParagraphs', () => {
    expect(parse('First\nsecond\n\nThird')).toEqual([
      paragraph('First\nsecond'),
      paragraph('Third'),
    ]);
  });

  it('testGivenStructuredReplyShouldRenderHeadingsTasksAndNestedLists', () => {
    expect(parse('## Plan\n\n3. Ship\n   - [x] Desktop\n   - [ ] Phone')).toEqual([
      heading(2, 'Plan'),
      listItem(0, '3.', null, 'Ship'),
      listItem(1, '•', true, 'Desktop'),
      listItem(1, '•', false, 'Phone'),
    ]);
  });

  it('testGivenStreamingFenceShouldRetainCodeAndRespectFenceLength', () => {
    expect(parse('````swift\n```\nlet x = 1')).toEqual([code('swift', '```\nlet x = 1')]);
    expect(parse('~~~sh\necho hi\n~~~\nDone')).toEqual([code('sh', 'echo hi'), paragraph('Done')]);
  });

  it('testGivenTableShouldKeepEscapedPipesAndCodeCells', () => {
    expect(parse('| Name | Value |\n| :--- | ---: |\n| a\\|b | `x|y` |')).toEqual([
      table(['Name', 'Value'], ['leading', 'trailing'], [['a|b', '`x|y`']]),
    ]);
  });

  it('testGivenWindowsLineEndingsShouldRenderParagraphsNormally', () => {
    expect(parse('First\r\nsecond\r\n\r\nThird')).toEqual([
      paragraph('First\nsecond'),
      paragraph('Third'),
    ]);
  });

  it('testGivenEmptyCodeFenceShouldRetainItsCopyablePanel', () => {
    expect(parse('```swift\n```')).toEqual([code('swift', '')]);
  });

  it('testGivenShortTableRowsShouldRetainColumnPositions', () => {
    expect(parse('A | B | C\n--- | --- | ---\nfirst | second')).toEqual([
      table(['A', 'B', 'C'], ['leading', 'leading', 'leading'], [['first', 'second', '']]),
    ]);
  });

  it('testGivenImageReplyShouldExposeAnAccessibleRemoteImage', () => {
    expect(parse('![Release preview](https://example.com/preview.png)')).toEqual([
      image('Release preview', 'https://example.com/preview.png'),
    ]);
    expect(parse('![private](file:///tmp/private.png)')).toEqual([
      paragraph('![private](file:///tmp/private.png)'),
    ]);
  });

  it('testGivenListWithCodeShouldRenderItsNestedPanelAndKeepCodeWhitespace', () => {
    expect(
      parse('1. Run this:\n   ```swift\n   if ready {\n       run()\n   }\n   ```\n2. Continue'),
    ).toEqual([
      listItem(0, '1.', null, 'Run this:'),
      listContinuation(1, [code('swift', 'if ready {\n    run()\n}')]),
      listItem(0, '2.', null, 'Continue'),
    ]);
  });

  it('testGivenListWithQuotedAdviceShouldRenderItsQuote', () => {
    expect(parse('- Check this:\n  > **Keep it readable.**\n- Done')).toEqual([
      listItem(0, '•', null, 'Check this:'),
      listContinuation(1, [quote([paragraph('**Keep it readable.**')])]),
      listItem(0, '•', null, 'Done'),
    ]);
  });

  it('testGivenQuotedReplyShouldKeepItsStructure', () => {
    expect(parse('> ## Note\n> Text\n\n---')).toEqual([
      quote([heading(2, 'Note'), paragraph('Text')]),
      rule(),
    ]);
  });

  it('testGivenIndentedListContinuationShouldKeepItWithItsItem', () => {
    expect(parse('- First\n  continued\n- Second')).toEqual([
      listItem(0, '•', null, 'First\ncontinued'),
      listItem(0, '•', null, 'Second'),
    ]);
  });

  it('testGivenDeepQuotesShouldBoundNestingAndRetainRemainingText', () => {
    let blocks = parse('>'.repeat(3_000) + ' text');
    let depth = 0;
    for (let first = blocks[0]; first?.kind === 'quote'; first = blocks[0]) {
      depth += 1;
      blocks = first.blocks;
    }
    expect(depth).toBe(32);
    expect(blocks).toEqual([paragraph('>'.repeat(2_968) + ' text')]);
  });

  it('testGivenTableCodeSpanShouldMatchDelimiterLength', () => {
    expect(parse('A | B\n--- | ---\n``x|y`` | end')).toEqual([
      table(['A', 'B'], ['leading', 'leading'], [['``x|y``', 'end']]),
    ]);
    expect(cells('``x`|y`` | end')).toEqual(['``x`|y``', 'end']);
    expect(cells('`unfinished | end')).toEqual(['`unfinished', 'end']);
  });

  it('testGivenCancelledParseShouldDiscardObsoleteReply', () => {
    const result = parse('Long obsolete reply\n'.repeat(10_000), { isCancelled: () => true });
    expect(result).toEqual([]);
  });

  it('testGivenDocumentShouldPrepareInlineContentForNestedBlocksAndTables', () => {
    const prepared = document(
      '# **Heading**\n> - *Nested*\n\nA | B\n--- | ---\n`code` | [safe](https://example.com)\n\n[unsafe](file:///tmp/private)',
    );
    const content = (source: string) => {
      const nodes = prepared.inlineContent.get(source);
      expect(nodes, source).toBeDefined();
      return nodes ?? [];
    };
    expect(inlineText(content('**Heading**'))).toBe('Heading');
    expect(inlineText(content('*Nested*'))).toBe('Nested');
    expect(inlineText(content('`code`'))).toBe('code');
    expect(content('[safe](https://example.com)')[0]).toMatchObject({
      kind: 'link',
      href: 'https://example.com',
    });
    expect(content('[unsafe](file:///tmp/private)')[0]).toMatchObject({ kind: 'link', href: null });
  });

  it('testGivenLargeTableShouldBoundNativeGridAndExplainTruncation', () => {
    const columns = Array.from({ length: 13 }, (_, index) => index + 1);
    const header = columns.map((n) => `Column ${String(n)}`).join(' | ');
    const delimiter = columns.map(() => '---').join(' | ');
    const row = columns.map((n) => `Cell ${String(n)}`).join(' | ');
    const blocks = parse([header, delimiter, ...Array<string>(101).fill(row)].join('\n'));
    const first = blocks[0];
    if (first?.kind !== 'table') throw new Error('Expected table');
    expect(first.headers).toHaveLength(12);
    expect(first.alignments).toHaveLength(12);
    expect(first.rows).toHaveLength(99);
    expect(first.rows.every((cellsInRow) => cellsInRow.length === 12)).toBe(true);
    expect(first.rows.at(-1)?.at(-1)).toBe('Cell 12');
    expect(blocks).toContainEqual(paragraph(TABLE_LIMIT_NOTICE));
    expect(TABLE_LIMIT_NOTICE).toBe('Table preview limited to 100 rows and 12 columns.');
    expect(blocks.at(-1)).toEqual(
      paragraph('Markdown preview limited to 256 blocks and 1,200 table cells.'),
    );
  });

  it('testGivenCancelledQueuedPreparationShouldSkipInlineParsing', () => {
    const controller = new AbortController();
    controller.abort();
    const result = prepareChatMarkdown('**obsolete**', controller.signal);
    expect(result.blocks).toEqual([]);
    expect(result.inlineContent.size).toBe(0);
  });

  it('testGivenManyBlocksShouldBoundTheCompleteDocumentAndExplainTruncation', () => {
    const blocks = parse('---\n'.repeat(10_000));
    expect(blocks).toHaveLength(257);
    expect(blocks.at(-1)).toEqual(paragraph(DOCUMENT_LIMIT_NOTICE));
    const nested = parse('> ---\n'.repeat(10_000));
    const first = nested[0];
    if (first?.kind !== 'quote') throw new Error('Expected quote');
    expect(first.blocks).toHaveLength(255);
    expect(nested.at(-1)).toEqual(blocks.at(-1));
  });

  it('testGivenUnmatchedDelimiterRunsShouldRetainTableCellSeparators', () => {
    const cell = Array.from({ length: 1_000 }, (_, index) => '`'.repeat(index + 1) + 'x').join('');
    expect(cells(cell + ' | end')).toEqual([cell, 'end']);
  });

  it('testGivenRepeatedOrderedMarkersShouldNumberConsecutiveItemsByDepth', () => {
    const blocks = parse(
      '3. First\n   1. Nested A\n   1. Nested B\n1. Second\n\n1. Third\n\nNew list:\n7. Restart',
    );
    const markers = blocks.flatMap((item) => (item.kind === 'listItem' ? [item.marker] : []));
    expect(markers).toEqual(['3.', '1.', '2.', '4.', '5.', '7.']);
  });

  it('testGivenSpacedRuleShouldRestartOrderedListNumbering', () => {
    for (const spaced of ['* * *', '- - -']) {
      expect(parse('1. Before\n\n' + spaced + '\n\n1. After')).toEqual([
        listItem(0, '1.', null, 'Before'),
        rule(),
        listItem(0, '1.', null, 'After'),
      ]);
    }
  });

  it('testGivenExactBlockBudgetShouldAvoidFalseTruncationAndCountContainers', () => {
    const exact = parse('---\n'.repeat(256));
    expect(exact).toHaveLength(256);
    expect(exact.every((item) => item.kind === 'rule')).toBe(true);
    const blocks = parse('---\n'.repeat(253) + '- Item\n  > Advice\nAfter');
    expect(blocks).toHaveLength(256);
    expect(blocks.at(-1)).toEqual(paragraph(DOCUMENT_LIMIT_NOTICE));
  });

  it('testGivenTruncatedTableShouldRetainFollowingConclusion', () => {
    const header = Array<string>(12).fill('Column').join(' | ');
    const delimiter = Array<string>(12).fill('---').join(' | ');
    const row = Array<string>(12).fill('Cell').join(' | ');
    const source = [
      header,
      delimiter,
      ...Array<string>(100).fill(row),
      '',
      'Important conclusion.',
      '',
      header,
      delimiter,
      row,
      '',
      'Final advice.',
    ].join('\n');
    const blocks = parse(source);
    expect(blocks).toContainEqual(paragraph('Important conclusion.'));
    expect(blocks).toContainEqual(paragraph('Final advice.'));
    expect(blocks.at(-1)).toEqual(paragraph(DOCUMENT_LIMIT_NOTICE));
  });

  it('testGivenFinalListItemShouldMergeProseWithoutAdditionalBlocks', () => {
    const blocks = parse('---\n'.repeat(255) + '- Item\n  continued');
    expect(blocks).toHaveLength(256);
    expect(blocks.at(-1)).toEqual(listItem(0, '•', null, 'Item\ncontinued'));
  });
});

// The sources rendered by RenderPreview.swift, DesktopChatPreview.swift and PhonePreview.swift.
describe('ChatMarkdown render fixtures', () => {
  for (const [name, source] of Object.entries(chatMarkdownFixtures)) {
    it(`parses the ${name} fixture`, () => {
      expect(parse(source)).toMatchSnapshot();
    });
  }

  it('parses the RenderPreview overview into the Swift block structure', () => {
    const kinds = parse(chatMarkdownFixtures.renderPreview).map((item) => item.kind);
    expect(kinds).toEqual([
      'heading',
      'paragraph',
      'heading',
      'listItem',
      'listItem',
      'listItem',
      'listItem',
      'quote',
      'table',
      'code',
      'paragraph',
    ] satisfies ChatMarkdownBlock['kind'][]);
  });
});

describe('parseInline', () => {
  it('reads emphasis, strong, code, strikethrough and nesting', () => {
    expect(parseInline('Here is **bold _and italic_** with `pu status` and ~~old~~.')).toEqual([
      { kind: 'text', text: 'Here is ' },
      {
        kind: 'strong',
        children: [
          { kind: 'text', text: 'bold ' },
          { kind: 'emphasis', children: [{ kind: 'text', text: 'and italic' }] },
        ],
      },
      { kind: 'text', text: ' with ' },
      { kind: 'code', text: 'pu status' },
      { kind: 'text', text: ' and ' },
      { kind: 'strikethrough', children: [{ kind: 'text', text: 'old' }] },
      { kind: 'text', text: '.' },
    ]);
  });

  it('keeps raw HTML as text', () => {
    expect(parseInline('<b>bold</b> <script>alert(1)</script>')).toEqual([
      { kind: 'text', text: '<b>bold</b> <script>alert(1)</script>' },
    ]);
  });

  it('activates only http, https and mailto links', () => {
    const safe = (source: string) => {
      const first = parseInline(source)[0];
      return first?.kind === 'link' ? first.href : undefined;
    };
    expect(safe('[a](https://example.com/a_(b))')).toBe('https://example.com/a_(b)');
    expect(safe('[a](HTTP://example.com)')).toBe('HTTP://example.com');
    expect(safe('[mail](mailto:dev@example.com)')).toBe('mailto:dev@example.com');
    expect(safe('<https://example.com>')).toBe('https://example.com');
    expect(safe('<dev@example.com>')).toBe('mailto:dev@example.com');
    expect(safe('[js](javascript:alert(1))')).toBeNull();
    expect(safe('[file](file:///etc/passwd)')).toBeNull();
    expect(safe('[relative](notes.md)')).toBeNull();
  });

  it('keeps unmatched delimiters, escapes and line breaks literally', () => {
    expect(inlineText(parseInline('2 * 3 *4 \\*not\\* [x] a_b_c\nnext'))).toBe(
      '2 * 3 *4 *not* [x] a_b_c\nnext',
    );
  });

  it('shows the alt text of an image inside a line without loading it', () => {
    expect(parseInline('See ![chart](https://example.com/c.png) here')).toEqual([
      { kind: 'text', text: 'See chart here' },
    ]);
  });
});
