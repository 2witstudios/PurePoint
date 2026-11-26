// The selection → naming → prompt flow of `CommandPaletteViewController` in
// apps/purepoint-macos/purepoint-macos/Views/CommandPalette/CommandPalettePanel.swift, without the
// AppKit views (WIN-6.6), so CommandPaletteNamingTests run headless. The palette view (WIN-7.5)
// renders `phase`, binds `name` / `prompt` to its fields and forwards Enter and Escape here.
import {
  type CommandPaletteItem,
  type CommandPaletteResult,
  isIndividualLaunch,
  itemId,
  showsNameField,
  showsPromptField,
  skipsPromptPhase,
} from './commandPalette';

export type CommandPalettePhase =
  | { readonly kind: 'selection' }
  | { readonly kind: 'naming'; readonly item: CommandPaletteItem }
  | { readonly kind: 'prompt'; readonly item: CommandPaletteItem };

export const NAME_HINT = 'Enter to continue';
export const NAME_REQUIRED_HINT = 'Enter a name to continue';

export class CommandPaletteFlow {
  onSelect: ((result: CommandPaletteResult) => void) | undefined;
  onDismiss: (() => void) | undefined;
  /** Off: individual launches ask for a name before the prompt (Settings › General). */
  usePointGuardNames = true;

  /** Name field text. */
  name = '';
  /** Prompt field text. */
  prompt = '';
  /** Hint under the name field while naming. */
  nameHint = '';

  private currentPhase: CommandPalettePhase = { kind: 'selection' };
  private draftItemId: string | null = null;

  get phase(): CommandPalettePhase {
    return this.currentPhase;
  }

  /** Whether the name field is shown. */
  get isNameFieldVisible(): boolean {
    const phase = this.currentPhase;
    if (phase.kind === 'naming') return true;
    return phase.kind === 'prompt' && showsNameField(phase.item);
  }

  /** Whether the prompt field is shown. */
  get isPromptFieldVisible(): boolean {
    const phase = this.currentPhase;
    return phase.kind === 'prompt' && showsPromptField(phase.item);
  }

  private requiresName(item: CommandPaletteItem): boolean {
    return !this.usePointGuardNames && isIndividualLaunch(item);
  }

  selectItem(item: CommandPaletteItem): void {
    if (this.requiresName(item)) {
      this.showNaming(item);
    } else if (skipsPromptPhase(item)) {
      const result = immediateResult(item);
      if (result) this.onSelect?.(result);
    } else {
      this.showPrompt(item);
    }
  }

  /** Enter in the name field. */
  enterName(): void {
    const phase = this.currentPhase;
    if (phase.kind === 'naming') {
      this.submitName(phase.item);
    } else if (phase.kind === 'prompt' && !showsPromptField(phase.item)) {
      this.submitPrompt();
    }
    // Otherwise Enter moves focus to the prompt field, which is the view's business.
  }

  /** Enter (without Shift or Alt) in the prompt field. */
  submitPrompt(): void {
    const phase = this.currentPhase;
    if (phase.kind !== 'prompt') return;
    const item = phase.item;
    const prompt = this.prompt.trim();
    const needsName = this.requiresName(item);
    const name = showsNameField(item) || needsName ? this.name.trim() : null;
    if (needsName && !name) return;
    const promptOrNull = prompt === '' ? null : prompt;
    const nameOrNull = name === '' ? null : name;
    let result: CommandPaletteResult | null;
    switch (item.kind) {
      case 'builtIn':
        result =
          item.variant.kind === 'worktree'
            ? { kind: 'createWorktree', name: nameOrNull }
            : {
                kind: 'spawnBuiltIn',
                variant: item.variant,
                prompt: promptOrNull,
                name: nameOrNull,
              };
        break;
      case 'agentDef':
        result = { kind: 'spawnAgentDef', def: item.def, prompt: promptOrNull, name: nameOrNull };
        break;
      default:
        result = immediateResult(item);
    }
    if (result) this.onSelect?.(result);
  }

  handleEscape(): void {
    const phase = this.currentPhase;
    switch (phase.kind) {
      case 'selection':
        this.onDismiss?.();
        break;
      case 'naming':
        this.currentPhase = { kind: 'selection' };
        break;
      case 'prompt':
        if (this.requiresName(phase.item)) this.showNaming(phase.item);
        else this.currentPhase = { kind: 'selection' };
        break;
    }
  }

  private submitName(item: CommandPaletteItem): void {
    const name = this.name.trim();
    if (name === '') {
      this.nameHint = NAME_REQUIRED_HINT;
      return;
    }
    this.name = name;
    if (skipsPromptPhase(item)) {
      if (item.kind === 'agentDef') {
        this.onSelect?.({ kind: 'spawnAgentDef', def: item.def, prompt: null, name });
      }
    } else {
      this.showPrompt(item);
    }
  }

  private showNaming(item: CommandPaletteItem): void {
    this.currentPhase = { kind: 'naming', item };
    this.prepareItem(item);
    this.nameHint = NAME_HINT;
  }

  private showPrompt(item: CommandPaletteItem): void {
    this.currentPhase = { kind: 'prompt', item };
    this.prepareItem(item);
    this.nameHint = '';
  }

  /** Drafts survive going back and forth on one item and reset when switching items. */
  private prepareItem(item: CommandPaletteItem): void {
    const id = itemId(item);
    if (this.draftItemId !== id) {
      this.prompt = '';
      this.name = '';
      this.draftItemId = id;
    }
  }
}

/** The result of an item that runs without a prompt; null for built-ins, which always prompt. */
function immediateResult(item: CommandPaletteItem): CommandPaletteResult | null {
  switch (item.kind) {
    case 'agentDef':
      return { kind: 'spawnAgentDef', def: item.def, prompt: null, name: null };
    case 'swarm':
      return { kind: 'runSwarm', def: item.swarm };
    case 'files':
      return { kind: 'openFilePane', path: null };
    case 'file':
      return { kind: 'openFilePane', path: item.file.absolutePath };
    case 'builtIn':
      return null;
  }
}
