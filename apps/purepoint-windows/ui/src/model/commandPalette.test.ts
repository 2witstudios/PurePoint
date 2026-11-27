// Item properties of Models/CommandPaletteItem.swift (WIN-6.6) not covered by the Swift suites.
import { describe, expect, it } from 'vitest';
import { CLAUDE, TERMINAL, WORKTREE, addedAgentVariant, allVariants } from './agentVariant';
import {
  FILES_ITEM,
  agentDefItem,
  builtInItem,
  categoryLabel,
  displayName,
  fileItem,
  isIndividualLaunch,
  isQueryOnly,
  itemIcon,
  itemId,
  promptPlaceholder,
  showsNameField,
  showsPromptField,
  skipsPromptPhase,
  subtitle,
  swarmItem,
} from './commandPalette';
import { agentDef, swarmDef } from './commandPaletteFixtures';

describe('CommandPaletteItem', () => {
  it('ids match the Mac', () => {
    expect(itemId(builtInItem(CLAUDE))).toBe('builtin:claude:agent');
    expect(itemId(builtInItem(WORKTREE))).toBe('builtin:claude:worktree');
    expect(itemId(agentDefItem(agentDef('x', { scope: 'global' })))).toBe('agentdef:global:x');
    expect(itemId(swarmItem(swarmDef('team')))).toBe('swarm:local:team');
    expect(itemId(FILES_ITEM)).toBe('files');
    expect(itemId(fileItem({ relativePath: 'a/b.ts', absolutePath: '/p/a/b.ts' }))).toBe(
      'file:/p/a/b.ts',
    );
  });

  it('agent definition subtitles, placeholders and icons', () => {
    const templated = agentDefItem(agentDef('t', { template: 'review', icon: 'star' }));
    const inline = agentDefItem(agentDef('i', { inline_prompt: 'go' }));
    const plain = agentDefItem(agentDef('p', { agent_type: 'codex' }));
    expect([subtitle(templated), subtitle(inline), subtitle(plain)]).toEqual([
      'Template: review',
      'Inline prompt',
      'codex',
    ]);
    expect(promptPlaceholder(templated)).toBe('Override prompt (optional)...');
    expect(promptPlaceholder(plain)).toBe('Enter prompt...');
    expect([itemIcon(templated), itemIcon(plain)]).toEqual(['star', 'cpu']);
    expect([skipsPromptPhase(inline), skipsPromptPhase(plain)]).toEqual([true, false]);
    expect(categoryLabel(plain)).toBe('Agent');
  });

  it('swarm subtitle counts agents across worktrees', () => {
    const one = swarmItem(swarmDef('s', { roster: [{ agent_def: 'a', role: 'r' }] }));
    const many = swarmItem(
      swarmDef('m', { worktree_count: 2, roster: [{ agent_def: 'a', role: 'r', quantity: 3 }] }),
    );
    expect(subtitle(one)).toBe('1 agent across 1 worktree');
    expect(subtitle(many)).toBe('6 agents across 2 worktrees');
  });

  it('file items show the file name and are query-only', () => {
    const item = fileItem({
      relativePath: 'src\\app\\main.ts',
      absolutePath: 'C:\\p\\src\\app\\main.ts',
    });
    expect(displayName(item)).toBe('main.ts');
    expect(subtitle(item)).toBe('src\\app\\main.ts');
    expect(isQueryOnly(item)).toBe(true);
    expect(isQueryOnly(FILES_ITEM)).toBe(false);
    expect(itemIcon(item, (name) => (name.endsWith('.ts') ? 'curlybraces' : 'doc'))).toBe(
      'curlybraces',
    );
  });

  it('worktree shows only the name field and is not an individual launch', () => {
    const worktree = builtInItem(WORKTREE);
    expect([showsNameField(worktree), showsPromptField(worktree)]).toEqual([true, false]);
    expect(isIndividualLaunch(worktree)).toBe(false);
    expect(isIndividualLaunch(builtInItem(TERMINAL))).toBe(true);
  });

  it('added agents sit between the built-ins and Terminal', () => {
    const added = addedAgentVariant('aider', 'aider --yes');
    expect(allVariants([added]).map((v) => v.displayName)).toEqual([
      'Claude',
      'Codex',
      'OpenCode',
      'aider',
      'Terminal',
    ]);
    expect(added.subtitle).toBe('aider --yes');
  });
});
