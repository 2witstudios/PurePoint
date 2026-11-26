// Port of apps/purepoint-macos/purepoint-macos/Services/MachineConnection.swift (WIN-8.14): the
// front end of `src-tauri/src/daemon/machine.rs`.
//
// The Mac's `MachineConnection` keeps an `ssh -L` forward up and probes the daemon through it.
// Windows has no forward (WINAPP-013): the backend reaches a machine with one `ssh.exe … pu daemon
// proxy` per connection, so `MachineConnection` here only supervises — it probes the daemon with a
// relayed `health` (never starting it, MACH-003) and reports the machine's state.
//
// Differences from Swift:
// - No forward, `forward.lock`, pid file or stale-forward sweep (REQ-WINAPP-055). Several app
//   instances each probe on their own.
// - A probe's relay exit status is definitive where the Mac has to infer: exit 69 reports the
//   daemon stopped at once, and ssh failing (or no `pu` on the machine) reports offline with the
//   reason. Other failures still count toward `HealthProbeTracker`'s verdict.
// - Offline retries back off from 2 s to 60 s, as the Mac's forward does.

import { invoke } from '@tauri-apps/api/core';
import type { Request, Response } from '../protocol';
import type { DaemonAvailability, MachineConnectionState } from '../model/sidebarItem';
import {
  type HealthProbe,
  HealthProbeTracker,
  type Machine,
  connectionStatesEqual,
} from '../model/machine';
import type { DaemonRequester } from './daemonConnection';
import { onMachinesChanged, unwatch, watchMachines } from './watchers';

// MARK: - Backend commands

interface WireMachine {
  readonly id: string;
  readonly name: string;
  readonly ssh_host: string;
}

function fromWire(machine: WireMachine): Machine {
  return { id: machine.id.toLowerCase(), name: machine.name, sshHost: machine.ssh_host };
}

/** The registered machines (`machines_load`). */
export async function loadMachineRegistry(): Promise<Machine[]> {
  return (await invoke<WireMachine[]>('machines_load')).map(fromWire);
}

/** Register a machine under the registry lock (`machines_add`); rejects with its message. */
export async function addMachineToRegistry(
  name: string,
  sshHost: string,
  id?: string,
): Promise<Machine> {
  return fromWire(await invoke<WireMachine>('machines_add', { name, sshHost, id: id ?? null }));
}

/** Forget a machine (`machines_remove`); its daemon is untouched. */
export function removeMachineFromRegistry(id: string): Promise<boolean> {
  return invoke<boolean>('machines_remove', { id });
}

/** `%USERPROFILE%\.ssh\config`, empty when there is none. */
export function readSshConfig(): Promise<string> {
  return invoke<string>('machines_ssh_config');
}

/** Add Machine's Test Connection: resolves what it found, rejects with ssh's reason. */
export function testMachineConnection(sshHost: string): Promise<string> {
  return invoke<string>('machine_test_connection', { sshHost });
}

/** A machine's daemon, through the backend's relay (`machine_request`). */
export function machineDaemonClient(machineId: string): DaemonRequester {
  return {
    send: (request: Request) => invoke<Response>('machine_request', { machineId, json: request }),
  };
}

/** What `MachineConnection` asks of the backend. */
export interface MachineBackend {
  probe(machineId: string): Promise<HealthProbe>;
  /** The error Start daemon met, or null. */
  startDaemon(machineId: string): Promise<string | null>;
}

export const tauriMachineBackend: MachineBackend = {
  probe: (machineId) => invoke<HealthProbe>('machine_probe', { machineId }),
  startDaemon: (machineId) => invoke<string | null>('machine_start_daemon', { machineId }),
};

/** Watch `machines.json`; resolves the function that stops watching. */
export async function watchMachineRegistry(onChange: () => void): Promise<() => void> {
  const stopListening = await onMachinesChanged(onChange);
  let id: Awaited<ReturnType<typeof watchMachines>> | null = null;
  try {
    id = await watchMachines();
  } catch (error) {
    console.warn('PurePoint: could not watch the machine registry', error);
  }
  return () => {
    stopListening();
    if (id !== null) void unwatch(id).catch(() => false);
  };
}

// MARK: - MachineConnection

export interface MachineConnectionOptions {
  readonly backend?: MachineBackend;
  /** Resolves after `ms`, or early when `signal` aborts. */
  readonly sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
}

/** Probe intervals (Swift `MachineConnection.supervise`). */
export const PROBE_INTERVAL_MS = 10_000;
export const INCONCLUSIVE_PROBE_INTERVAL_MS = 2_000;
export const OFFLINE_BACKOFF_INITIAL_MS = 2_000;
export const OFFLINE_BACKOFF_MAX_MS = 60_000;

function abortableSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(done, ms);
    function done() {
      clearTimeout(timer);
      signal.removeEventListener('abort', done);
      resolve();
    }
    signal.addEventListener('abort', done);
  });
}

function reason(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

/**
 * One machine's connection: probes its daemon while connected and reports every state change.
 * The remote daemon and its agents keep running whatever happens here (MACH-004).
 */
export class MachineConnection {
  readonly machine: Machine;
  private readonly onState: (state: MachineConnectionState) => void;
  private readonly backend: MachineBackend;
  private readonly sleep: (ms: number, signal: AbortSignal) => Promise<void>;
  private generation = 0;
  private running = false;
  private lastReported: MachineConnectionState = { kind: 'disconnected' };
  private wake: AbortController | null = null;

  constructor(
    machine: Machine,
    onState: (state: MachineConnectionState) => void,
    options: MachineConnectionOptions = {},
  ) {
    this.machine = machine;
    this.onState = onState;
    this.backend = options.backend ?? tauriMachineBackend;
    this.sleep = options.sleep ?? abortableSleep;
  }

  get isSupervising(): boolean {
    return this.running;
  }

  connect(): void {
    if (this.running) return;
    this.running = true;
    this.generation += 1;
    void this.supervise(this.generation);
  }

  disconnect(): void {
    this.running = false;
    this.generation += 1;
    this.wake?.abort();
    this.report({ kind: 'disconnected' });
  }

  /** Probe now rather than at the next interval — after Start daemon, for instance. */
  refreshNow(): void {
    this.wake?.abort();
  }

  /** Start the remote daemon over ssh (an explicit user action); the error, or null. */
  async startDaemon(): Promise<string | null> {
    let error: string | null;
    try {
      error = await this.backend.startDaemon(this.machine.id);
    } catch (thrown) {
      error = reason(thrown);
    }
    this.refreshNow();
    return error;
  }

  /** A supervisor stays in charge only while it is the latest one. */
  private isCurrent(generation: number): boolean {
    return this.running && generation === this.generation;
  }

  private async supervise(generation: number): Promise<void> {
    let backoff = OFFLINE_BACKOFF_INITIAL_MS;
    let tracker = new HealthProbeTracker();
    this.report({ kind: 'connecting' });
    while (this.isCurrent(generation)) {
      let probe: HealthProbe;
      try {
        probe = await this.backend.probe(this.machine.id);
      } catch (error) {
        probe = { kind: 'failed', reason: reason(error) };
      }
      if (!this.isCurrent(generation)) return;
      let delay: number;
      if (probe.kind === 'offline') {
        tracker = new HealthProbeTracker();
        this.report({ kind: 'offline', reason: probe.reason });
        delay = backoff;
        backoff = Math.min(backoff * 2, OFFLINE_BACKOFF_MAX_MS);
      } else {
        backoff = OFFLINE_BACKOFF_INITIAL_MS;
        let availability: DaemonAvailability | null;
        if (probe.kind === 'stopped') {
          tracker = new HealthProbeTracker();
          availability = { kind: 'stopped' };
        } else {
          availability = tracker.record(probe.kind === 'failed' ? { kind: 'failed' } : probe);
        }
        if (availability !== null) this.report({ kind: 'connected', daemon: availability });
        // An inconclusive probe (BUSY, or a failure short of the limit) retries soon.
        delay = availability === null ? INCONCLUSIVE_PROBE_INTERVAL_MS : PROBE_INTERVAL_MS;
      }
      const wake = new AbortController();
      this.wake = wake;
      await this.sleep(delay, wake.signal);
      if (this.wake === wake) this.wake = null;
    }
  }

  private report(state: MachineConnectionState): void {
    if (connectionStatesEqual(state, this.lastReported)) return;
    this.lastReported = state;
    this.onState(state);
  }
}
