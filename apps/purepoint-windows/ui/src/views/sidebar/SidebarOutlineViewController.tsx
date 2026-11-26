// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineViewController.swift
// (WIN-7.2).
//
// The project tree: projects, their worktrees, the workspaces (agents) in each, and a MACHINES
// section. The Mac hosts an `NSOutlineView`; here it is a virtualized list of 24px rows drawn from
// the outline model (`model/sidebarOutline.ts`), which already holds everything platform-free —
// the tree, which rows are open, what a row's selection means. This file only draws it and turns
// clicks and keys into the model's operations.
//
// Differences from Swift:
// - Rows are windowed by hand (only the rows in view, plus a margin, are in the DOM), as
//   `NSOutlineView` reuses its cell views; the tree itself is never virtualized.
// - Expansion is not held by a view: a row is open unless the expansion store records it closed,
//   so the Mac's replay after `reloadData()` and its collapse-cascade filtering are not needed.
//   Closing a row that holds the selection selects that row, as `NSOutlineView` does.
// - The keyboard is the outline's own: arrows (the `SidebarKeyNavigation` rules the iPad port
//   wrote down from `NSOutlineView`), Enter, F2 (rename) and Delete. Delete runs the destructive
//   item of the row's menu — Kill Agent / Close Workspace, Delete Worktree, Remove Project,
//   Remove Machine — with the confirmation that menu item asks for.
// - Context menus, the worktree add menu and project drag are WIN-7.8; the add buttons open the
//   command palette directly (`onShowCommandPalette`).
// - The agent signal pill shows a provisional presentation until WIN-7.7 ports
//   `AgentModel.signalPresentation`; callers can pass their own `presentSignal`.

import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useStore } from 'zustand';
import {
  type MachineSnapshot,
  type ProjectSnapshot,
  type SidebarNode,
  type SidebarSelection,
  isExpandable,
  projectName,
  selectionsEqual,
} from '../../model/sidebarItem.ts';
import {
  type SidebarConfirmation,
  type SidebarMenuContext,
  sidebarConfirmation,
} from '../../model/sidebarMenu.ts';
import {
  type WorkspaceRenderState,
  type WorkspaceSignal,
  type WorkspacesByProject,
  buildSidebarTree,
  isSelectable,
  isSettled,
  makeSidebarRenderState,
  selectionFor,
  sidebarAncestors,
  sidebarExpansion,
  sidebarStatusColor,
  targetId,
  workspaceRenderState,
} from '../../model/sidebarOutline.ts';
import { type Workspace, primaryAgentId, workspaceAgentIds } from '../../model/workspace.ts';
import { structurallyEqual } from '../../model/workspaceModel.ts';
import {
  type createSidebarExpansionStore,
  useSidebarExpansionStore,
} from '../../state/sidebarExpansionStore.ts';
import { Icon } from '../../theme/Icon.tsx';
import './sidebar.css';

/** `PurePointTheme.sidebarRowHeight`. */
export const SIDEBAR_ROW_HEIGHT = 24;
/** `NSOutlineView.indentationPerLevel`. */
const INDENTATION_PER_LEVEL = 12;
/** Rows drawn above and below the visible ones, so a scroll never shows a gap. */
const OVERSCAN_ROWS = 8;
/** The viewport assumed before one has been measured (tests have no layout). */
const FALLBACK_VIEWPORT_HEIGHT = 800;

export type SidebarExpansionStoreApi = ReturnType<typeof createSidebarExpansionStore>;

/** What a row's signal pill shows (Swift `AgentSignalPresentation`). */
export interface SignalPill {
  readonly label: string;
  readonly needsAttention: boolean;
  readonly accessibilityDescription: string;
}

const UNKNOWN_PILL: SignalPill = {
  label: 'Unknown',
  needsAttention: false,
  accessibilityDescription: 'Unknown',
};

/**
 * The pill until WIN-7.7 ports `signalPresentation(available:)`: unknown when the project cannot
 * be reached or nothing is observed, otherwise the observed activity.
 */
export function provisionalSignalPill(signal: WorkspaceSignal): SignalPill {
  const snapshot = signal.snapshot;
  if (!signal.available || snapshot === null) return UNKNOWN_PILL;
  const needsAttention = snapshot.pending_attention.length > 0;
  switch (snapshot.activity) {
    case 'working':
      return { label: 'Working', needsAttention, accessibilityDescription: 'Working' };
    case 'idle':
      return { label: 'Idle', needsAttention, accessibilityDescription: 'Idle' };
    case 'unknown':
      return { ...UNKNOWN_PILL, needsAttention };
  }
}

/** How a row was chosen. */
export interface SidebarSelectOptions {
  readonly additive?: boolean;
}

/** What the controller reports (Swift's `on…` callbacks), addressed by ids. */
export interface SidebarOutlineCallbacks {
  /**
   * A row was chosen; null clears the selection. `additive` is a Ctrl+click, which adds a
   * workspace's agents to the terminals on screen rather than replacing them.
   */
  readonly onSelectionChanged?: (
    selection: SidebarSelection | null,
    options?: SidebarSelectOptions,
  ) => void;
  /** An add button: (project, selection context, whether worktree variants are offered). */
  readonly onShowCommandPalette?: (
    projectId: string,
    selection: SidebarSelection | null,
    includeWorktree: boolean,
  ) => void;
  /** Kill every agent in a workspace and close it: (project, workspace). */
  readonly onKillWorkspace?: (projectId: string, workspaceId: string) => void;
  /** Rename a workspace's primary agent: (project, agent, new name). */
  readonly onRenameAgent?: (projectId: string, agentId: string, name: string) => void;
  readonly onDeleteWorktree?: (projectId: string, worktreeId: string) => void;
  readonly onRemoveProject?: (projectId: string) => void;
  readonly onRemoveMachine?: (machineId: string) => void;
  /** Enter on a machine row (Swift `toggleConnection(of:)`). */
  readonly onToggleMachineConnection?: (machineId: string) => void;
}

export interface SidebarOutlineViewControllerProps extends SidebarOutlineCallbacks {
  readonly projects: readonly ProjectSnapshot[];
  readonly machines?: readonly MachineSnapshot[];
  /** Canonical workspaces per project key: the sidebar renders these and nothing else. */
  readonly workspacesByProject?: WorkspacesByProject;
  /** The workspace whose grid is on screen. */
  readonly activeWorkspaceId?: string | null;
  /** The shell's selection; the row it names is selected (programmatic selection). */
  readonly selection?: SidebarSelection | null;
  readonly expansionStore?: SidebarExpansionStoreApi;
  /** What the confirmations read beyond the row (machine names, Point Guard folders). */
  readonly menuContext?: SidebarMenuContext;
  readonly presentSignal?: (signal: WorkspaceSignal) => SignalPill;
}

/** One visible row. */
export interface SidebarRow {
  readonly node: SidebarNode;
  readonly depth: number;
  /** Index of the parent among the visible rows. */
  readonly parent: number | null;
  readonly isExpanded: boolean;
}

/** The rows on screen, top to bottom: every row whose ancestors are all open. */
export function visibleRows(
  nodes: readonly SidebarNode[],
  closedRowKeys: ReadonlySet<string>,
): SidebarRow[] {
  const rows: SidebarRow[] = [];
  const walk = (level: readonly SidebarNode[], depth: number, parent: number | null) => {
    for (const node of level) {
      const isExpanded = isExpandable(node) && !closedRowKeys.has(node.rowKey);
      const index = rows.length;
      rows.push({ node, depth, parent, isExpanded });
      if (isExpanded) walk(node.children, depth + 1, index);
    }
  };
  walk(nodes, 0, null);
  return rows;
}

// MARK: - Keyboard navigation (Swift `SidebarKeyNavigation`)

export type SidebarKey = 'up' | 'down' | 'left' | 'right';

export type SidebarKeyCommand =
  | { readonly kind: 'select'; readonly index: number }
  | { readonly kind: 'expand'; readonly index: number }
  | { readonly kind: 'collapse'; readonly index: number }
  | { readonly kind: 'none' };

/**
 * Arrow-key movement through the visible rows, as `NSOutlineView` moves its selection: up and down
 * step over rows that cannot be selected, right opens a closed row, and left closes an open row or
 * moves to the row's parent.
 */
export function sidebarKeyCommand(
  key: SidebarKey,
  rows: readonly SidebarRow[],
  selected: number | null,
): SidebarKeyCommand {
  const current = selected !== null && selected >= 0 && selected < rows.length ? selected : null;
  const selectable = (index: number) => {
    const row = rows[index];
    return row !== undefined && isSelectable(row.node);
  };
  switch (key) {
    case 'down': {
      for (let index = current === null ? 0 : current + 1; index < rows.length; index++) {
        if (selectable(index)) return { kind: 'select', index };
      }
      return { kind: 'none' };
    }
    case 'up': {
      for (let index = (current ?? rows.length) - 1; index >= 0; index--) {
        if (selectable(index)) return { kind: 'select', index };
      }
      return { kind: 'none' };
    }
    case 'left': {
      if (current === null) return { kind: 'none' };
      const row = rows[current];
      if (row === undefined) return { kind: 'none' };
      if (isExpandable(row.node) && row.isExpanded) return { kind: 'collapse', index: current };
      if (row.parent !== null && selectable(row.parent)) {
        return { kind: 'select', index: row.parent };
      }
      return { kind: 'none' };
    }
    case 'right': {
      if (current === null) return { kind: 'none' };
      const row = rows[current];
      if (row === undefined) return { kind: 'none' };
      return isExpandable(row.node) && !row.isExpanded
        ? { kind: 'expand', index: current }
        : { kind: 'none' };
    }
  }
}

const ARROW_KEYS: Readonly<Record<string, SidebarKey>> = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
};

// MARK: - Lookups

function findNode(
  nodes: readonly SidebarNode[],
  matches: (node: SidebarNode) => boolean,
): SidebarNode | null {
  for (const node of nodes) {
    if (matches(node)) return node;
    const found = findNode(node.children, matches);
    if (found !== null) return found;
  }
  return null;
}

/** The project a row belongs to (itself for a project row). */
function owningProject(nodes: readonly SidebarNode[], node: SidebarNode): ProjectSnapshot | null {
  if (node.kind === 'project') return node.project;
  const chain = sidebarAncestors((candidate) => candidate === node, nodes);
  for (let index = chain.length - 1; index >= 0; index--) {
    const ancestor = chain[index];
    if (ancestor?.kind === 'project') return ancestor.project;
  }
  return null;
}

function findWorkspace(
  workspacesByProject: WorkspacesByProject,
  workspaceId: string,
): { projectId: string; workspace: Workspace } | null {
  for (const [projectId, list] of Object.entries(workspacesByProject)) {
    const workspace = list?.find((candidate) => candidate.id === workspaceId);
    if (workspace !== undefined) return { projectId, workspace };
  }
  return null;
}

function rowDomId(rowKey: string): string {
  return 'sidebar-row-' + rowKey.replace(/[^\w-]/g, '_');
}

/**
 * A row's stable test id: `sidebar-row-<kind>-<name>`, named as the row reads (a project by its
 * folder, a worktree by its name, an agent by its title).
 */
function rowTestId(
  node: SidebarNode,
  project: ProjectSnapshot | null,
  workspace: WorkspaceRenderState | null,
): string {
  switch (node.kind) {
    case 'project':
      return `sidebar-row-project-${projectName(node.project)}`;
    case 'worktree':
      return `sidebar-row-worktree-${node.worktree.name}`;
    case 'workspace':
      return `sidebar-row-agent-${workspace?.title ?? project?.projectId ?? node.workspace.id}`;
    case 'machine':
      return `sidebar-row-machine-${node.machine.name}`;
    case 'sectionHeader':
      return `sidebar-row-section-${node.title}`;
  }
}

// MARK: - Cells

function StatusDot({ color }: { color: string }) {
  return <span className="sidebar-status-dot" data-color={color} aria-hidden="true" />;
}

function Badge({
  count,
  accent,
  label,
  testId,
}: {
  count: number;
  accent?: boolean;
  label?: string;
  testId?: string;
}) {
  return (
    <span
      className="sidebar-badge"
      data-accent={accent === true ? 'true' : undefined}
      data-testid={testId}
      aria-label={label}
    >
      {count}
    </span>
  );
}

function AddButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className="sidebar-add-button"
      aria-label={label}
      title={label}
      tabIndex={-1}
      onMouseDown={(event) => {
        event.stopPropagation();
      }}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <Icon name="plus.circle" size={14} />
    </button>
  );
}

interface CellProps {
  readonly node: SidebarNode;
  readonly project: ProjectSnapshot | null;
  readonly workspace: WorkspaceRenderState | null;
  readonly presentSignal: (signal: WorkspaceSignal) => SignalPill;
  readonly renameEditor: ReactNode;
  readonly onAdd: (node: SidebarNode) => void;
}

/** A row's content (Swift `make…Cell`) and its accessible name and tooltip. */
function cellFor({ node, workspace, presentSignal, renameEditor, onAdd }: CellProps): {
  content: ReactNode;
  label: string;
  tooltip?: string;
  unavailable?: boolean;
} {
  switch (node.kind) {
    case 'project': {
      const project = node.project;
      const name = projectName(project);
      let status: ReactNode = null;
      let label = name;
      let tooltip: string | undefined;
      if (project.isUnavailable) {
        status = <span className="sidebar-status-text">Unavailable</span>;
        tooltip =
          "The machine's daemon is unreachable. Unsaved file tabs are kept until it is back.";
        label = `${name}, unavailable`;
      } else if (node.project.layoutError !== null && project.projectId.startsWith('machine:')) {
        status = (
          <Icon
            name="exclamationmark.triangle.fill"
            size={12}
            className="sidebar-warning"
            label="Layout error"
          />
        );
        tooltip = node.project.layoutError + ' Retrying; choose Retry Layout to try now.';
        label = `${name}, ${node.project.layoutError}`;
      }
      return {
        label,
        ...(tooltip === undefined ? {} : { tooltip }),
        unavailable: project.isUnavailable,
        content: (
          <>
            <Icon name="folder.fill" size={13} className="sidebar-row-icon" />
            <span className="sidebar-row-name">{name}</span>
            <span className="sidebar-row-spacer" />
            {status}
            {project.channelUnread > 0 && (
              <Badge
                count={project.channelUnread}
                accent
                testId="channelUnreadCount"
                label={`${String(project.channelUnread)} unread channel messages`}
              />
            )}
            <AddButton
              label="Add"
              onClick={() => {
                onAdd(node);
              }}
            />
          </>
        ),
      };
    }
    case 'sectionHeader':
      return {
        label: 'Machines',
        content: <span className="sidebar-section-title">{node.title}</span>,
      };
    case 'machine': {
      const machine = node.machine;
      let tooltip = `${machine.name} (${machine.sshHost}): ${machine.statusText}`;
      if (machine.state.kind === 'offline') tooltip += ' — ' + machine.state.reason;
      if (machine.actionError !== null) tooltip += '\n' + machine.actionError;
      return {
        label: tooltip,
        tooltip,
        content: (
          <>
            <StatusDot color={sidebarStatusColor(machine.state)} />
            <Icon name="desktopcomputer" size={13} className="sidebar-row-icon" />
            <span className="sidebar-row-name">{machine.name}</span>
            <span className="sidebar-row-spacer" />
            <span className="sidebar-status-text">{machine.statusText}</span>
            {machine.actionError !== null && (
              <Icon
                name="exclamationmark.triangle.fill"
                size={12}
                className="sidebar-warning"
                label="Error"
              />
            )}
          </>
        ),
      };
    }
    case 'worktree': {
      const worktree = node.worktree;
      return {
        label: worktree.branch,
        content: (
          <>
            <Icon name="arrow.triangle.branch" size={12} className="sidebar-row-icon" />
            <span className="sidebar-row-name">{worktree.branch}</span>
            <span className="sidebar-row-spacer" />
            <AddButton
              label="Add"
              onClick={() => {
                onAdd(node);
              }}
            />
            {worktree.agents.length > 0 && (
              <Badge
                count={worktree.agents.length}
                label={`${String(worktree.agents.length)} agents`}
              />
            )}
          </>
        ),
      };
    }
    case 'workspace': {
      if (workspace === null) return { label: node.workspace.id, content: null };
      const pill = workspace.signal === null ? null : presentSignal(workspace.signal);
      const dot =
        workspace.status === null ? 'tertiary' : workspace.status === 'running' ? 'green' : 'red';
      const paneNote = workspace.paneCount > 1 ? `, ${String(workspace.paneCount)} panes` : '';
      const tabNote =
        workspace.tabCount > workspace.paneCount ? `, ${String(workspace.tabCount)} tabs` : '';
      return {
        label: `${workspace.title}, ${pill?.accessibilityDescription ?? workspace.status ?? 'files'}${paneNote}${tabNote}`,
        content: (
          <>
            <StatusDot color={dot} />
            {renameEditor ?? <span className="sidebar-row-name">{workspace.title}</span>}
            {pill !== null && renameEditor === null && (
              <span className="sidebar-pill" data-attention={pill.needsAttention}>
                {pill.label}
              </span>
            )}
            <span className="sidebar-row-spacer" />
            {workspace.paneCount > 1 && (
              <span className="sidebar-count">
                <Icon name="rectangle.split.2x2" size={11} label="Panes" />
                {workspace.paneCount}
              </span>
            )}
            {workspace.tabCount > workspace.paneCount && (
              <span className="sidebar-count">
                <Icon name="square.stack" size={11} label="Tabs" />
                {workspace.tabCount}
              </span>
            )}
          </>
        ),
      };
    }
  }
}

// MARK: - Rename editor

function RenameField({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  return (
    <input
      className="sidebar-rename"
      aria-label="Rename"
      value={value}
      // Focus and select-all once mounted, as the Mac's field editor does.
      ref={(input) => {
        if (input !== null && document.activeElement !== input && !done.current) {
          input.focus();
          input.select();
        }
      }}
      onChange={(event) => {
        setValue(event.target.value);
      }}
      onMouseDown={(event) => {
        event.stopPropagation();
      }}
      onClick={(event) => {
        event.stopPropagation();
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === 'Enter') {
          event.preventDefault();
          done.current = true;
          onCommit(value);
        } else if (event.key === 'Escape') {
          event.preventDefault();
          done.current = true;
          onCancel();
        }
      }}
      onBlur={() => {
        if (done.current) return;
        done.current = true;
        onCommit(value);
      }}
    />
  );
}

// MARK: - Confirmation (Swift `showConfirmation`, an NSAlert sheet)

export function SidebarConfirmationDialog({
  confirmation,
  onConfirm,
  onCancel,
}: {
  confirmation: SidebarConfirmation;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      className="sidebar-confirm-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="sidebar-confirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="sidebar-confirm-title"
        aria-describedby="sidebar-confirm-message"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onCancel();
          }
        }}
      >
        <h2 id="sidebar-confirm-title" className="sidebar-confirm-title">
          {confirmation.title}
        </h2>
        <p id="sidebar-confirm-message" className="sidebar-confirm-message">
          {confirmation.message}
        </p>
        <div className="sidebar-confirm-buttons">
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          {/* The alert's first button is its default, as NSAlert makes it. */}
          <button
            type="button"
            data-default="true"
            ref={(button) => {
              button?.focus();
            }}
            onClick={onConfirm}
          >
            {confirmation.confirmTitle}
          </button>
        </div>
      </div>
    </div>
  );
}

// MARK: - Controller

interface PendingConfirmation {
  readonly confirmation: SidebarConfirmation;
  readonly action: () => void;
}

interface Pick {
  /** The row the user chose. */
  readonly rowKey: string;
  /** The shell's selection that pick stands for (rows that report none keep the current one). */
  readonly selection: SidebarSelection | null;
}

interface TreeCache {
  readonly renderState: ReturnType<typeof makeSidebarRenderState>;
  readonly nodes: SidebarNode[];
}

const EMPTY_MACHINES: readonly MachineSnapshot[] = [];
const EMPTY_WORKSPACES: WorkspacesByProject = {};

export function SidebarOutlineViewController({
  projects,
  machines = EMPTY_MACHINES,
  workspacesByProject = EMPTY_WORKSPACES,
  activeWorkspaceId = null,
  selection = null,
  expansionStore = useSidebarExpansionStore,
  menuContext,
  presentSignal = provisionalSignalPill,
  ...callbacks
}: SidebarOutlineViewControllerProps) {
  // Rebuild the tree only when what the rows show has changed (Swift `lastRenderState`), so an
  // unchanged refresh keeps every node — and with it the selection and scroll — as it was.
  const renderState = makeSidebarRenderState(
    projects,
    machines,
    workspacesByProject,
    activeWorkspaceId,
  );
  const [tree, setTree] = useState<TreeCache>(() => ({
    renderState,
    nodes: buildSidebarTree(projects, machines, workspacesByProject),
  }));
  let nodes = tree.nodes;
  if (!structurallyEqual(tree.renderState, renderState)) {
    nodes = buildSidebarTree(projects, machines, workspacesByProject);
    setTree({ renderState, nodes });
  }

  const closedRowKeys = useStore(expansionStore, (state) => state.closedRowKeys);
  const rows = visibleRows(nodes, closedRowKeys);

  const [pick, setPick] = useState<Pick | null>(null);
  const [renaming, setRenaming] = useState<{ workspaceId: string; original: string } | null>(null);
  const [pending, setPending] = useState<PendingConfirmation | null>(null);
  const [viewport, setViewport] = useState({ top: 0, height: 0 });
  const scroller = useRef<HTMLDivElement>(null);
  const lastRevealedKey = useRef<string | null>(null);

  // The selected row: the user's pick while it still stands for the shell's selection, otherwise
  // the first row the selection names (Swift `selectNode(for:)`).
  const target = targetId(selection);
  let selectedNode: SidebarNode | null = null;
  if (pick !== null) {
    const picked = findNode(nodes, (node) => node.rowKey === pick.rowKey);
    if (picked !== null && (picked.id === target || selectionsEqual(pick.selection, selection))) {
      selectedNode = picked;
    }
  }
  if (selectedNode === null && target !== null) {
    selectedNode = findNode(nodes, (node) => node.id === target);
  }
  const selectedKey = selectedNode?.rowKey ?? null;
  const selectedIndex =
    selectedKey === null ? null : rows.findIndex((row) => row.node.rowKey === selectedKey);

  // Record the rows that are closed once the tree is settled, dropping rows that are gone
  // (Swift `restoreExpansionState`).
  useEffect(() => {
    if (!isSettled(nodes)) return;
    const store = expansionStore.getState();
    store.record(sidebarExpansion(nodes, store.closedRowKeys).closed);
  }, [nodes, expansionStore]);

  // A row inside a closed row has no row of its own yet, so its ancestors are opened first.
  useEffect(() => {
    if (selectedKey === null) return;
    const store = expansionStore.getState();
    for (const ancestor of sidebarAncestors((node) => node.rowKey === selectedKey, nodes)) {
      if (store.closedRowKeys.has(ancestor.rowKey)) store.setExpanded(ancestor.rowKey, true);
    }
  }, [selectedKey, nodes, expansionStore]);

  // Bring a newly selected row into view.
  useEffect(() => {
    if (selectedKey === lastRevealedKey.current) return;
    if (selectedIndex === null || selectedIndex < 0) return;
    lastRevealedKey.current = selectedKey;
    const element = scroller.current;
    if (element === null) return;
    const top = selectedIndex * SIDEBAR_ROW_HEIGHT;
    const height = element.clientHeight;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (height > 0 && top + SIDEBAR_ROW_HEIGHT > element.scrollTop + height) {
      element.scrollTop = top + SIDEBAR_ROW_HEIGHT - height;
    }
  }, [selectedKey, selectedIndex]);

  // Track the viewport for windowing.
  useEffect(() => {
    const element = scroller.current;
    if (element === null || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      setViewport({ top: element.scrollTop, height: element.clientHeight });
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  // MARK: Actions

  const choose = (node: SidebarNode, additive = false) => {
    if (!isSelectable(node)) return;
    const next = selectionFor(node);
    setPick({ rowKey: node.rowKey, selection: next ?? selection });
    if (next === null) return;
    if (additive) callbacks.onSelectionChanged?.(next, { additive: true });
    else callbacks.onSelectionChanged?.(next);
  };

  const setExpanded = (node: SidebarNode, expanded: boolean) => {
    expansionStore.getState().setExpanded(node.rowKey, expanded);
    // Closing a row that holds the selection selects the row, as NSOutlineView does.
    if (
      !expanded &&
      selectedKey !== null &&
      selectedKey !== node.rowKey &&
      sidebarAncestors((candidate) => candidate.rowKey === selectedKey, nodes).includes(node)
    ) {
      choose(node);
    }
  };

  const confirm = (confirmation: SidebarConfirmation, action: () => void) => {
    setPending({ confirmation, action });
  };

  const showCommandPalette = (node: SidebarNode) => {
    if (node.kind === 'project') {
      callbacks.onShowCommandPalette?.(node.project.projectId, null, true);
    } else if (node.kind === 'worktree') {
      callbacks.onShowCommandPalette?.(
        node.projectId,
        { kind: 'worktree', id: node.worktree.id },
        false,
      );
    }
  };

  /** Delete: the destructive item of the row's menu (Swift `perform(_:on:)`). */
  const deleteRow = (node: SidebarNode) => {
    switch (node.kind) {
      case 'workspace': {
        const found = findWorkspace(workspacesByProject, node.workspace.id);
        if (found === null) return;
        const kill = () => {
          callbacks.onKillWorkspace?.(found.projectId, node.workspace.id);
        };
        const agentCount = workspaceAgentIds(node.workspace).length;
        if (agentCount > 1) confirm(sidebarConfirmation.closeWorkspace(agentCount), kill);
        else kill();
        return;
      }
      case 'worktree':
        confirm(sidebarConfirmation.deleteWorktree(node.worktree), () => {
          callbacks.onDeleteWorktree?.(node.projectId, node.worktree.id);
        });
        return;
      case 'project':
        confirm(sidebarConfirmation.removeProject(node.project, menuContext), () => {
          callbacks.onRemoveProject?.(node.project.projectId);
        });
        return;
      case 'machine':
        confirm(sidebarConfirmation.removeMachine(node.machine), () => {
          callbacks.onRemoveMachine?.(node.machine.id);
        });
        return;
      case 'sectionHeader':
        return;
    }
  };

  const beginRename = (node: SidebarNode) => {
    if (node.kind !== 'workspace') return;
    const project = owningProject(nodes, node);
    const title =
      project === null ? node.workspace.id : workspaceRenderState(node.workspace, project).title;
    setRenaming({ workspaceId: node.workspace.id, original: title });
  };

  const endRename = (value: string | null) => {
    const editing = renaming;
    setRenaming(null);
    scroller.current?.focus();
    if (editing === null || value === null) return;
    const name = value.trim();
    if (name === '' || name === editing.original) return;
    // Renaming a workspace renames the agent it is named after.
    const found = findWorkspace(workspacesByProject, editing.workspaceId);
    const agentId = found === null ? null : primaryAgentId(found.workspace);
    if (found !== null && agentId !== null) {
      callbacks.onRenameAgent?.(found.projectId, agentId, name);
    }
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    // Plain keys only: a modified key belongs to the app's commands.
    if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
    const current = selectedIndex === null || selectedIndex < 0 ? null : selectedIndex;
    const arrow = ARROW_KEYS[event.key];
    if (arrow !== undefined) {
      event.preventDefault();
      const command = sidebarKeyCommand(arrow, rows, current);
      const row = command.kind === 'none' ? undefined : rows[command.index];
      if (row === undefined) return;
      if (command.kind === 'select') choose(row.node);
      else setExpanded(row.node, command.kind === 'expand');
      return;
    }
    const row = current === null ? undefined : rows[current];
    if (row === undefined) return;
    switch (event.key) {
      case 'Enter': {
        event.preventDefault();
        const node = row.node;
        if (node.kind === 'machine') callbacks.onToggleMachineConnection?.(node.machine.id);
        else {
          const next = selectionFor(node);
          if (next !== null) callbacks.onSelectionChanged?.(next);
        }
        return;
      }
      case 'F2':
        event.preventDefault();
        beginRename(row.node);
        return;
      case 'Delete':
        event.preventDefault();
        deleteRow(row.node);
        return;
    }
  };

  // MARK: Windowing

  const viewportHeight = viewport.height > 0 ? viewport.height : FALLBACK_VIEWPORT_HEIGHT;
  const first = Math.max(0, Math.floor(viewport.top / SIDEBAR_ROW_HEIGHT) - OVERSCAN_ROWS);
  const last = Math.min(
    rows.length,
    Math.ceil((viewport.top + viewportHeight) / SIDEBAR_ROW_HEIGHT) + OVERSCAN_ROWS,
  );

  const rendered = rows.slice(first, last).map((row, offset) => {
    const index = first + offset;
    const { node } = row;
    const project = owningProject(nodes, node);
    const workspace =
      node.kind === 'workspace' && project !== null
        ? workspaceRenderState(node.workspace, project)
        : null;
    const cell = cellFor({
      node,
      project,
      workspace,
      presentSignal,
      renameEditor:
        renaming !== null &&
        node.kind === 'workspace' &&
        renaming.workspaceId === node.workspace.id ? (
          <RenameField
            initial={renaming.original}
            onCommit={endRename}
            onCancel={() => {
              endRename(null);
            }}
          />
        ) : null,
      onAdd: showCommandPalette,
    });
    const expandable = isExpandable(node);
    const selected = node.rowKey === selectedKey;
    return (
      <div
        key={node.rowKey}
        id={rowDomId(node.rowKey)}
        className="sidebar-row"
        role="treeitem"
        data-kind={node.kind}
        data-row-key={node.rowKey}
        data-testid={rowTestId(node, project, workspace)}
        data-agent-id={
          node.kind === 'workspace' ? (primaryAgentId(node.workspace) ?? undefined) : undefined
        }
        data-unavailable={cell.unavailable === true ? 'true' : undefined}
        aria-level={row.depth + 1}
        aria-selected={isSelectable(node) ? selected : undefined}
        aria-expanded={expandable ? row.isExpanded : undefined}
        aria-label={cell.label}
        title={cell.tooltip}
        style={{
          top: index * SIDEBAR_ROW_HEIGHT,
          height: SIDEBAR_ROW_HEIGHT,
          paddingLeft: 4 + row.depth * INDENTATION_PER_LEVEL,
        }}
        onMouseDown={(event) => {
          if (event.button !== 0) return;
          choose(node, event.ctrlKey);
        }}
      >
        {expandable ? (
          <button
            type="button"
            className="sidebar-disclosure"
            tabIndex={-1}
            aria-label={row.isExpanded ? 'Collapse' : 'Expand'}
            onMouseDown={(event) => {
              event.stopPropagation();
              event.preventDefault();
            }}
            onClick={(event) => {
              event.stopPropagation();
              setExpanded(node, !row.isExpanded);
            }}
          >
            <Icon name={row.isExpanded ? 'chevron.down' : 'chevron.right'} size={11} />
          </button>
        ) : (
          <span className="sidebar-disclosure-spacer" />
        )}
        {cell.content}
      </div>
    );
  });

  return (
    <>
      <div
        ref={scroller}
        className="sidebar-outline"
        role="tree"
        aria-label="Projects"
        tabIndex={0}
        aria-activedescendant={
          selectedKey !== null && selectedIndex !== null && selectedIndex >= 0
            ? rowDomId(selectedKey)
            : undefined
        }
        data-row-count={rows.length}
        onScroll={(event) => {
          const element = event.currentTarget;
          setViewport({ top: element.scrollTop, height: element.clientHeight });
        }}
        onKeyDown={onKeyDown}
      >
        <div
          className="sidebar-outline-canvas"
          style={{ height: rows.length * SIDEBAR_ROW_HEIGHT }}
        >
          {rendered}
        </div>
      </div>
      {pending !== null && (
        <SidebarConfirmationDialog
          confirmation={pending.confirmation}
          onConfirm={() => {
            setPending(null);
            scroller.current?.focus();
            pending.action();
          }}
          onCancel={() => {
            setPending(null);
            scroller.current?.focus();
          }}
        />
      )}
    </>
  );
}
