// Port of apps/purepoint-macos/purepoint-macos/State/TriggersState.swift (WIN-8.9).
//
// The Triggers tab of the Agents hub: triggers merged across the library's project roots, the
// selection and the creation sheet, plus `assign_trigger` (the Mac assigns from the CLI only).
//
// Shape (DEC-14): published values live in a Zustand store (`triggers.store`). As in
// AgentsHubState, a daemon `error` reply sets `error` (Swift drops it silently).

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Request, Response, TriggerActionPayload } from '../protocol';
import { triggerItemFromInfo, type TriggerItem } from '../model/triggerItem';
import { localDaemonClient, type DaemonRequester } from '../services/daemonConnection';

export interface TriggersStateFields {
  readonly triggers: readonly TriggerItem[];
  readonly selectedTriggerId: string | null;
  readonly showingCreationSheet: boolean;
  readonly isLoading: boolean;
  readonly error: string | null;
  /** The last assignment that went through, for the detail view to confirm. */
  readonly lastAssignment: {
    readonly agentId: string;
    readonly triggerName: string;
    readonly sequenceLength: number;
  } | null;
}

const INITIAL: TriggersStateFields = {
  triggers: [],
  selectedTriggerId: null,
  showingCreationSheet: false,
  isLoading: false,
  error: null,
  lastAssignment: null,
};

export const selectedTriggerOf = (s: TriggersStateFields): TriggerItem | null =>
  s.selectedTriggerId === null
    ? null
    : (s.triggers.find((t) => t.id === s.selectedTriggerId) ?? null);

class TriggerRequestError extends Error {}

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = error.message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}

export class TriggersState {
  readonly store: StoreApi<TriggersStateFields>;
  private readonly client: DaemonRequester;

  constructor(client: DaemonRequester = localDaemonClient) {
    this.client = client;
    this.store = createStore<TriggersStateFields>()(() => INITIAL);
  }

  private get s(): TriggersStateFields {
    return this.store.getState();
  }

  get triggers(): readonly TriggerItem[] {
    return this.s.triggers;
  }
  get selectedTriggerId(): string | null {
    return this.s.selectedTriggerId;
  }
  set selectedTriggerId(value: string | null) {
    this.store.setState({ selectedTriggerId: value });
  }
  get showingCreationSheet(): boolean {
    return this.s.showingCreationSheet;
  }
  set showingCreationSheet(value: boolean) {
    this.store.setState({ showingCreationSheet: value });
  }
  get isLoading(): boolean {
    return this.s.isLoading;
  }
  get error(): string | null {
    return this.s.error;
  }

  get selectedTrigger(): TriggerItem | null {
    return selectedTriggerOf(this.s);
  }

  // Event handlers call these (React's rules keep views from assigning to a hook value).
  select(id: string | null): void {
    this.selectedTriggerId = id;
  }
  setShowingCreationSheet(shown: boolean): void {
    this.showingCreationSheet = shown;
  }
  dismissError(): void {
    this.store.setState({ error: null });
  }

  private async send(request: Request): Promise<Response> {
    const response = await this.client.send(request);
    if (response.type === 'error') throw new TriggerRequestError(response.message);
    return response;
  }

  // MARK: - Backend Integration

  async loadTriggers(projectRoots: readonly string[]): Promise<void> {
    this.store.setState({ isLoading: true, error: null });
    try {
      const merged = new Map<string, TriggerItem>();
      for (const root of projectRoots) {
        const response = await this.send({ type: 'list_triggers', project_root: root });
        if (response.type !== 'trigger_list') continue;
        for (const info of response.triggers) {
          const item = triggerItemFromInfo(info, root);
          if (!merged.has(item.id)) merged.set(item.id, item);
        }
      }
      const triggers = [...merged.values()].sort((a, b) =>
        a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
      );
      this.store.setState({ triggers });
    } catch (error) {
      this.store.setState({ error: `Failed to load triggers: ${describeError(error)}` });
    }
    this.store.setState({ isLoading: false });
  }

  async saveTrigger(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    description: string | null;
    on: string;
    sequence: readonly TriggerActionPayload[];
    variables: Readonly<Record<string, string>>;
    scope: string;
  }): Promise<void> {
    try {
      await this.send({
        type: 'save_trigger',
        project_root: options.projectRoot,
        name: options.name,
        description: options.description,
        on: options.on,
        sequence: [...options.sequence],
        variables: { ...options.variables },
        scope: options.scope,
      });
      await this.loadTriggers(options.projectRoots);
    } catch (error) {
      this.store.setState({ error: `Failed to save trigger: ${describeError(error)}` });
    }
  }

  async deleteTrigger(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    scope: string;
  }): Promise<void> {
    try {
      await this.send({
        type: 'delete_trigger',
        project_root: options.projectRoot,
        name: options.name,
        scope: options.scope,
      });
      if (this.selectedTriggerId === `${options.scope}:${options.name}`) {
        this.selectedTriggerId = null;
      }
      await this.loadTriggers(options.projectRoots);
    } catch (error) {
      this.store.setState({ error: `Failed to delete trigger: ${describeError(error)}` });
    }
  }

  /** Give a live agent this trigger (`assign_trigger`); true when the daemon took it. */
  async assignTrigger(options: {
    projectRoot: string;
    agentId: string;
    triggerName: string;
  }): Promise<boolean> {
    try {
      const response = await this.send({
        type: 'assign_trigger',
        project_root: options.projectRoot,
        agent_id: options.agentId,
        trigger_name: options.triggerName,
      });
      if (response.type !== 'assign_trigger_result') return false;
      this.store.setState({
        lastAssignment: {
          agentId: response.agent_id,
          triggerName: response.trigger_name,
          sequenceLength: response.sequence_len,
        },
      });
      return true;
    } catch (error) {
      this.store.setState({ error: `Failed to assign trigger: ${describeError(error)}` });
      return false;
    }
  }
}

const states = new WeakMap<object, TriggersState>();

/** The triggers state of one app window (Swift `appState.triggersState`), made on first use. */
export function triggersStateFor(owner: object): TriggersState {
  let state = states.get(owner);
  if (state === undefined) {
    state = new TriggersState();
    states.set(owner, state);
  }
  return state;
}
