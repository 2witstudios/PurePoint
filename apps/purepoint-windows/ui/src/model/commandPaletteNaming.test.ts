// Port of apps/purepoint-macos/purepoint-macosTests/CommandPaletteNamingTests.swift (WIN-6.6).
//
// Each title keeps the Swift test function name. The Mac drives `CommandPaletteViewController`
// through its AppKit fields; these drive the same flow through `CommandPaletteFlow`, with
// `isNameFieldVisible` / `isPromptFieldVisible` standing in for the fields' `isHidden`.
import { describe, expect, it } from 'vitest';
import { CODEX, WORKTREE, allVariants } from './agentVariant';
import {
  type CommandPaletteItem,
  type CommandPaletteResult,
  FILES_ITEM,
  agentDefItem,
  builtInItem,
  swarmItem,
} from './commandPalette';
import { agentDef, swarmDef } from './commandPaletteFixtures';
import { CommandPaletteFlow } from './commandPaletteFlow';
import {
  CommandPalettePreferences,
  type PreferenceStorage,
} from '../state/commandPalettePreferences';

function memoryStorage(): PreferenceStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

function flow(): { vc: CommandPaletteFlow; results: CommandPaletteResult[] } {
  const vc = new CommandPaletteFlow();
  const results: CommandPaletteResult[] = [];
  vc.onSelect = (result) => {
    results.push(result);
  };
  return { vc, results };
}

describe('CommandPaletteNamingTests', () => {
  it('preferenceDefaultsOnAndPersistsOff', () => {
    const defaults = memoryStorage();
    const settings = new CommandPalettePreferences(defaults);
    expect(settings.usePointGuardNames).toBe(true);
    settings.usePointGuardNames = false;
    expect(new CommandPalettePreferences(defaults).usePointGuardNames).toBe(false);
    settings.usePointGuardNames = true;
    expect(new CommandPalettePreferences(defaults).usePointGuardNames).toBe(true);
  });

  it('individualLaunchesRequireNameBeforePromptAndPreserveDrafts', () => {
    const items: CommandPaletteItem[] = [
      ...allVariants().map(builtInItem),
      agentDefItem(agentDef('custom', { command: 'my-command' })),
    ];
    for (const item of items) {
      const { vc, results } = flow();
      vc.usePointGuardNames = false;
      vc.selectItem(item);
      expect(vc.isNameFieldVisible).toBe(true);
      expect(vc.isPromptFieldVisible).toBe(false);
      vc.name = ' \n ';
      vc.enterName();
      expect(results).toEqual([]);
      expect(vc.isNameFieldVisible).toBe(true);
      vc.name = '  Fix Auth  ';
      vc.enterName();
      expect(vc.isNameFieldVisible).toBe(false);
      expect(vc.isPromptFieldVisible).toBe(true);
      vc.prompt = 'Investigate login';
      vc.handleEscape();
      expect(vc.isNameFieldVisible).toBe(true);
      expect(vc.name).toBe('Fix Auth');
      vc.handleEscape();
      expect(results).toEqual([]);
      vc.selectItem(item);
      expect(vc.name).toBe('Fix Auth');
      vc.enterName();
      expect(vc.prompt).toBe('Investigate login');
      vc.submitPrompt();
      expect(results).toHaveLength(1);
      const result = results[0];
      if (result?.kind !== 'spawnBuiltIn' && result?.kind !== 'spawnAgentDef') {
        throw new Error('Expected individual launch');
      }
      expect(result.prompt).toBe('Investigate login');
      expect(result.name).toBe('Fix Auth');
    }
  });

  it('inlinePromptRequiresNameAndThenLaunchesWithoutPromptStep', () => {
    const { vc, results } = flow();
    vc.usePointGuardNames = false;
    vc.selectItem(agentDefItem(agentDef('review', { inline_prompt: 'Review changes' })));
    expect(results).toEqual([]);
    vc.name = 'Review Auth';
    vc.enterName();
    expect(vc.isPromptFieldVisible).toBe(false);
    const result = results[0];
    if (result?.kind !== 'spawnAgentDef') throw new Error('Expected custom agent launch');
    expect(result.def.inline_prompt).toBe('Review changes');
    expect(result.prompt).toBeNull();
    expect(result.name).toBe('Review Auth');
  });

  it('automaticNamingAndNonAgentActionsKeepExistingFlow', () => {
    const { vc, results } = flow();
    let dismissals = 0;
    vc.onDismiss = () => {
      dismissals += 1;
    };
    vc.selectItem(builtInItem(CODEX));
    expect(vc.isNameFieldVisible).toBe(false);
    vc.submitPrompt();
    const first = results[0];
    if (first?.kind !== 'spawnBuiltIn') throw new Error('Expected built-in launch');
    expect(first.name).toBeNull();
    results.length = 0;
    vc.usePointGuardNames = false;
    vc.selectItem(builtInItem(WORKTREE));
    expect(vc.isNameFieldVisible).toBe(true);
    expect(vc.isPromptFieldVisible).toBe(false);
    vc.handleEscape();
    vc.handleEscape();
    expect(dismissals).toBe(1);
    expect(results).toEqual([]);
    vc.selectItem(FILES_ITEM);
    vc.selectItem(swarmItem(swarmDef('team')));
    expect(results).toHaveLength(2);
  });
});
