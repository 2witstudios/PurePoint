// Port of apps/purepoint-macos/purepoint-macosTests/CommandPaletteOrderTests.swift (WIN-6.6).
//
// Each title keeps the Swift test function name. `SettingsState(defaults:)` is
// `CommandPalettePreferences` over an in-memory storage standing in for a UserDefaults suite.
import { describe, expect, it } from 'vitest';
import { allVariants, variantsWithWorktree } from './agentVariant';
import { type CommandPaletteItem, buildItems, displayName } from './commandPalette';
import { agentDef } from './commandPaletteFixtures';
import {
  CommandPalettePreferences,
  type PreferenceStorage,
} from '../state/commandPalettePreferences';

function items(order: readonly string[] = []): CommandPaletteItem[] {
  return buildItems({
    builtInVariants: variantsWithWorktree(),
    agents: [
      agentDef('custom', { command: 'my-command' }),
      agentDef('hidden', { available_in_command_dialog: false }),
    ],
    swarms: [],
    preferredOrder: order,
  });
}

const names = (list: readonly CommandPaletteItem[]): string[] => list.map(displayName);

function memoryStorage(): PreferenceStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

describe('CommandPaletteOrderTests', () => {
  it('defaultOrderPreservesExistingEntries', () => {
    expect(names(items())).toEqual([
      'Claude',
      'Codex',
      'OpenCode',
      'Terminal',
      'Worktree',
      'custom',
    ]);
  });

  it('customCommandCanPrecedeBuiltIns', () => {
    const order = ['agentdef:local:custom', 'builtin:codex:agent', 'builtin:claude:agent'];
    expect(names(items(order))).toEqual([
      'custom',
      'Codex',
      'Claude',
      'OpenCode',
      'Terminal',
      'Worktree',
    ]);
  });

  it('missingAndDuplicatePreferencesDoNotDropEntries', () => {
    const order = [
      'missing',
      'builtin:codex:agent',
      'builtin:codex:agent',
      'builtin:claude:worktree',
    ];
    expect(names(items(order))).toEqual([
      'Codex',
      'Worktree',
      'Claude',
      'OpenCode',
      'Terminal',
      'custom',
    ]);
  });

  it('tabPaletteKeepsOrderWhenWorktreeIsUnavailable', () => {
    const palette = buildItems({
      builtInVariants: allVariants(),
      agents: [],
      swarms: [],
      includeFiles: true,
      preferredOrder: ['builtin:claude:worktree', 'builtin:opencode:agent', 'builtin:codex:agent'],
    });
    expect(names(palette)).toEqual(['OpenCode', 'Codex', 'Claude', 'Terminal', 'Files']);
  });

  it('movingEntriesPersistsAcrossSettingsInstances', () => {
    const defaults = memoryStorage();
    const settings = new CommandPalettePreferences(defaults);

    settings.moveCommandPaletteItem('builtin:codex:agent', -1, items());
    const restored = new CommandPalettePreferences(defaults);
    expect(items(restored.commandPaletteOrder).map(displayName)[0]).toBe('Codex');

    const unchanged = restored.commandPaletteOrder;
    restored.moveCommandPaletteItem('builtin:codex:agent', -1, items(unchanged));
    expect(restored.commandPaletteOrder).toEqual(unchanged);

    restored.commandPaletteOrder = [];
    expect(new CommandPalettePreferences(defaults).commandPaletteOrder).toEqual([]);
  });

  it('movingPreservesUnavailablePreferences', () => {
    const settings = new CommandPalettePreferences(memoryStorage());
    settings.commandPaletteOrder = ['unavailable', 'agentdef:local:custom', 'builtin:claude:agent'];
    const visible = items(settings.commandPaletteOrder);
    settings.moveCommandPaletteItem('agentdef:local:custom', 1, visible);
    expect(settings.commandPaletteOrder[0]).toBe('unavailable');
    expect(names(items(settings.commandPaletteOrder)).slice(0, 2)).toEqual(['Claude', 'custom']);
  });
});
