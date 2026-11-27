// Port of apps/purepoint-macos/purepoint-macos/State/PointGuardServiceModel.swift (WIN-9.3).
//
// The setup side of Point Guard: providers and sign-in, model choice, the working folder,
// phone pairing and paired devices, all as owner admin requests to the app-owned bridge.
// The process side (launch, readiness, stop, the Quit wait) lives in the backend
// (`src-tauri/src/pointguard/service.rs`); this state mirrors its `pointguard-state`
// snapshots and resets everything tied to a run when the owned runtime exits, as the
// Swift `ownedRuntimeExited` does.
//
// Shape (DEC-14): published values live in a Zustand store (`pointGuard.store`). The Swift
// `chat` binding becomes `onConnection` / `onDisconnect` hooks for the chat (WIN-9.4).
// Windows wording: "this PC" where the Mac says "this Mac".

import { createStore, type StoreApi } from 'zustand/vanilla';
import {
  pointGuardBackend,
  type JsonObject,
  type JsonValue,
  type PointGuardBackend,
  type PointGuardChatConnection,
  type PointGuardLayout,
  type PointGuardSnapshot,
} from '../services/pointguard';

export interface PointGuardServiceFields {
  readonly phase: string;
  readonly error: string | null;
  readonly ready: boolean;
  readonly running: boolean;
  readonly layout: PointGuardLayout | null;
  readonly providers: readonly JsonValue[];
  readonly auth: JsonValue;
  readonly enrollment: JsonValue;
  readonly devices: readonly JsonValue[];
  /** Devices with an access change or revoke in flight; their row stays disabled until it lands. */
  readonly deviceUpdates: ReadonlySet<string>;
  readonly restartRequired: boolean;
  readonly hasPendingAuthCleanup: boolean;
  readonly hasPendingEnrollmentCleanup: boolean;
  readonly cwd: string;
  /** The owned Pi process's folder; a saved configuration change applies only after restart. */
  readonly activeCwd: string;
  readonly selectedProvider: string;
  readonly selectedModel: string;
  readonly enrollmentStatus: string;
  readonly remoteRecovery: string | null;
  readonly remoteEndpoint: string | null;
}

const INITIAL: PointGuardServiceFields = {
  phase: 'Stopped',
  error: null,
  ready: false,
  running: false,
  layout: null,
  providers: [],
  auth: null,
  enrollment: null,
  devices: [],
  deviceUpdates: new Set(),
  restartRequired: false,
  hasPendingAuthCleanup: false,
  hasPendingEnrollmentCleanup: false,
  cwd: '',
  activeCwd: '',
  selectedProvider: '',
  selectedModel: '',
  enrollmentStatus: '',
  remoteRecovery: null,
  remoteEndpoint: null,
};

// MARK: - JSON access (the Swift PiJSONValue subscripts)

/** `value[key]`, or null when `value` is not an object or has no such key. */
export function field(value: JsonValue | undefined, key: string): JsonValue {
  if (value !== null && value !== undefined && typeof value === 'object' && !Array.isArray(value))
    return value[key] ?? null;
  return null;
}

/** The string, or null. */
export function text(value: JsonValue | undefined): string | null {
  return typeof value === 'string' ? value : null;
}

/** The array's elements, or none. */
export function values(value: JsonValue | undefined): JsonValue[] {
  return Array.isArray(value) ? value : [];
}

function describeError(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

// MARK: - Phone access (the Swift PointGuardPhoneAccess)

/** The device grants as one choice. New devices pair with full access. */
export type PhoneAccessLevel = 'full' | 'terminals' | 'view' | 'off';
export const PHONE_ACCESS_LEVELS: readonly PhoneAccessLevel[] = [
  'full',
  'terminals',
  'view',
  'off',
];

export function phoneAccessLevel(
  terminals: string,
  workspaceManagement: boolean,
): PhoneAccessLevel {
  switch (terminals) {
    case 'control':
      return workspaceManagement ? 'full' : 'terminals';
    case 'view':
      return 'view';
    default:
      return 'off';
  }
}

export function levelTerminals(level: PhoneAccessLevel): string {
  return level === 'full' || level === 'terminals' ? 'control' : level;
}

export function levelWorkspaceManagement(level: PhoneAccessLevel): boolean {
  return level === 'full';
}

export function levelTitle(level: PhoneAccessLevel): string {
  return {
    full: 'Full access',
    terminals: 'Terminals only',
    view: 'View only',
    off: 'No access',
  }[level];
}

export type PhoneAccessStep =
  { kind: 'workspaceManagement'; allowed: boolean } | { kind: 'terminals'; terminals: string };

/**
 * The grant writes that move a device's stored grants to `level`. Works from the raw stored
 * values, so a legacy management grant hidden under View or Off is still cleared. Workspace
 * management requires Control, so it is dropped before terminals change and added after.
 */
export function phoneAccessSteps(
  terminals: string,
  workspaceManagement: boolean,
  level: PhoneAccessLevel,
): PhoneAccessStep[] {
  const steps: PhoneAccessStep[] = [];
  const wanted = levelWorkspaceManagement(level);
  if (workspaceManagement && !wanted) steps.push({ kind: 'workspaceManagement', allowed: false });
  if (terminals !== levelTerminals(level))
    steps.push({ kind: 'terminals', terminals: levelTerminals(level) });
  if (!workspaceManagement && wanted) steps.push({ kind: 'workspaceManagement', allowed: true });
  return steps;
}

export function phoneAccessSummary(terminals: string, workspaceManagement: boolean): string {
  if (terminals === 'control' && workspaceManagement)
    return 'Full control: agents, terminals, workspaces and chatting with Point Guard, which can run commands and manage agents on this PC.';
  if (terminals === 'control')
    return 'Controls agent terminals and reads Point Guard chat. Sending to Point Guard also needs Workspace management.';
  if (terminals === 'view')
    return 'Views agents and terminals and reads Point Guard chat. Cannot type, manage workspaces or send to Point Guard.';
  return 'No access to agents, terminals or Point Guard chat.';
}

// MARK: - The state

export interface PointGuardServiceOptions {
  backend?: PointGuardBackend;
  /** Replaces admin requests (the Swift `requestOverride`, for previews and tests). */
  requestOverride?: (operation: string, fields: JsonObject) => Promise<JsonValue>;
  remoteEndpoint?: string | null;
  /** The sign-in status poll interval. */
  sleep?: (ms: number) => Promise<void>;
  /** The chat binding: called with the local chat endpoint once ready. */
  onConnection?: (connection: PointGuardChatConnection) => void;
  /** The chat binding: called when the owned runtime is gone. */
  onDisconnect?: () => void;
}

const defaultSleep = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

/** Owns only the bridge this app launched; descriptors never authorize process adoption. */
export class PointGuardServiceState {
  readonly store: StoreApi<PointGuardServiceFields>;
  private readonly backend: PointGuardBackend;
  private readonly requestOverride: PointGuardServiceOptions['requestOverride'];
  private readonly sleep: (ms: number) => Promise<void>;
  private readonly onConnection: PointGuardServiceOptions['onConnection'];
  private readonly onDisconnect: PointGuardServiceOptions['onDisconnect'];
  private runtimeGeneration = 0;
  private authGeneration = 0;
  private enrollmentGeneration = 0;
  private activeAttemptId: string | null = null;
  private readonly pendingAuthCleanup = new Set<string>();
  private readonly pendingEnrollmentCleanup = new Set<string>();
  private launchCwd: string | null = null;
  private snapshot: PointGuardSnapshot | null = null;
  private unlisten: (() => void) | null = null;

  constructor(options: PointGuardServiceOptions = {}) {
    this.backend = options.backend ?? pointGuardBackend;
    this.requestOverride = options.requestOverride;
    this.sleep = options.sleep ?? defaultSleep;
    this.onConnection = options.onConnection;
    this.onDisconnect = options.onDisconnect;
    this.store = createStore<PointGuardServiceFields>()(() => ({
      ...INITIAL,
      remoteEndpoint: options.remoteEndpoint ?? null,
    }));
  }

  private get s(): PointGuardServiceFields {
    return this.store.getState();
  }
  private set(patch: Partial<PointGuardServiceFields>): void {
    this.store.setState(patch);
  }

  get phase(): string {
    return this.s.phase;
  }
  get error(): string | null {
    return this.s.error;
  }
  set error(value: string | null) {
    this.set({ error: value });
  }
  get ready(): boolean {
    return this.s.ready;
  }

  /** Follow the backend's snapshots; resolves once the current one is applied. */
  async attach(): Promise<void> {
    if (!this.unlisten) {
      this.unlisten = await this.backend.onState((snapshot) => {
        this.apply(snapshot);
      });
    }
    this.apply(await this.backend.status());
  }

  detach(): void {
    this.unlisten?.();
    this.unlisten = null;
  }

  /** Mirror one backend snapshot; a new or ended run resets the run's state. */
  apply(snapshot: PointGuardSnapshot): void {
    const previous = this.snapshot;
    this.snapshot = snapshot;
    const ended =
      previous !== null &&
      previous.instanceId !== null &&
      previous.instanceId !== snapshot.instanceId;
    if (ended || (previous?.ready === true && !snapshot.ready)) this.ownedRuntimeExited();
    this.set({
      phase: snapshot.phase,
      ready: snapshot.ready,
      running: snapshot.running,
      layout: snapshot.layout,
      ...(snapshot.error !== (previous?.error ?? null) ? { error: snapshot.error } : {}),
      ...(snapshot.ready ? { remoteEndpoint: snapshot.remoteEndpoint } : {}),
    });
    if (snapshot.ready && previous?.ready !== true) void this.becameReady();
  }

  private async becameReady(): Promise<void> {
    const generation = this.runtimeGeneration;
    this.launchCwd = null;
    if (this.onConnection) {
      try {
        const connection = await this.backend.chatConnection();
        if (generation === this.runtimeGeneration) this.onConnection(connection);
      } catch (error) {
        this.error = describeError(error);
      }
    }
    await this.refresh();
  }

  /** Start (or Retry) the owned runtime; a folder chosen while stopped applies to this launch. */
  async start(): Promise<void> {
    try {
      await this.backend.start(this.launchCwd ?? undefined);
    } catch (error) {
      this.error = describeError(error);
    }
  }

  async request(operation: string, fields: JsonObject = {}): Promise<JsonValue> {
    if (this.requestOverride) return this.requestOverride(operation, fields);
    if (!this.s.ready) throw new Error('Start Point Guard before changing setup.');
    try {
      return await this.backend.request(operation, fields);
    } catch (error) {
      throw new Error(describeError(error), { cause: error });
    }
  }

  async refresh(): Promise<void> {
    const generation = this.runtimeGeneration;
    try {
      const status = await this.request('status');
      if (generation !== this.runtimeGeneration) return;
      const activeCwd = text(field(status, 'cwd')) ?? '';
      const recovery = field(status, 'remoteRecovery');
      const remoteRecovery = [text(field(recovery, 'message')), text(field(recovery, 'recovery'))]
        .filter((part): part is string => part !== null)
        .join(' ');
      this.set({
        activeCwd,
        cwd: activeCwd,
        remoteRecovery: remoteRecovery === '' ? null : remoteRecovery,
        selectedProvider: text(field(status, 'provider')) ?? '',
        selectedModel: text(field(status, 'model')) ?? '',
      });
      const providers = values(field(await this.request('providers'), 'providers'));
      this.set({ providers });
      const devices = values(field(await this.request('devices.list'), 'devices'));
      this.set({ devices });
    } catch (error) {
      this.error = describeError(error);
    }
  }

  get authMessage(): string | null {
    const auth = this.s.auth;
    const name = text(field(auth, 'providerName')) ?? 'your provider';
    const status = text(field(auth, 'status'));
    switch (status) {
      case 'pending':
        return `Signing in to ${name}… Complete the provider’s instructions below.`;
      case 'complete':
        return text(field(auth, 'type')) === 'oauth'
          ? `Signed in to ${name}.`
          : `Credentials saved for ${name}.`;
      case 'canceled':
      case 'cancelled':
        return 'Sign-in canceled. Try again when you’re ready.';
      case 'expired':
        return 'Sign-in expired. Start a new sign-in to try again.';
      case 'failed':
        return 'Sign-in failed. Start a new sign-in to try again.';
      case null:
        return null;
      default:
        return 'Sign-in could not be confirmed. Start a new sign-in to try again.';
    }
  }

  async login(provider: string, type: string): Promise<void> {
    const generation = ++this.authGeneration;
    const lifetime = this.runtimeGeneration;
    this.error = null;
    try {
      if (this.activeAttemptId !== null) this.pendingAuthCleanup.add(this.activeAttemptId);
      if (!(await this.retryAuthCleanup()) || generation !== this.authGeneration) return;
      this.activeAttemptId = null;
      const match = this.s.providers.find((p) => text(field(p, 'id')) === provider);
      const name = text(field(match, 'name')) ?? provider;
      const identity: JsonObject = { provider, providerName: name, type };
      this.set({ auth: { ...identity, status: 'pending' } });
      const result = await this.request('auth.start', { provider, type });
      if (lifetime !== this.runtimeGeneration) return;
      const id = text(field(result, 'attemptId'));
      if (id === null) throw new Error('Provider login did not start.');
      if (generation !== this.authGeneration) {
        this.pendingAuthCleanup.add(id);
        await this.retryAuthCleanup();
        return;
      }
      this.activeAttemptId = id;
      void this.pollAuth(id, identity, generation);
    } catch (error) {
      if (generation === this.authGeneration) {
        this.error = describeError(error);
        if (this.activeAttemptId === null) this.set({ auth: null });
      }
    }
  }

  private async pollAuth(id: string, identity: JsonObject, generation: number): Promise<void> {
    try {
      while (generation === this.authGeneration) {
        const status = await this.request('auth.status', { attemptId: id });
        if (generation !== this.authGeneration) return;
        const display: JsonObject = { ...identity };
        for (const key of ['status', 'attemptId', 'error']) display[key] = field(status, key);
        const state = text(field(status, 'status'));
        if (state === 'pending') {
          display.events = field(status, 'events');
          display.prompt = field(status, 'prompt');
        }
        this.set({ auth: display });
        if (state !== 'pending') {
          this.activeAttemptId = null;
          if (state === 'complete') {
            this.set({ restartRequired: true });
            await this.refresh();
          }
          return;
        }
        await this.sleep(500);
      }
    } catch (error) {
      if (generation === this.authGeneration) this.error = describeError(error);
    }
  }

  async respond(value: string): Promise<void> {
    const auth = this.s.auth;
    const id = text(field(auth, 'attemptId'));
    const promptId = text(field(field(auth, 'prompt'), 'id'));
    if (id === null || promptId === null) return;
    try {
      await this.request('auth.respond', { attemptId: id, promptId, value });
    } catch (error) {
      this.error = describeError(error);
    }
  }

  /** Cleanup IDs remain owned until an explicit successful server receipt. */
  async retryAuthCleanup(): Promise<boolean> {
    const lifetime = this.runtimeGeneration;
    for (const id of [...this.pendingAuthCleanup].sort()) {
      try {
        await this.request('auth.cancel', { attemptId: id });
        if (lifetime !== this.runtimeGeneration) return false;
        this.pendingAuthCleanup.delete(id);
      } catch {
        if (lifetime !== this.runtimeGeneration) return false;
        this.set({
          hasPendingAuthCleanup: true,
          error:
            'Login cancellation could not be confirmed. Retry pending cancellations when Point Guard is reachable.',
        });
        return false;
      }
    }
    const pending = this.pendingAuthCleanup.size > 0;
    this.set({ hasPendingAuthCleanup: pending });
    return !pending;
  }

  async cancelLogin(): Promise<void> {
    const auth = this.s.auth;
    if (
      this.activeAttemptId === null &&
      text(field(auth, 'status')) !== 'pending' &&
      !this.s.hasPendingAuthCleanup
    )
      return;
    const generation = ++this.authGeneration;
    if (this.activeAttemptId !== null) this.pendingAuthCleanup.add(this.activeAttemptId);
    if (!(await this.retryAuthCleanup()) || generation !== this.authGeneration) return;
    this.activeAttemptId = null;
    this.set({
      auth: {
        status: 'canceled',
        provider: field(auth, 'provider'),
        providerName: field(auth, 'providerName'),
        type: field(auth, 'type'),
      },
    });
  }

  async selectModel(provider: string, model: string): Promise<void> {
    try {
      await this.request('model.select', { provider, model });
      await this.refresh();
    } catch (error) {
      this.error = describeError(error);
    }
  }

  async configure(cwd: string): Promise<void> {
    try {
      const folder = await this.backend.validateFolder(cwd);
      if (this.s.ready) {
        await this.request('runtime.configure', { cwd: folder });
        this.launchCwd = null;
        this.set({ restartRequired: true });
      } else {
        // Explicit recovery override is passed to the next owned launch;
        // the runtime still validates schema/lock/private state before saving.
        this.launchCwd = folder;
        this.set({ phase: 'Folder selected · Start / Retry' });
      }
      this.set({ cwd: folder });
    } catch (error) {
      this.error = describeError(error);
    }
  }

  async connectPhone(): Promise<void> {
    const generation = ++this.enrollmentGeneration;
    const lifetime = this.runtimeGeneration;
    const remoteEndpoint = this.s.remoteEndpoint;
    if (remoteEndpoint === null) {
      this.error =
        'Connect Tailscale on this PC, then restart Point Guard to enable phone pairing.';
      return;
    }
    const oldId = text(field(this.s.enrollment, 'enrollmentId'));
    this.set({ enrollment: null, enrollmentStatus: '' });
    if (oldId !== null) this.pendingEnrollmentCleanup.add(oldId);
    if (!(await this.retryEnrollmentCleanup()) || generation !== this.enrollmentGeneration) return;
    try {
      const result = await this.request('pairing.create', { endpoint: remoteEndpoint });
      if (lifetime !== this.runtimeGeneration) return;
      if (generation !== this.enrollmentGeneration) {
        const id = text(field(result, 'enrollmentId'));
        if (id !== null) this.pendingEnrollmentCleanup.add(id);
        await this.retryEnrollmentCleanup();
        return;
      }
      this.set({ enrollment: result, enrollmentStatus: 'pending' });
    } catch (error) {
      if (generation === this.enrollmentGeneration) this.error = describeError(error);
    }
  }

  /**
   * The Connect phone sheet's status poll: refresh the pending pairing code every `intervalMs`
   * until it is used, closed or replaced, `cancelled()` says so, or the code expires (`expiresAt`,
   * ms since the epoch) - an expired code can never be consumed, so asking is pointless.
   */
  async pollEnrollment(
    cancelled: () => boolean,
    intervalMs = 1000,
    now: () => number = Date.now,
  ): Promise<void> {
    const live = () => {
      if (cancelled() || this.s.enrollmentStatus !== 'pending') return false;
      const expiresAt = field(this.s.enrollment, 'expiresAt');
      return typeof expiresAt === 'number' && now() < expiresAt;
    };
    while (live()) {
      await this.refreshEnrollment();
      if (!live()) return;
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }

  async refreshEnrollment(): Promise<void> {
    const id = text(field(this.s.enrollment, 'enrollmentId'));
    if (id === null) return;
    try {
      const status = await this.request('pairing.status', { enrollmentId: id });
      if (text(field(this.s.enrollment, 'enrollmentId')) !== id) return;
      const enrollmentStatus = text(field(status, 'status')) ?? 'unknown';
      this.set({ enrollmentStatus });
      if (enrollmentStatus === 'consumed') await this.refresh();
    } catch (error) {
      this.error = describeError(error);
    }
  }

  async retryEnrollmentCleanup(): Promise<boolean> {
    const lifetime = this.runtimeGeneration;
    for (const id of [...this.pendingEnrollmentCleanup].sort()) {
      try {
        await this.request('pairing.revoke', { enrollmentId: id });
        if (lifetime !== this.runtimeGeneration) return false;
        this.pendingEnrollmentCleanup.delete(id);
      } catch {
        if (lifetime !== this.runtimeGeneration) return false;
        this.set({
          hasPendingEnrollmentCleanup: true,
          error:
            'Pairing-code revocation could not be confirmed. The code may remain valid until expiry. Retry pending revocations when Point Guard is reachable.',
        });
        return false;
      }
    }
    const pending = this.pendingEnrollmentCleanup.size > 0;
    this.set({ hasPendingEnrollmentCleanup: pending });
    return !pending;
  }

  async closeEnrollment(): Promise<void> {
    this.enrollmentGeneration++;
    const id = text(field(this.s.enrollment, 'enrollmentId'));
    if (id !== null) this.pendingEnrollmentCleanup.add(id);
    this.set({ enrollment: null, enrollmentStatus: '' });
    await this.retryEnrollmentCleanup();
  }

  private markUpdating(id: string, updating: boolean): void {
    const next = new Set(this.s.deviceUpdates);
    if (updating) next.add(id);
    else next.delete(id);
    this.set({ deviceUpdates: next });
  }

  /** Moves a device to `level` from its stored grants. One change per device at a time. */
  async setAccess(id: string, level: PhoneAccessLevel): Promise<void> {
    const device = this.s.devices.find((d) => text(field(d, 'deviceId')) === id);
    if (this.s.deviceUpdates.has(id) || device === undefined) return;
    this.markUpdating(id, true);
    try {
      const stored = field(device, 'workspaceManagement') === true;
      const terminals = text(field(device, 'terminals')) ?? 'off';
      for (const step of phoneAccessSteps(terminals, stored, level)) {
        if (step.kind === 'workspaceManagement')
          await this.request('devices.workspaceManagement', {
            deviceId: id,
            workspaceManagement: step.allowed,
          });
        else await this.request('devices.terminals', { deviceId: id, terminals: step.terminals });
      }
    } catch (error) {
      this.error = describeError(error);
    }
    await this.refresh();
    this.markUpdating(id, false);
  }

  async revokeDevice(id: string): Promise<void> {
    if (this.s.deviceUpdates.has(id)) return;
    this.markUpdating(id, true);
    try {
      await this.request('devices.revoke', { deviceId: id });
      await this.refresh();
    } catch (error) {
      this.error = describeError(error);
    }
    this.markUpdating(id, false);
  }

  /** Restart to apply saved changes; refused while the chat is busy (the Swift `chat.busy`). */
  async restart(chatBusy = false): Promise<void> {
    if (chatBusy) {
      this.error = 'Wait for Point Guard to finish or Stop this run before restarting.';
      return;
    }
    try {
      await this.backend.restart(this.launchCwd ?? undefined);
      this.set({ restartRequired: false });
    } catch (error) {
      this.error = describeError(error);
    }
  }

  async stop(): Promise<void> {
    try {
      await this.backend.stop();
    } catch (error) {
      this.error = describeError(error);
    }
  }

  /** Only the exact owned child exit invalidates its ephemeral provider/enrollment IDs. */
  private ownedRuntimeExited(): void {
    this.runtimeGeneration++;
    this.authGeneration++;
    this.enrollmentGeneration++;
    this.activeAttemptId = null;
    this.pendingAuthCleanup.clear();
    this.pendingEnrollmentCleanup.clear();
    this.set({
      activeCwd: '',
      remoteRecovery: null,
      auth: null,
      enrollment: null,
      enrollmentStatus: '',
      hasPendingAuthCleanup: false,
      hasPendingEnrollmentCleanup: false,
    });
    this.onDisconnect?.();
  }
}
