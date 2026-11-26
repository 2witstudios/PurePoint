// Port of apps/purepoint-macos/purepoint-macos/Models/CommandPaletteItem.swift (WIN-6.6),
// plus the order move from State/SettingsState.swift (`moveCommandPaletteItem`) as a pure function.
//
// Agent and swarm definitions are the daemon's `AgentDefInfo` / `SwarmDefInfo` payloads (the Mac's
// AgentsHubModels `AgentDefinition` / `SwarmDefinition` are thin copies of them). Their ids are
// `scope:name`, as on the Mac.
import type { AgentDefInfo, SwarmDefInfo } from '../protocol';
import type { AgentVariant } from './agentVariant';

/** A file that can be opened into a file pane. */
export interface PaletteFileEntry {
  readonly relativePath: string;
  readonly absolutePath: string;
}

export type CommandPaletteItem =
  | { readonly kind: 'builtIn'; readonly variant: AgentVariant }
  | { readonly kind: 'agentDef'; readonly def: AgentDefInfo }
  | { readonly kind: 'swarm'; readonly swarm: SwarmDefInfo }
  /** Opens the file navigator in a pane. */
  | { readonly kind: 'files' }
  /** Opens a specific file in a pane. Only offered once the user types a query. */
  | { readonly kind: 'file'; readonly file: PaletteFileEntry };

export type CommandPaletteResult =
  | {
      readonly kind: 'spawnBuiltIn';
      readonly variant: AgentVariant;
      readonly prompt: string | null;
      readonly name: string | null;
    }
  | {
      readonly kind: 'spawnAgentDef';
      readonly def: AgentDefInfo;
      readonly prompt: string | null;
      readonly name: string | null;
    }
  | { readonly kind: 'runSwarm'; readonly def: SwarmDefInfo }
  | { readonly kind: 'createWorktree'; readonly name: string | null }
  /** Show the file navigator in the pane, optionally opened on `path`. */
  | { readonly kind: 'openFilePane'; readonly path: string | null };

export const builtInItem = (variant: AgentVariant): CommandPaletteItem => ({
  kind: 'builtIn',
  variant,
});
export const agentDefItem = (def: AgentDefInfo): CommandPaletteItem => ({ kind: 'agentDef', def });
export const swarmItem = (swarm: SwarmDefInfo): CommandPaletteItem => ({ kind: 'swarm', swarm });
export const FILES_ITEM: CommandPaletteItem = { kind: 'files' };
export const fileItem = (file: PaletteFileEntry): CommandPaletteItem => ({ kind: 'file', file });

/** `AgentDefinition.id` / `SwarmDefinition.id` on the Mac. */
export const definitionId = (def: { scope: string; name: string }): string =>
  `${def.scope}:${def.name}`;

/** Total agents a swarm launches: its roster (quantity defaults to 1) in every worktree. */
export function swarmTotalAgents(swarm: SwarmDefInfo): number {
  return swarm.worktree_count * swarm.roster.reduce((sum, r) => sum + (r.quantity ?? 1), 0);
}

function lastPathComponent(path: string): string {
  const parts = path.split(/[\\/]/).filter((p) => p !== '');
  return parts.at(-1) ?? path;
}

export function itemId(item: CommandPaletteItem): string {
  switch (item.kind) {
    case 'builtIn':
      return `builtin:${item.variant.id}:${item.variant.kind}`;
    case 'agentDef':
      return `agentdef:${definitionId(item.def)}`;
    case 'swarm':
      return `swarm:${definitionId(item.swarm)}`;
    case 'files':
      return 'files';
    case 'file':
      return `file:${item.file.absolutePath}`;
  }
}

export function displayName(item: CommandPaletteItem): string {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.displayName;
    case 'agentDef':
      return item.def.name;
    case 'swarm':
      return item.swarm.name;
    case 'files':
      return 'Files';
    case 'file':
      return lastPathComponent(item.file.relativePath);
  }
}

/**
 * SF Symbol name for the row. File rows take their icon from the editor language, which lands
 * with `EditorTab` (WIN-8.5); until then the caller may pass `fileIcon`, else `doc`.
 */
export function itemIcon(
  item: CommandPaletteItem,
  fileIcon: (fileName: string) => string = () => 'doc',
): string {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.icon;
    case 'agentDef':
      return item.def.icon ?? 'cpu';
    case 'swarm':
      return 'person.3';
    case 'files':
      return 'folder';
    case 'file':
      return fileIcon(lastPathComponent(item.file.relativePath));
  }
}

const plural = (n: number, word: string): string => `${String(n)} ${word}${n === 1 ? '' : 's'}`;

export function subtitle(item: CommandPaletteItem): string {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.subtitle;
    case 'agentDef':
      if (item.def.template !== null) return `Template: ${item.def.template}`;
      if (item.def.inline_prompt !== null) return 'Inline prompt';
      return item.def.agent_type;
    case 'swarm':
      return `${plural(swarmTotalAgents(item.swarm), 'agent')} across ${plural(item.swarm.worktree_count, 'worktree')}`;
    case 'files':
      return 'Browse and edit files';
    case 'file':
      return item.file.relativePath;
  }
}

export function promptPlaceholder(item: CommandPaletteItem): string {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.promptPlaceholder;
    case 'agentDef':
      return item.def.template !== null ? 'Override prompt (optional)...' : 'Enter prompt...';
    default:
      return '';
  }
}

export function categoryLabel(item: CommandPaletteItem): string | null {
  switch (item.kind) {
    case 'agentDef':
      return 'Agent';
    case 'swarm':
      return 'Swarm';
    case 'file':
      return 'File';
    default:
      return null;
  }
}

/** Text blob used for fuzzy-filtering in the palette. */
export function searchableText(item: CommandPaletteItem): string {
  switch (item.kind) {
    case 'builtIn':
      return `${item.variant.id} ${item.variant.displayName} ${item.variant.subtitle}`;
    case 'agentDef':
      return `${item.def.name} ${item.def.agent_type} ${item.def.tags.join(' ')}`;
    case 'swarm':
      return item.swarm.name;
    case 'files':
      return 'files file browser explorer navigator editor markdown';
    case 'file':
      return item.file.relativePath;
  }
}

/** Whether selecting this item should skip the prompt phase and execute immediately. */
export function skipsPromptPhase(item: CommandPaletteItem): boolean {
  switch (item.kind) {
    case 'builtIn':
      return false;
    case 'agentDef':
      return item.def.inline_prompt !== null;
    default:
      return true;
  }
}

export function isIndividualLaunch(item: CommandPaletteItem): boolean {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.kind !== 'worktree';
    case 'agentDef':
      return true;
    default:
      return false;
  }
}

/** Only shown when the user has typed a query (one per file would flood the default list). */
export const isQueryOnly = (item: CommandPaletteItem): boolean => item.kind === 'file';

/** The worktree-style name field should be shown in the prompt phase. */
export const showsNameField = (item: CommandPaletteItem): boolean =>
  item.kind === 'builtIn' && item.variant.kind === 'worktree';

/** Whether the prompt field shows in the prompt phase. Worktree items only need a name. */
export function showsPromptField(item: CommandPaletteItem): boolean {
  switch (item.kind) {
    case 'builtIn':
      return item.variant.kind !== 'worktree';
    case 'agentDef':
      return true;
    default:
      return false;
  }
}

export interface BuildItemsInput {
  readonly builtInVariants: readonly AgentVariant[];
  readonly agents: readonly AgentDefInfo[];
  readonly swarms: readonly SwarmDefInfo[];
  readonly includeFiles?: boolean;
  readonly files?: readonly PaletteFileEntry[];
  /** Item ids in the user's order; unknown and repeated ids are ignored. */
  readonly preferredOrder?: readonly string[];
}

/**
 * Built-ins, then Files (when included), then agents shown in the command dialog, then swarms,
 * stably re-sorted so ids in `preferredOrder` come first in that order.
 */
export function buildItems(input: BuildItemsInput): CommandPaletteItem[] {
  const { includeFiles = false, files = [], preferredOrder = [] } = input;
  const items: CommandPaletteItem[] = [
    ...input.builtInVariants.map(builtInItem),
    ...(includeFiles ? [FILES_ITEM, ...files.map(fileItem)] : []),
    ...input.agents.filter((d) => d.available_in_command_dialog).map(agentDefItem),
    ...input.swarms.map(swarmItem),
  ];
  const ranks = new Map<string, number>();
  preferredOrder.forEach((id, index) => {
    if (!ranks.has(id)) ranks.set(id, index);
  });
  const rank = (item: CommandPaletteItem): number =>
    ranks.get(itemId(item)) ?? Number.MAX_SAFE_INTEGER;
  return items
    .map((item, offset) => ({ item, offset, rank: rank(item) }))
    .sort((a, b) => (a.rank === b.rank ? a.offset - b.offset : a.rank - b.rank))
    .map((entry) => entry.item);
}

/**
 * Swaps `id` with its visible neighbour `offset` away, keeping preferences for entries this
 * project doesn't show. Returns `order` itself when the move is out of range.
 */
export function moveCommandPaletteItem(
  order: readonly string[],
  id: string,
  offset: number,
  items: readonly CommandPaletteItem[],
): readonly string[] {
  const visibleIds = items.map(itemId);
  const index = visibleIds.indexOf(id);
  const neighbour = visibleIds[index + offset];
  if (index < 0 || neighbour === undefined) return order;
  const next = [...new Set([...order, ...visibleIds])];
  const source = next.indexOf(id);
  const destination = next.indexOf(neighbour);
  next[source] = neighbour;
  next[destination] = id;
  return next;
}
