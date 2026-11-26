// The command palette's view (WIN-7.5): rows, filtering, keyboard navigation and the naming and
// prompt steps, as CommandPalettePanel.swift and CommandPaletteRowView.swift behave.
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import {
  type CommandPaletteItem,
  type CommandPaletteResult,
  FILES_ITEM,
  agentDefItem,
  buildItems,
  builtInItem,
  fileItem,
  swarmItem,
} from '../../model/commandPalette.ts';
import { agentDef, swarmDef } from '../../model/commandPaletteFixtures.ts';
import { CLAUDE, TERMINAL, WORKTREE, variantsWithWorktree } from '../../model/agentVariant.ts';
import { CommandPaletteHost, CommandPalettePanel, PROMPT_HINT } from './CommandPalettePanel.tsx';
import { MAX_FILE_RESULTS, filterPaletteItems, isSubsequence } from './paletteFilter.ts';
import {
  dismissPalette,
  isPaletteVisible,
  requestPaletteOpen,
  showPalette,
} from './paletteStore.ts';

beforeAll(() => {
  Element.prototype.scrollIntoView = () => undefined;
});

afterEach(() => {
  act(() => {
    dismissPalette();
  });
});

const reviewer = agentDef('reviewer', { tags: ['review'], template: 'review-pr' });
const quick = agentDef('quick-fix', { inline_prompt: 'Fix the build' });
const hidden = agentDef('hidden', { available_in_command_dialog: false });
const fleet = swarmDef('fleet', { worktree_count: 2, roster: [{ agent_def: 'a', role: 'r' }] });

function projectItems(): CommandPaletteItem[] {
  return buildItems({
    builtInVariants: variantsWithWorktree(),
    agents: [reviewer, quick, hidden],
    swarms: [fleet],
  });
}

function setup(
  options: {
    items?: CommandPaletteItem[];
    usePointGuardNames?: boolean;
    initialItem?: CommandPaletteItem | null;
  } = {},
) {
  const onSelect = vi.fn<(result: CommandPaletteResult) => void>();
  const onDismiss = vi.fn();
  render(
    <CommandPalettePanel
      items={options.items ?? projectItems()}
      usePointGuardNames={options.usePointGuardNames ?? true}
      initialItem={options.initialItem ?? null}
      onSelect={onSelect}
      onDismiss={onDismiss}
    />,
  );
  return { onSelect, onDismiss };
}

const rowNames = () =>
  screen
    .queryAllByTestId('palette-row')
    .map((row) => row.querySelector('.palette-row-name')?.textContent);
const highlighted = () =>
  screen
    .queryAllByTestId('palette-row')
    .find((row) => row.dataset.highlighted === 'true')
    ?.querySelector('.palette-row-name')?.textContent;
const search = () => screen.getByRole('combobox', { name: 'Filter' });
const key = (target: HTMLElement, name: string, init: KeyboardEventInit = {}) => {
  fireEvent.keyDown(target, { key: name, ...init });
};

describe('CommandPalettePanel rows', () => {
  it('lists built-ins, then the dialog agents, then swarms, with subtitles and badges', () => {
    setup();
    expect(rowNames()).toEqual([
      'Claude',
      'Codex',
      'OpenCode',
      'Terminal',
      'Worktree',
      'reviewer',
      'quick-fix',
      'fleet',
    ]);
    const rows = screen.getAllByTestId('palette-row');
    expect(rows[3]).toHaveTextContent('Shell session');
    expect(rows[5]).toHaveTextContent('Template: review-pr');
    expect(rows[5]?.querySelector('.palette-row-badge')).toHaveTextContent('Agent');
    expect(rows[6]).toHaveTextContent('Inline prompt');
    expect(rows[7]).toHaveTextContent('2 agents across 2 worktrees');
    expect(rows[7]?.querySelector('.palette-row-badge')).toHaveTextContent('Swarm');
    expect(rows[0]?.querySelector('.palette-row-badge')).toBeNull();
    // Icons: the variant's, then cpu for a definition without one, person.3 for a swarm.
    expect(rows[3]?.querySelector('svg')?.dataset.symbol).toBe('terminal');
    expect(rows[5]?.querySelector('svg')?.dataset.symbol).toBe('cpu');
    expect(rows[7]?.querySelector('svg')?.dataset.symbol).toBe('person.3');
  });

  it('opens with the search field focused and the first row highlighted', () => {
    setup();
    expect(search()).toHaveFocus();
    expect(search()).toHaveAttribute('placeholder', 'Type to filter...');
    expect(highlighted()).toBe('Claude');
  });

  it('shows file rows only for a query, with the language icon', () => {
    const items = [
      builtInItem(CLAUDE),
      FILES_ITEM,
      fileItem({ relativePath: 'src/EditorState.swift', absolutePath: '/p/src/EditorState.swift' }),
    ];
    setup({ items });
    expect(rowNames()).toEqual(['Claude', 'Files']);
    fireEvent.change(search(), { target: { value: 'edst' } });
    expect(rowNames()).toEqual(['EditorState.swift']);
    const row = screen.getByTestId('palette-row');
    expect(row).toHaveTextContent('src/EditorState.swift');
    expect(row.querySelector('.palette-row-badge')).toHaveTextContent('File');
    expect(row.querySelector('svg')?.dataset.symbol).toBe('swift');
  });
});

describe('CommandPalettePanel filtering', () => {
  it('ranks name prefixes, then name matches, then other text', () => {
    setup();
    fireEvent.change(search(), { target: { value: 'co' } });
    expect(rowNames()).toEqual(['Codex', 'OpenCode', 'Claude']);
    fireEvent.change(search(), { target: { value: 'review' } });
    expect(rowNames()).toEqual(['reviewer']);
  });

  it('highlights the first match, and nothing when nothing matches', () => {
    setup();
    fireEvent.change(search(), { target: { value: 'term' } });
    expect(rowNames()).toEqual(['Terminal']);
    expect(highlighted()).toBe('Terminal');
    fireEvent.change(search(), { target: { value: 'zzz' } });
    expect(rowNames()).toEqual([]);
    key(search(), 'Enter');
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'selection');
  });

  it('keeps at most 30 files', () => {
    const files = Array.from({ length: 40 }, (_, i) =>
      fileItem({ relativePath: `a/file${String(i)}.ts`, absolutePath: `/p/a/file${String(i)}.ts` }),
    );
    const shown = filterPaletteItems([builtInItem(CLAUDE), ...files], 'file');
    expect(shown).toHaveLength(MAX_FILE_RESULTS);
    expect(filterPaletteItems(files, '')).toEqual([]);
  });

  it('matches subsequences in order only', () => {
    expect(isSubsequence('edst', 'editorstate')).toBe(true);
    expect(isSubsequence('tsde', 'editorstate')).toBe(false);
  });
});

describe('CommandPalettePanel keyboard', () => {
  it('moves the highlight with the arrows, stopping at the ends', () => {
    setup();
    key(search(), 'ArrowUp');
    expect(highlighted()).toBe('Claude');
    key(search(), 'ArrowDown');
    key(search(), 'ArrowDown');
    expect(highlighted()).toBe('OpenCode');
    for (let i = 0; i < 10; i += 1) key(search(), 'ArrowDown');
    expect(highlighted()).toBe('fleet');
    key(search(), 'ArrowUp');
    expect(highlighted()).toBe('quick-fix');
  });

  it('Enter on a built-in asks for the prompt, and Enter there spawns it', () => {
    const { onSelect } = setup();
    fireEvent.change(search(), { target: { value: 'term' } });
    key(search(), 'Enter');
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'prompt');
    expect(screen.getByTestId('palette-item-header')).toHaveTextContent('Terminal');
    const prompt = screen.getByRole('textbox', { name: 'Prompt' });
    expect(prompt).toHaveFocus();
    expect(prompt).toHaveAttribute('placeholder', 'Enter initial command (optional)...');
    expect(screen.getByText(PROMPT_HINT)).toBeInTheDocument();
    fireEvent.change(prompt, { target: { value: '  ls -la  ' } });
    key(prompt, 'Enter', { shiftKey: true });
    expect(onSelect).not.toHaveBeenCalled();
    key(prompt, 'Enter');
    expect(onSelect).toHaveBeenCalledWith({
      kind: 'spawnBuiltIn',
      variant: TERMINAL,
      prompt: 'ls -la',
      name: null,
    });
  });

  it('an empty prompt spawns with no prompt', () => {
    const { onSelect } = setup();
    key(search(), 'Enter');
    key(screen.getByRole('textbox', { name: 'Prompt' }), 'Enter');
    expect(onSelect).toHaveBeenCalledWith({
      kind: 'spawnBuiltIn',
      variant: CLAUDE,
      prompt: null,
      name: null,
    });
  });

  it('Escape steps back to the full list, then dismisses', () => {
    const { onDismiss } = setup();
    fireEvent.change(search(), { target: { value: 'cod' } });
    key(search(), 'Enter');
    key(screen.getByRole('textbox', { name: 'Prompt' }), 'Escape');
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'selection');
    expect(search()).toHaveValue('');
    expect(search()).toHaveFocus();
    expect(rowNames()).toHaveLength(8);
    expect(onDismiss).not.toHaveBeenCalled();
    key(search(), 'Escape');
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('a definition with an inline prompt and a swarm run at once', () => {
    const { onSelect } = setup();
    fireEvent.change(search(), { target: { value: 'quick' } });
    key(search(), 'Enter');
    expect(onSelect).toHaveBeenLastCalledWith({
      kind: 'spawnAgentDef',
      def: quick,
      prompt: null,
      name: null,
    });
  });

  it('a click highlights a row and a double click chooses it', () => {
    const { onSelect } = setup({ items: [builtInItem(CLAUDE), swarmItem(fleet)] });
    const rows = screen.getAllByTestId('palette-row');
    const swarmRow = rows[1];
    if (swarmRow === undefined) throw new Error('no swarm row');
    fireEvent.mouseDown(swarmRow);
    expect(highlighted()).toBe('fleet');
    expect(search()).toHaveFocus();
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.doubleClick(swarmRow);
    expect(onSelect).toHaveBeenCalledWith({ kind: 'runSwarm', def: fleet });
  });

  it('a file row opens the file pane on it', () => {
    const file = { relativePath: 'README.md', absolutePath: '/p/README.md' };
    const { onSelect } = setup({ items: [FILES_ITEM, fileItem(file)] });
    fireEvent.change(search(), { target: { value: 'readme' } });
    key(search(), 'Enter');
    expect(onSelect).toHaveBeenCalledWith({ kind: 'openFilePane', path: '/p/README.md' });
  });
});

describe('CommandPalettePanel naming', () => {
  it('asks for a name first when Point Guard does not name agents', () => {
    const { onSelect } = setup({ usePointGuardNames: false });
    key(search(), 'Enter');
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'naming');
    expect(screen.getByTestId('palette-item-header')).toHaveTextContent('Name Claude');
    const name = screen.getByRole('textbox', { name: 'Name' });
    expect(name).toHaveFocus();
    expect(name).toHaveAttribute('placeholder', 'e.g., Fix authentication');
    expect(screen.getByTestId('palette-name-note')).toHaveTextContent('Enter to continue');
    key(name, 'Enter');
    expect(screen.getByTestId('palette-name-note')).toHaveTextContent('Enter a name to continue');
    fireEvent.change(name, { target: { value: ' Fix auth ' } });
    expect(screen.getByTestId('palette-name-note')).toHaveTextContent('Enter to continue');
    key(name, 'Enter');
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'prompt');
    const prompt = screen.getByRole('textbox', { name: 'Prompt' });
    expect(prompt).toHaveFocus();
    // Escape returns to the name, keeping the draft.
    key(prompt, 'Escape');
    expect(screen.getByRole('textbox', { name: 'Name' })).toHaveValue('Fix auth');
    key(screen.getByRole('textbox', { name: 'Name' }), 'Enter');
    fireEvent.change(screen.getByRole('textbox', { name: 'Prompt' }), {
      target: { value: 'go' },
    });
    key(screen.getByRole('textbox', { name: 'Prompt' }), 'Enter');
    expect(onSelect).toHaveBeenCalledWith({
      kind: 'spawnBuiltIn',
      variant: CLAUDE,
      prompt: 'go',
      name: 'Fix auth',
    });
  });

  it('a worktree takes a name with its branch preview and creates the worktree', () => {
    const { onSelect } = setup();
    fireEvent.change(search(), { target: { value: 'worktree' } });
    key(search(), 'Enter');
    const name = screen.getByRole('textbox', { name: 'Name' });
    expect(name).toHaveFocus();
    expect(name).toHaveAttribute('placeholder', 'e.g., fix-auth-bug');
    expect(screen.queryByRole('textbox', { name: 'Prompt' })).toBeNull();
    fireEvent.change(name, { target: { value: 'Fix Auth_Bug' } });
    expect(screen.getByTestId('palette-name-note')).toHaveTextContent('pu/fix-auth-bug');
    key(name, 'Enter');
    expect(onSelect).toHaveBeenCalledWith({ kind: 'createWorktree', name: 'Fix Auth_Bug' });
  });

  it('an initial item goes straight to its step', () => {
    setup({ items: [builtInItem(TERMINAL)], initialItem: builtInItem(TERMINAL) });
    expect(screen.getByTestId('command-palette')).toHaveAttribute('data-phase', 'prompt');
    expect(screen.getByRole('textbox', { name: 'Prompt' })).toHaveFocus();
  });

  it('a named definition with an inline prompt spawns once named', () => {
    const { onSelect } = setup({ items: [agentDefItem(quick)], usePointGuardNames: false });
    key(search(), 'Enter');
    const name = screen.getByRole('textbox', { name: 'Name' });
    fireEvent.change(name, { target: { value: 'fixer' } });
    key(name, 'Enter');
    expect(onSelect).toHaveBeenCalledWith({
      kind: 'spawnAgentDef',
      def: quick,
      prompt: null,
      name: 'fixer',
    });
  });
});

describe('CommandPaletteHost', () => {
  it('shows the presented palette, dismisses it on a click outside and after a choice', () => {
    const onSelect = vi.fn();
    render(<CommandPaletteHost />);
    expect(screen.queryByTestId('command-palette')).toBeNull();
    act(() => {
      showPalette({ items: [builtInItem(WORKTREE)], usePointGuardNames: true, onSelect });
    });
    expect(screen.getByTestId('command-palette')).toBeInTheDocument();
    fireEvent.pointerDown(screen.getByTestId('palette-backdrop'));
    expect(screen.queryByTestId('command-palette')).toBeNull();

    act(() => {
      showPalette({ items: [builtInItem(WORKTREE)], usePointGuardNames: true, onSelect });
    });
    key(search(), 'Enter');
    key(screen.getByRole('textbox', { name: 'Name' }), 'Enter');
    expect(onSelect).toHaveBeenCalledWith({ kind: 'createWorktree', name: null });
    expect(isPaletteVisible()).toBe(false);
    expect(screen.queryByTestId('command-palette')).toBeNull();
  });

  it('a second open request toggles a shown palette closed without loading', async () => {
    const load = vi.fn(() => {
      showPalette({ items: [builtInItem(CLAUDE)], usePointGuardNames: true, onSelect: vi.fn() });
    });
    await act(async () => {
      await requestPaletteOpen(load);
    });
    expect(isPaletteVisible()).toBe(true);
    act(() => {
      expect(requestPaletteOpen(load)).toBeNull();
    });
    expect(isPaletteVisible()).toBe(false);
    expect(load).toHaveBeenCalledOnce();
  });
});
