import { render } from '@testing-library/react';
import { icons } from 'lucide-react';
import { describe, expect, it } from 'vitest';
import matrix from '../../../../../docs/reference/windows-parity-matrix.md?raw';
import { Icon } from './Icon.tsx';
import { SF_SYMBOLS, symbolFill, type SfSymbolName } from './icons.ts';

const unquote = (cell: string) => cell.trim().replace(/^`(.*)`$/, '$1');

/** Rows of the matrix's `| SF Symbol | Lucide icon | Used in |` table, as [symbol, lucide name]. */
function symbolTable(markdown: string): [string, string][] {
  const lines = markdown.split(/\r?\n/);
  const start = lines.findIndex((l) =>
    /^\|\s*SF Symbol\s*\|\s*Lucide icon\s*\|\s*Used in\s*\|$/.test(l.trim()),
  );
  if (start < 0) throw new Error('SF Symbols table not found in windows-parity-matrix.md');
  const rows: [string, string][] = [];
  for (const line of lines.slice(start + 2)) {
    if (!line.trim().startsWith('|')) break;
    const [symbol = '', lucide = ''] = line.trim().slice(1, -1).split('|').map(unquote);
    rows.push([symbol, lucide]);
  }
  return rows;
}

const pascal = (kebab: string) =>
  kebab
    .split('-')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join('');

const table = symbolTable(matrix);
const mapped: Record<string, readonly [string, unknown]> = SF_SYMBOLS;
const lucide: Record<string, unknown> = icons;

describe('SF Symbol icon map', () => {
  it('reads the whole matrix table', () => {
    expect(table.length).toBeGreaterThanOrEqual(104);
  });

  it.each(table)('maps %s to %s', (symbol, lucideName) => {
    const entry = mapped[symbol];
    expect(entry, `${symbol} has no entry in icons.ts`).toBeDefined();
    expect(entry?.[0]).toBe(lucideName);
    const component = lucide[pascal(lucideName)];
    expect(component, `lucide-react has no icon named ${lucideName}`).toBeDefined();
    expect(entry?.[1]).toBe(component);
  });

  it('has no symbols the matrix does not list', () => {
    const listed = new Set(table.map(([s]) => s));
    expect(Object.keys(SF_SYMBOLS).filter((s) => !listed.has(s))).toEqual([]);
  });

  it('fills .fill variants', () => {
    expect(symbolFill('gear')).toBe('none');
    expect(symbolFill('stop.fill')).toBe('solid');
    expect(symbolFill('checkmark.circle.fill')).toBe('tint');
  });
});

describe('<Icon>', () => {
  it('renders a decorative Lucide svg', () => {
    const { container } = render(<Icon name="gear" />);
    const svg = container.querySelector('svg');
    expect(svg).toHaveAttribute('data-symbol', 'gear');
    expect(svg).toHaveAttribute('aria-hidden', 'true');
    expect(svg).toHaveAttribute('width', '16');
    expect(svg).toHaveAttribute('fill', 'none');
  });

  it('labels and fills when asked', () => {
    const name: SfSymbolName = 'stop.fill';
    const { getByRole } = render(<Icon name={name} label="Stop" size={12} />);
    const svg = getByRole('img', { name: 'Stop' });
    expect(svg).toHaveAttribute('fill', 'currentColor');
    expect(svg).toHaveAttribute('width', '12');
  });
});
