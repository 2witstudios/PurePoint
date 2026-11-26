// Port of apps/purepoint-macos/purepoint-macosTests/ContentBlockSplitterTests.swift (WIN-8.16).
// Each title keeps the Swift test function name.
import { describe, expect, it } from 'vitest';

import { splitContentBlocks, type ContentBlock } from './content';

function codeBlocks(blocks: ContentBlock[]) {
  return blocks.filter((block) => block.type === 'codeBlock');
}

describe('ContentBlockSplitterTests', () => {
  it('givenPlainTextShouldReturnSingleTextBlock', () => {
    const blocks = splitContentBlocks('Hello, world!');
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toEqual({ type: 'text', id: 'cb-0', text: 'Hello, world!' });
  });

  it('givenSingleCodeFenceShouldReturnTextAndCodeBlock', () => {
    const input = [
      'Here is some code:',
      '',
      '```swift',
      'let x = 42',
      '```',
      '',
      'And some more text.',
    ].join('\n');

    const blocks = splitContentBlocks(input);
    expect(blocks).toHaveLength(3);

    const [before, code, after] = blocks;
    expect(before?.type === 'text' && before.text).toContain('Here is some code:');
    expect(code).toMatchObject({ type: 'codeBlock', language: 'swift', code: 'let x = 42' });
    expect(after?.type === 'text' && after.text).toContain('And some more text.');
  });

  it('givenMultipleCodeFencesShouldSplitCorrectly', () => {
    const input = [
      'First:',
      '```python',
      'print("a")',
      '```',
      'Middle text.',
      '```rust',
      'fn main() {}',
      '```',
      'End.',
    ].join('\n');

    const blocks = splitContentBlocks(input);
    expect(blocks).toHaveLength(5);
    expect(blocks.map((block) => block.type)).toEqual([
      'text',
      'codeBlock',
      'text',
      'codeBlock',
      'text',
    ]);
    expect(blocks[1]).toMatchObject({ language: 'python' });
    expect(blocks[3]).toMatchObject({ language: 'rust' });
  });

  it('givenCodeFenceWithLanguageShouldCaptureLanguage', () => {
    const input = ['```typescript', 'const x: number = 1;', '```'].join('\n');

    const blocks = splitContentBlocks(input);
    expect(codeBlocks(blocks)[0]).toMatchObject({ language: 'typescript' });
  });

  it('givenCodeFenceWithoutLanguageShouldDefaultToNil', () => {
    const input = ['```', 'plain code', '```'].join('\n');

    const blocks = splitContentBlocks(input);
    expect(codeBlocks(blocks)[0]).toMatchObject({ language: null, code: 'plain code' });
  });

  it('givenUnclosedCodeFenceShouldTreatAsText', () => {
    const input = ['Some text', '```python', 'unclosed code block'].join('\n');

    const blocks = splitContentBlocks(input);
    expect(blocks).toHaveLength(1);
    const [only] = blocks;
    expect(only?.type).toBe('text');
    expect(only?.type === 'text' && only.text).toContain('unclosed code block');
  });

  it('givenOnlyCodeFenceShouldReturnSingleCodeBlock', () => {
    const input = ['```js', 'console.log("hi")', '```'].join('\n');

    const found = codeBlocks(splitContentBlocks(input));
    expect(found).toHaveLength(1);
    expect(found[0]).toMatchObject({ language: 'js', code: 'console.log("hi")' });
  });

  it('givenEmptyStringShouldReturnEmptyArray', () => {
    expect(splitContentBlocks('')).toEqual([]);
  });
});
