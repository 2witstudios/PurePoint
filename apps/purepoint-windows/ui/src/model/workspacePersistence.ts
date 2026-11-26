// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/WorkspacePersistence.swift (WIN-6.3).
//
// Reads and writes `.pu/workspaces.json`, the per-project layout document the Mac app, the
// Windows app and the CLI (`pu grid`) share. The file is the only place pane grouping is stored.
// Writes are byte-identical to the Mac's `JSONEncoder` output (see `swiftJson.ts`), and decoding
// follows the Swift Codable types: a document Swift would reject is rejected here too.
//
// Loading never fabricates grouping: a missing or unreadable file yields an empty list, and the
// reconciler then adopts every manifest agent into its own workspace.
//
// The functions are pure; file access goes through an injected `WorkspaceFileIO` (or
// `LayoutHostIO` for the conditional saves), so the registry (WIN-6.2) can wire either the Tauri
// filesystem or a remote host to them.
//
// Swift's `try?` maps every failure to `nil`; here that is `null`, and the IO functions catch the
// same errors Swift's `try?` swallows.

import {
  allLeafIds,
  firstLeafId,
  leaf,
  split,
  type PaneAxis,
  type PaneSplitNode,
} from './paneSplit';
import { encodeSwiftJson, type JsonValue } from './swiftJson';
import {
  EMPTY_CONTENT,
  POINT_GUARD_CONTENT,
  PROJECT_ROOT,
  agentContent,
  containerWorktreeId,
  contentAgentId,
  fileContent,
  makeWorkspace,
  pointGuardId,
  worktreeContainer,
  type Pane,
  type Surface,
  type SurfaceContent,
  type Workspace,
} from './workspace';

// MARK: - Document types (the Swift Codable types, field for field)

/**
 * Version 4 adds Point Guard tabs (`kind: "point_guard"`). A version 3 build reading one would
 * load it as an empty tab and save it back as one, so every build refuses to overwrite a document
 * newer than it understands (`isNewerThanSupported`).
 */
export const CURRENT_DOCUMENT_VERSION = 4;

/** Swift `PersistedSurface`. `kind` is `agent`, `file`, `point_guard` or `empty`. */
export interface PersistedSurface {
  readonly id: number;
  readonly kind: string;
  readonly agentId?: string;
  readonly path?: string;
}

/**
 * Swift `GridLayoutNode`. A leaf carries its pane's tab stack; version 2 stored a single
 * `agentId` on the leaf instead, which still decodes so those files migrate in place.
 */
export interface GridLayoutNode {
  readonly type: 'leaf' | 'split';
  /** Optional so layouts written before pane IDs were stored still load. */
  readonly leafId?: number;
  /** Version 2 only: the one agent a leaf held before panes had tabs. */
  readonly agentId?: string;
  readonly activeTabId?: number;
  readonly tabs?: readonly PersistedSurface[];
  readonly axis?: PaneAxis;
  readonly ratio?: number;
  readonly first?: GridLayoutNode;
  readonly second?: GridLayoutNode;
}

/** Version 2 only: a file pane's leaf and the file it had open. */
export interface PersistedFilePane {
  readonly leafId: number;
  readonly path?: string;
}

/** Swift `PersistedWorkspace`: one workspace as written to `.pu/workspaces.json`. */
export interface PersistedWorkspace {
  readonly id: string;
  /** Absent means the workspace lives in the project root rather than a worktree. */
  readonly worktreeId?: string;
  readonly focusedLeafId: number;
  readonly nextLeafId: number;
  /** Optional so version 2 documents, written before tabs existed, still decode. */
  readonly nextSurfaceId?: number;
  readonly tree: GridLayoutNode;
  /** Version 2 only: leaves showing files, before a file became a kind of tab. Never written. */
  readonly filePanes?: readonly PersistedFilePane[];
}

/** Swift `PersistedWorkspaceDocument`: the whole per-project layout document. */
export interface PersistedWorkspaceDocument {
  readonly version: number;
  readonly workspaces: readonly PersistedWorkspace[];
  /**
   * The workspace on screen when this was written: where `pu grid` commands without
   * `--workspace` land. Read by the CLI only; the app restores selection from its own store
   * (`workspaceSelection.ts`).
   */
  readonly activeWorkspaceId?: string;
}

/** Legacy `.pu/grid-layout.json`: a single app-wide grid owned by one agent. */
export interface LegacyPersistedGridLayout {
  readonly ownerAgentId?: string;
  readonly tree: GridLayoutNode;
}

// MARK: - Paths

function puDirectory(projectRoot: string): string {
  // NSString.appendingPathComponent drops a trailing separator before appending.
  const trimmed = projectRoot.length > 1 ? projectRoot.replace(/[/\\]+$/, '') : projectRoot;
  return trimmed === '/' ? '/.pu' : `${trimmed}/.pu`;
}

export function workspaceFilePath(projectRoot: string): string {
  return `${puDirectory(projectRoot)}/workspaces.json`;
}

export function legacyGridLayoutPath(projectRoot: string): string {
  return `${puDirectory(projectRoot)}/grid-layout.json`;
}

// MARK: - Decoding (Swift JSONDecoder semantics)

/** Thrown inside the decoders, like `DecodingError`; callers turn it into `null` (`try?`). */
class DecodeError extends Error {}

type JsonObject = Readonly<Record<string, unknown>>;

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new DecodeError('not JSON');
  }
}

function asObject(value: unknown): JsonObject {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new DecodeError('expected an object');
  }
  return value as JsonObject;
}

function asArray(value: unknown): readonly unknown[] {
  if (!Array.isArray(value)) throw new DecodeError('expected an array');
  return value;
}

function asInt(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value)) {
    throw new DecodeError('expected an integer');
  }
  return value;
}

function asDouble(value: unknown): number {
  if (typeof value !== 'number') throw new DecodeError('expected a number');
  return value;
}

function asString(value: unknown): string {
  if (typeof value !== 'string') throw new DecodeError('expected a string');
  return value;
}

/** `decode(_:forKey:)`: the key must be present and non-null. */
function required<T>(object: JsonObject, key: string, decode: (value: unknown) => T): T {
  const value = object[key];
  if (value === undefined || value === null) throw new DecodeError(`missing ${key}`);
  return decode(value);
}

/** `decodeIfPresent(_:forKey:)`: absent or `null` is nil; any other wrong type throws. */
function optional<T>(
  object: JsonObject,
  key: string,
  decode: (value: unknown) => T,
): T | undefined {
  const value = object[key];
  if (value === undefined || value === null) return undefined;
  return decode(value);
}

/** Spread helper so `exactOptionalPropertyTypes` objects never carry `key: undefined`. */
function present<K extends string, V>(key: K, value: V | undefined): Partial<Record<K, V>> {
  return value === undefined ? {} : ({ [key]: value } as Record<K, V>);
}

function decodeSurface(value: unknown): PersistedSurface {
  const object = asObject(value);
  return {
    id: required(object, 'id', asInt),
    kind: required(object, 'kind', asString),
    ...present('agentId', optional(object, 'agentId', asString)),
    ...present('path', optional(object, 'path', asString)),
  };
}

function decodeNodeType(value: unknown): 'leaf' | 'split' {
  const raw = asString(value);
  if (raw !== 'leaf' && raw !== 'split') throw new DecodeError(`unknown node type ${raw}`);
  return raw;
}

function decodeAxis(value: unknown): PaneAxis {
  const raw = asString(value);
  if (raw !== 'horizontal' && raw !== 'vertical') throw new DecodeError(`unknown axis ${raw}`);
  return raw;
}

function decodeNode(value: unknown): GridLayoutNode {
  const object = asObject(value);
  return {
    type: required(object, 'type', decodeNodeType),
    ...present('leafId', optional(object, 'leafId', asInt)),
    ...present('agentId', optional(object, 'agentId', asString)),
    ...present('activeTabId', optional(object, 'activeTabId', asInt)),
    ...present(
      'tabs',
      optional(object, 'tabs', (tabs) => asArray(tabs).map(decodeSurface)),
    ),
    ...present('axis', optional(object, 'axis', decodeAxis)),
    ...present('ratio', optional(object, 'ratio', asDouble)),
    ...present('first', optional(object, 'first', decodeNode)),
    ...present('second', optional(object, 'second', decodeNode)),
  };
}

function decodeFilePane(value: unknown): PersistedFilePane {
  const object = asObject(value);
  return {
    leafId: required(object, 'leafId', asInt),
    ...present('path', optional(object, 'path', asString)),
  };
}

function decodeWorkspace(value: unknown): PersistedWorkspace {
  const object = asObject(value);
  return {
    id: required(object, 'id', asString),
    ...present('worktreeId', optional(object, 'worktreeId', asString)),
    focusedLeafId: required(object, 'focusedLeafId', asInt),
    nextLeafId: required(object, 'nextLeafId', asInt),
    ...present('nextSurfaceId', optional(object, 'nextSurfaceId', asInt)),
    tree: required(object, 'tree', decodeNode),
    ...present(
      'filePanes',
      optional(object, 'filePanes', (panes) => asArray(panes).map(decodeFilePane)),
    ),
  };
}

function attempt<T>(decode: () => T): T | null {
  try {
    return decode();
  } catch (error) {
    if (error instanceof DecodeError) return null;
    throw error;
  }
}

/** `try? JSONDecoder().decode(PersistedWorkspaceDocument.self, from:)`. */
export function decodeWorkspaceDocument(text: string): PersistedWorkspaceDocument | null {
  return attempt(() => {
    const object = asObject(parseJson(text));
    return {
      version: required(object, 'version', asInt),
      workspaces: required(object, 'workspaces', (list) => asArray(list).map(decodeWorkspace)),
      ...present('activeWorkspaceId', optional(object, 'activeWorkspaceId', asString)),
    };
  });
}

/** `try? JSONDecoder().decode(GridLayoutNode.self, from:)`. */
export function decodeGridLayoutNode(text: string): GridLayoutNode | null {
  return attempt(() => decodeNode(parseJson(text)));
}

/** `try? JSONDecoder().decode(LegacyPersistedGridLayout.self, from:)`. */
export function decodeLegacyGridLayout(text: string): LegacyPersistedGridLayout | null {
  return attempt(() => {
    const object = asObject(parseJson(text));
    return {
      ...present('ownerAgentId', optional(object, 'ownerAgentId', asString)),
      tree: required(object, 'tree', decodeNode),
    };
  });
}

/**
 * The version probe: just the version, readable from any document, including one too new to
 * decode. `null` when there is no integer `version`.
 */
export function probeDocumentVersion(text: string): number | null {
  return attempt(() => required(asObject(parseJson(text)), 'version', asInt));
}

/** A document written by a newer build. Saving over it would downgrade it, so callers must not. */
export function isNewerThanSupported(text: string | null): boolean {
  if (text === null) return false;
  const version = probeDocumentVersion(text);
  return version !== null && version > CURRENT_DOCUMENT_VERSION;
}

// MARK: - Surfaces

/** Swift `PersistedSurface.init(_ surface:)`. */
export function persistedSurface(surface: Surface): PersistedSurface {
  const { id, content } = surface;
  switch (content.kind) {
    case 'empty':
      return { id, kind: 'empty' };
    case 'pointGuard':
      return { id, kind: 'point_guard' };
    case 'agent':
      return { id, kind: 'agent', agentId: content.agentId };
    case 'file':
      return { id, kind: 'file', ...present('path', content.path ?? undefined) };
  }
}

/** Swift `PersistedSurface.surface`: an unknown kind (a newer app's file) loads as an empty tab. */
export function surfaceFromPersisted(persisted: PersistedSurface): Surface {
  let content: SurfaceContent;
  switch (persisted.kind) {
    case 'agent':
      content = persisted.agentId === undefined ? EMPTY_CONTENT : agentContent(persisted.agentId);
      break;
    case 'file':
      content = fileContent(persisted.path ?? null);
      break;
    case 'point_guard':
      content = POINT_GUARD_CONTENT;
      break;
    default:
      content = EMPTY_CONTENT;
  }
  return { id: persisted.id, content };
}

// MARK: - Tree coding

/**
 * Leaf IDs are written to disk. They are the handles the daemon's grid protocol addresses panes
 * by, so a restart that renumbered them would silently retarget every in-flight `pu grid`
 * command. Surface IDs are written for the same reason.
 */
export function layoutNode(node: PaneSplitNode, panes: Workspace['panes']): GridLayoutNode {
  if (node.type === 'leaf') {
    const pane = panes[node.id];
    return {
      type: 'leaf',
      leafId: node.id,
      ...present('activeTabId', pane?.activeTabId),
      tabs: (pane?.tabs ?? []).map(persistedSurface),
    };
  }
  return {
    type: 'split',
    axis: node.axis,
    ratio: node.ratio,
    first: layoutNode(node.first, panes),
    second: layoutNode(node.second, panes),
  };
}

interface TreeCounters {
  nextLeafId: number;
  nextSurfaceId: number;
}

/**
 * Rebuild geometry and tab stacks from disk. Persisted IDs are reused; the counters only supply
 * IDs for older files that lack them. A version 2 leaf becomes a single tab: its agent, else its
 * file pane, else an empty tab. `legacyFilePaths` maps a leaf to its file (`null` = navigator).
 */
function treeFrom(
  node: GridLayoutNode,
  panes: Map<number, Pane>,
  legacyFilePaths: ReadonlyMap<number, string | null>,
  counters: TreeCounters,
): PaneSplitNode {
  if (
    node.type === 'split' &&
    node.axis !== undefined &&
    node.ratio !== undefined &&
    node.first !== undefined &&
    node.second !== undefined
  ) {
    const first = treeFrom(node.first, panes, legacyFilePaths, counters);
    const second = treeFrom(node.second, panes, legacyFilePaths, counters);
    return split(node.axis, node.ratio, first, second);
  }

  // Anything else, including a split missing a field, is a leaf (as in Swift).
  const id = node.leafId ?? counters.nextLeafId;
  counters.nextLeafId = Math.max(counters.nextLeafId, id + 1);

  const stored = node.tabs ?? [];
  const firstStored = stored[0];
  if (firstStored !== undefined) {
    const tabs = stored.map(surfaceFromPersisted);
    counters.nextSurfaceId = Math.max(
      counters.nextSurfaceId,
      Math.max(-1, ...tabs.map((tab) => tab.id)) + 1,
    );
    panes.set(id, { tabs, activeTabId: node.activeTabId ?? firstStored.id });
  } else {
    let content: SurfaceContent;
    if (node.agentId !== undefined) {
      content = agentContent(node.agentId);
    } else if (legacyFilePaths.has(id)) {
      content = fileContent(legacyFilePaths.get(id) ?? null);
    } else {
      content = EMPTY_CONTENT;
    }
    const surface = { id: counters.nextSurfaceId, content };
    counters.nextSurfaceId += 1;
    panes.set(id, { tabs: [surface], activeTabId: surface.id });
  }
  return leaf(id);
}

function panesRecord(panes: Map<number, Pane>): Record<number, Pane> {
  const record: Record<number, Pane> = {};
  for (const [leafId, pane] of panes) record[leafId] = pane;
  return record;
}

/** Swift `workspace(from:projectRoot:)`. `projectId` re-keys Point Guard workspaces to this app. */
export function workspaceFromPersisted(
  persisted: PersistedWorkspace,
  projectId: string,
): Workspace {
  const counters = {
    nextLeafId: persisted.nextLeafId,
    nextSurfaceId: persisted.nextSurfaceId ?? 0,
  };
  const filePaths = new Map<number, string | null>();
  for (const pane of persisted.filePanes ?? []) {
    if (!filePaths.has(pane.leafId)) filePaths.set(pane.leafId, pane.path ?? null);
  }
  const panes = new Map<number, Pane>();
  const root = treeFrom(persisted.tree, panes, filePaths, counters);
  return makeWorkspace({
    id: persisted.id.startsWith('ws-point-guard:') ? pointGuardId(projectId) : persisted.id,
    container:
      persisted.worktreeId === undefined ? PROJECT_ROOT : worktreeContainer(persisted.worktreeId),
    root,
    panes: panesRecord(panes),
    focusedLeafId: persisted.focusedLeafId,
    nextLeafId: counters.nextLeafId,
    nextSurfaceId: counters.nextSurfaceId,
  });
}

// MARK: - Encoding

/** The document `documentData` encodes. Always the current version; `filePanes` is never written. */
export function persistedDocument(
  workspaces: readonly Workspace[],
  activeWorkspaceId: string | null = null,
): PersistedWorkspaceDocument {
  return {
    version: CURRENT_DOCUMENT_VERSION,
    workspaces: workspaces.map((workspace) => ({
      id: workspace.id,
      ...present('worktreeId', containerWorktreeId(workspace.container) ?? undefined),
      focusedLeafId: workspace.focusedLeafId,
      nextLeafId: workspace.nextLeafId,
      nextSurfaceId: workspace.nextSurfaceId,
      tree: layoutNode(workspace.root, workspace.panes),
    })),
    ...present('activeWorkspaceId', activeWorkspaceId ?? undefined),
  };
}

/** `JSONEncoder` with `[.prettyPrinted, .sortedKeys]` over a document. `null` if it cannot encode. */
export function encodeWorkspaceDocument(document: PersistedWorkspaceDocument): string | null {
  try {
    return encodeSwiftJson(document as unknown as JsonValue);
  } catch {
    return null;
  }
}

/** Swift `documentData`: the encoded layout document, shared by local and remote saves. */
export function documentText(
  workspaces: readonly Workspace[],
  activeWorkspaceId: string | null = null,
): string | null {
  return encodeWorkspaceDocument(persistedDocument(workspaces, activeWorkspaceId));
}

/** Swift `workspaces(from:projectId:)`: decode a document; anything undecodable is no layout. */
export function workspacesFromText(text: string, projectId: string): Workspace[] {
  const document = decodeWorkspaceDocument(text);
  if (document === null) return [];
  return document.workspaces.map((workspace) => workspaceFromPersisted(workspace, projectId));
}

/**
 * `workspaces` as a document read back would give them, so they compare with one (the registry's
 * `decoded`). The registry saves only when its published layout differs from this.
 */
export function decodedWorkspaces(
  workspaces: readonly Workspace[],
  projectId: string,
): Workspace[] {
  const text = documentText(workspaces);
  return text === null ? [...workspaces] : workspacesFromText(text, projectId);
}

// MARK: - File IO

/**
 * The file access persistence needs. `read` resolves `null` for a missing file (and may reject
 * for other failures, which are treated like Swift's `try?`: as no file). `remove` retires the
 * legacy grid layout; without it the legacy file is left in place.
 */
export interface WorkspaceFileIO {
  read(path: string): Promise<string | null>;
  /** Atomic, creating `.pu/` when missing. */
  write(path: string, text: string): Promise<void>;
  remove?(path: string): Promise<void>;
}

async function readOrNull(io: WorkspaceFileIO, path: string): Promise<string | null> {
  try {
    return await io.read(path);
  } catch {
    return null;
  }
}

async function removeQuietly(io: WorkspaceFileIO, path: string): Promise<void> {
  try {
    await io.remove?.(path);
  } catch {
    // `try? FileManager.default.removeItem`.
  }
}

/**
 * Swift `save`: write the layout unless the file on disk was written by a newer build. Resolves
 * whether it wrote. Write failures are swallowed like Swift's `try?`.
 */
export async function saveWorkspaces(
  io: WorkspaceFileIO,
  projectRoot: string,
  workspaces: readonly Workspace[],
  activeWorkspaceId: string | null = null,
): Promise<boolean> {
  const path = workspaceFilePath(projectRoot);
  if (isNewerThanSupported(await readOrNull(io, path))) return false;
  const text = documentText(workspaces, activeWorkspaceId);
  if (text === null) return false;
  try {
    await io.write(path, text);
    return true;
  } catch {
    return false;
  }
}

/** Swift `load`: the layout, or an empty list when it cannot be read. */
export async function loadWorkspaces(
  io: WorkspaceFileIO,
  projectRoot: string,
): Promise<Workspace[]> {
  try {
    return await loadWorkspacesForRegistry(io, projectRoot);
  } catch {
    return [];
  }
}

/**
 * Swift `loadForRegistry`. Rejects only when a Point Guard identity migration could not be
 * written: the registry must not publish a new identity until its write has succeeded. A missing
 * or undecodable `workspaces.json` falls back to migrating the legacy `grid-layout.json`.
 */
export async function loadWorkspacesForRegistry(
  io: WorkspaceFileIO,
  projectRoot: string,
): Promise<Workspace[]> {
  const path = workspaceFilePath(projectRoot);
  const text = await readOrNull(io, path);
  const document = text === null ? null : decodeWorkspaceDocument(text);
  if (text !== null && document !== null) {
    const restored = document.workspaces.map((workspace) =>
      workspaceFromPersisted(workspace, projectRoot),
    );
    // Copied/renamed project directories keep their layout, but must not publish an ID belonging
    // to another open project. Persist the new ID for CLI routing too.
    if (document.workspaces.some((workspace, index) => workspace.id !== restored[index]?.id)) {
      await io.write(path, migrateWorkspaceIds(text, restored));
    }
    return restored;
  }
  return migrateLegacyLayout(io, projectRoot);
}

/**
 * Identity-only migration also works for decodable newer documents. Re-encoding the typed model
 * would drop unknown fields and surface kinds, so the raw JSON is patched and keeps its version.
 * Swift re-serializes with `JSONSerialization` and the same formatting options.
 */
function migrateWorkspaceIds(text: string, restored: readonly Workspace[]): string {
  const document = JSON.parse(text) as unknown;
  if (typeof document !== 'object' || document === null || Array.isArray(document)) {
    throw new Error('The layout file is corrupt.');
  }
  const record = document as Record<string, JsonValue>;
  const workspaces = record.workspaces;
  if (
    !Array.isArray(workspaces) ||
    workspaces.length !== restored.length ||
    !workspaces.every((item) => typeof item === 'object' && item !== null && !Array.isArray(item))
  ) {
    throw new Error('The layout file is corrupt.');
  }
  const activeId = typeof record.activeWorkspaceId === 'string' ? record.activeWorkspaceId : null;
  const patched = (workspaces as readonly Record<string, JsonValue>[]).map((workspace, index) => {
    const id = restored[index]?.id ?? '';
    if (activeId !== null && workspace.id === activeId) record.activeWorkspaceId = id;
    return { ...workspace, id };
  });
  return encodeSwiftJson({ ...record, workspaces: patched });
}

/**
 * Fold a pre-workspace `grid-layout.json` into a single workspace, then retire the file. Agents
 * that file never mentioned are adopted by the reconciler, so nothing is lost.
 */
async function migrateLegacyLayout(io: WorkspaceFileIO, projectRoot: string): Promise<Workspace[]> {
  const legacyPath = legacyGridLayoutPath(projectRoot);
  const text = await readOrNull(io, legacyPath);
  if (text === null) return [];

  let tree: GridLayoutNode;
  let ownerAgentId: string | null;
  const legacy = decodeLegacyGridLayout(text);
  const bare = legacy === null ? decodeGridLayoutNode(text) : null;
  if (legacy !== null) {
    tree = legacy.tree;
    ownerAgentId = legacy.ownerAgentId ?? null;
  } else if (bare !== null) {
    tree = bare;
    ownerAgentId = null;
  } else {
    return [];
  }

  const counters = { nextLeafId: 0, nextSurfaceId: 0 };
  const panes = new Map<number, Pane>();
  const root = treeFrom(tree, panes, new Map(), counters);
  const firstAgentId =
    allLeafIds(root)
      .map((leafId) => {
        const first = panes.get(leafId)?.tabs[0];
        return first === undefined ? null : contentAgentId(first.content);
      })
      .find((agentId) => agentId !== null) ?? null;
  const anchorAgentId = ownerAgentId ?? firstAgentId;
  if (anchorAgentId === null) {
    await removeQuietly(io, legacyPath);
    return [];
  }

  const workspace = makeWorkspace({
    id: `ws-${anchorAgentId}`,
    container: PROJECT_ROOT, // reconcile corrects this from the manifest
    root,
    panes: panesRecord(panes),
    focusedLeafId: firstLeafId(root),
    nextLeafId: counters.nextLeafId,
    nextSurfaceId: counters.nextSurfaceId,
  });

  const migrated = [workspace];
  await saveWorkspaces(io, projectRoot, migrated);
  await removeQuietly(io, legacyPath);
  return migrated;
}

/** Swift `clear`. */
export async function clearWorkspaces(io: WorkspaceFileIO, projectRoot: string): Promise<void> {
  await removeQuietly(io, workspaceFilePath(projectRoot));
}

// MARK: - Conditional saves (another machine writes the same file)

/** The layout files are capped at 4 MiB when read through a host. */
export const MAX_LAYOUT_BYTES = 4 * 1_048_576;

/** A file read through a host, with its modification time. */
export interface HostFileText {
  readonly text: string;
  readonly modifiedMs: number;
  /** The file was larger than `maxBytes`; `text` is incomplete. */
  readonly truncated: boolean;
}

/** `HostError.conflict`: the file changed since `expectedModifiedMs`. */
export class LayoutWriteConflictError extends Error {
  constructor() {
    super('The layout file changed since it was read.');
    this.name = 'LayoutWriteConflictError';
  }
}

/**
 * Swift `ProjectHost`, as far as layout saves use it. `stat` and `read` resolve `null` for a
 * missing file and reject for any other failure. `write` writes only if the file's modification
 * time still equals `expectedModifiedMs` (`null`: only if there is no file), rejecting with
 * `LayoutWriteConflictError` otherwise, and resolves the new modification time.
 */
export interface LayoutHostIO {
  stat(path: string): Promise<{ readonly modifiedMs: number } | null>;
  read(path: string, maxBytes: number): Promise<HostFileText | null>;
  write(path: string, text: string, expectedModifiedMs: number | null): Promise<number>;
}

/** What a conditional layout save did (Swift `RemoteSaveOutcome`). */
export type LayoutSaveOutcome =
  | { readonly kind: 'written'; readonly modifiedMs: number }
  /** The document changed since this app last read or wrote it; it carries that document. */
  | { readonly kind: 'conflict'; readonly text: string; readonly modifiedMs: number }
  /** Nothing written: the document could not be read safely, or is a newer version. */
  | { readonly kind: 'skipped' };

const SKIPPED: LayoutSaveOutcome = { kind: 'skipped' };

async function readUsable(host: LayoutHostIO, path: string): Promise<HostFileText | null> {
  try {
    const current = await host.read(path, MAX_LAYOUT_BYTES);
    if (current === null || current.truncated || isNewerThanSupported(current.text)) return null;
    return current;
  } catch {
    return null;
  }
}

/**
 * Swift `saveLocal`: write a local project's layout unless the document on disk changed since
 * this app last read or wrote it (`expectedModifiedMs`, `null` when there was no document).
 * Another machine reaching this one writes the same file; its change comes back as a conflict for
 * the caller to adopt. A document deleted meanwhile has nothing to adopt, so it is written again.
 */
export async function saveLocalLayout(
  host: LayoutHostIO,
  text: string,
  projectRoot: string,
  expectedModifiedMs: number | null,
): Promise<LayoutSaveOutcome> {
  const path = workspaceFilePath(projectRoot);
  let onDisk: number | null;
  try {
    onDisk = (await host.stat(path))?.modifiedMs ?? null;
  } catch {
    return SKIPPED;
  }
  if (onDisk !== null && onDisk !== expectedModifiedMs) {
    const current = await readUsable(host, path);
    return current === null
      ? SKIPPED
      : { kind: 'conflict', text: current.text, modifiedMs: current.modifiedMs };
  }
  if (onDisk !== null) {
    let existing: HostFileText | null;
    try {
      existing = await host.read(path, MAX_LAYOUT_BYTES);
    } catch {
      existing = null; // `isNewerThanSupported(projectRoot:)` reads with `try?`
    }
    if (isNewerThanSupported(existing?.text ?? null)) return SKIPPED;
  }
  try {
    return { kind: 'written', modifiedMs: await host.write(path, text, onDisk) };
  } catch {
    return SKIPPED;
  }
}

/**
 * Swift `saveRemote`: save through a host, never downgrading a newer document. When the document
 * changed since `expectedModifiedMs` nothing is written and the outcome carries it. Nothing is
 * written either when the current document cannot be read (anything but a missing file).
 */
export async function saveRemoteLayout(
  host: LayoutHostIO,
  text: string,
  projectRoot: string,
  expectedModifiedMs: number | null,
): Promise<LayoutSaveOutcome> {
  const path = workspaceFilePath(projectRoot);
  let onDisk: number | null;
  try {
    const existing = await host.read(path, MAX_LAYOUT_BYTES);
    if (existing === null) {
      onDisk = null;
    } else {
      if (existing.truncated || isNewerThanSupported(existing.text)) return SKIPPED;
      // A document this app has not seen (none when it last read) is someone else's.
      if (existing.modifiedMs !== expectedModifiedMs) {
        return { kind: 'conflict', text: existing.text, modifiedMs: existing.modifiedMs };
      }
      onDisk = existing.modifiedMs;
    }
  } catch {
    return SKIPPED;
  }
  try {
    return { kind: 'written', modifiedMs: await host.write(path, text, onDisk) };
  } catch (error) {
    if (!(error instanceof LayoutWriteConflictError)) return SKIPPED;
    // Changed between the read and the write: adopt what is there now.
    const current = await readUsable(host, path);
    return current === null
      ? SKIPPED
      : { kind: 'conflict', text: current.text, modifiedMs: current.modifiedMs };
  }
}

/** Thrown by `loadRemoteLayout` when the document is over `MAX_LAYOUT_BYTES`. */
export class LayoutTooLargeError extends Error {
  constructor() {
    super('The layout file on the remote machine is larger than 4 MB.');
    this.name = 'LayoutTooLargeError';
  }
}

/**
 * Swift `loadRemote`. Only a missing file is an empty layout; any other failure rejects, because
 * an empty layout adopted after a failed read would be saved over the real one.
 */
export async function loadRemoteLayout(
  host: LayoutHostIO,
  projectRoot: string,
  projectId: string,
): Promise<{ workspaces: Workspace[]; modifiedMs: number | null }> {
  const read = await host.read(workspaceFilePath(projectRoot), MAX_LAYOUT_BYTES);
  if (read === null) return { workspaces: [], modifiedMs: null };
  if (read.truncated) throw new LayoutTooLargeError();
  return { workspaces: workspacesFromText(read.text, projectId), modifiedMs: read.modifiedMs };
}

// MARK: - In-memory IO (tests, and callers with no filesystem)

/** A `WorkspaceFileIO` over a map of path to contents. */
export function memoryWorkspaceFileIO(
  files: Map<string, string> = new Map(),
): WorkspaceFileIO & { readonly files: Map<string, string> } {
  return {
    files,
    read: (path) => Promise.resolve(files.get(path) ?? null),
    write: (path, text) => {
      files.set(path, text);
      return Promise.resolve();
    },
    remove: (path) => {
      files.delete(path);
      return Promise.resolve();
    },
  };
}
