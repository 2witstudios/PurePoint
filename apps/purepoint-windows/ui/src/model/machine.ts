// Port of apps/purepoint-macos/purepoint-macos/Models/Machine.swift (WIN-8.14), with the
// platform-free half of Services/MachineConnection.swift (`HealthProbeTracker`) and the
// machine's status text from State/MachineState.swift.
//
// A machine is a computer reachable over SSH that runs its own PurePoint daemon (machines.md).
// The registry, `%USERPROFILE%\.pu\machines.json`, is shared with `pu machine`; the backend reads
// and writes it under pu-core's lock (`src-tauri/src/daemon/machine.rs`), so this file holds only
// the shape, the validation the Add Machine sheet shows, and the pure rules.
//
// Differences from Swift:
// - Loading, encoding and the locked update are the backend's (pu-core `machines.rs`); the
//   messages below are the Mac's, shown before anything is sent.
// - `SSHConfig.hostAliases(path:)` reads `%USERPROFILE%\.ssh\config` through the backend
//   (`machines_ssh_config`); the parser is here.

import type { DaemonAvailability, MachineConnectionState } from './sidebarItem.ts';

/** A machine in `machines.json` (Swift `Machine`). */
export interface Machine {
  /** Lowercase UUID; names `%USERPROFILE%\.pu\machines\<id>`. */
  readonly id: string;
  readonly name: string;
  /** An `~/.ssh/config` alias or `user@host[:port]`, passed to ssh as one argument. */
  readonly sshHost: string;
}

/** How error messages name the registry (Swift `MachineRegistryStore.displayPath`). */
export const MACHINE_REGISTRY_DISPLAY_PATH = '%USERPROFILE%\\.pu\\machines.json';

// MARK: - Validation (Swift `MachineRegistryStore.validate`)

export const INVALID_NAME_MESSAGE =
  "Use 1–64 letters, digits, '.', '_' or '-', starting with a letter or digit.";
export const INVALID_HOST_MESSAGE =
  'Use an ~/.ssh/config alias or user@host[:port], without spaces.';

export function duplicateNameMessage(name: string): string {
  return `A machine named "${name}" already exists.`;
}

/** Names are what `pu --machine` matches: `[A-Za-z0-9._-]`, starting alphanumeric. */
export function isValidMachineName(name: string): boolean {
  return /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(name);
}

const encoder = new TextEncoder();

/** Passed to ssh as one argument, so it must not look like an option. */
export function isValidSshHost(host: string): boolean {
  if (host === '' || encoder.encode(host).length > 255 || host.startsWith('-')) return false;
  // Whitespace (including newlines) and control characters.
  return !/[\s\p{Cc}]/u.test(host);
}

/** The first problem with a new machine, or null (Swift `MachineRegistryStore.add`). */
export function machineEntryError(
  name: string,
  sshHost: string,
  existing: readonly Machine[] = [],
): string | null {
  if (!isValidMachineName(name)) return INVALID_NAME_MESSAGE;
  if (!isValidSshHost(sshHost)) return INVALID_HOST_MESSAGE;
  const taken = existing.find((machine) => machine.name.toLowerCase() === name.toLowerCase());
  return taken === undefined ? null : duplicateNameMessage(taken.name);
}

// MARK: - SSH destinations (Swift `SSHDestination`)

export const DEFAULT_SSH_PORT = 22;

/** A `user@host[:port]` destination (the only form the iPad has; a PC accepts aliases too). */
export interface SshDestination {
  readonly user: string;
  readonly host: string;
  readonly port: number;
}

function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const value = Number(text);
  return value >= 1 && value <= 65_535 ? value : null;
}

function isValidUser(user: string): boolean {
  return (
    user !== '' &&
    encoder.encode(user).length <= 64 &&
    !user.startsWith('-') &&
    !/[@:\s\p{Cc}]/u.test(user)
  );
}

function isValidHost(host: string): boolean {
  return (
    host !== '' &&
    encoder.encode(host).length <= 253 &&
    !host.startsWith('-') &&
    !/[@[\]/\s\p{Cc}]/u.test(host)
  );
}

/**
 * Parse `user@host`, `user@host:port`, `user@[v6]` or `user@[v6]:port`; null for anything else
 * (an ssh config alias). A bare IPv6 literal without brackets is a host with the default port.
 */
export function parseSshDestination(sshHost: string): SshDestination | null {
  const at = sshHost.lastIndexOf('@');
  if (at === -1) return null;
  const user = sshHost.slice(0, at);
  let rest = sshHost.slice(at + 1);
  let port = DEFAULT_SSH_PORT;
  let host: string;
  if (rest.startsWith('[')) {
    const close = rest.indexOf(']');
    if (close === -1) return null;
    host = rest.slice(1, close);
    rest = rest.slice(close + 1);
    if (rest !== '') {
      const parsed = rest.startsWith(':') ? parsePort(rest.slice(1)) : null;
      if (parsed === null) return null;
      port = parsed;
    }
  } else if (rest.split(':').length === 2) {
    const colon = rest.indexOf(':');
    host = rest.slice(0, colon);
    const parsed = parsePort(rest.slice(colon + 1));
    if (parsed === null) return null;
    port = parsed;
  } else {
    host = rest;
  }
  if (!isValidUser(user) || !isValidHost(host)) return null;
  return { user, host, port };
}

/** The `ssh_host` form: the port only when it is not 22, an IPv6 literal in brackets. */
export function sshHostString(destination: SshDestination): string {
  const host = destination.host.includes(':') ? `[${destination.host}]` : destination.host;
  const port = destination.port === DEFAULT_SSH_PORT ? '' : `:${String(destination.port)}`;
  return `${destination.user}@${host}${port}`;
}

// MARK: - ssh config (Swift `SSHConfig`)

/** Every concrete alias on `Host` lines; patterns (`*`, `?`, negations) are skipped. */
export function sshConfigHostAliases(text: string): string[] {
  const aliases: string[] = [];
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === '' || line.startsWith('#')) continue;
    // `Host a b`, `Host=a`, `host  a` are all valid ssh_config syntax.
    const separator = line.search(/[ \t=]/);
    if (separator === -1) continue;
    if (line.slice(0, separator).toLowerCase() !== 'host') continue;
    const values = line
      .slice(separator + 1)
      .replace(/^[ \t=]+|[ \t=]+$/g, '')
      .split(/[ \t]+/);
    for (const value of values) {
      const alias = value.replace(/^"+|"+$/g, '');
      if (alias === '' || /[*?!]/.test(alias) || aliases.includes(alias)) continue;
      aliases.push(alias);
    }
  }
  return aliases;
}

/** The name Add Machine suggests for a host (Swift `AddMachineSheet.suggestedName`). */
export function suggestedMachineName(alias: string): string {
  const candidate =
    alias
      .split('@')
      .filter((part) => part !== '')
      .at(-1) ?? alias;
  const cleaned = candidate.split(':').filter((part) => part !== '')[0] ?? candidate;
  return cleaned.replace(/[^A-Za-z0-9._-]/g, '');
}

// MARK: - Health probes (Swift `HealthProbe`, `HealthProbeTracker`)

/**
 * One relayed `health` (the backend's `machine_probe`). Windows adds what the relay's exit status
 * says: `stopped` (exit 69, no daemon) and `offline` (ssh failed, or no `pu` there).
 */
export type HealthProbe =
  | { readonly kind: 'running' }
  | { readonly kind: 'incompatible'; readonly version: number }
  | { readonly kind: 'busy' }
  | { readonly kind: 'stopped' }
  | { readonly kind: 'offline'; readonly reason: string }
  | { readonly kind: 'failed'; readonly reason: string };

/** Probes that only count toward a verdict (Swift's `HealthProbe` cases). */
export type TrackedProbe =
  | Extract<HealthProbe, { kind: 'running' | 'incompatible' | 'busy' }>
  | {
      readonly kind: 'failed';
    };

/**
 * Turns probes into what the machine reports. One slow or failed probe must not read as a stopped
 * daemon (that drops its projects), so only several failures in a row do; BUSY is transient and
 * changes nothing.
 */
export class HealthProbeTracker {
  static readonly failuresBeforeStopped = 3;

  private failures = 0;

  get consecutiveFailures(): number {
    return this.failures;
  }

  /** The availability to report after `probe`, or null to keep the last report and retry soon. */
  record(probe: TrackedProbe): DaemonAvailability | null {
    switch (probe.kind) {
      case 'running':
        this.failures = 0;
        return { kind: 'running' };
      case 'incompatible':
        this.failures = 0;
        return { kind: 'incompatible', version: probe.version };
      case 'busy':
        return null;
      case 'failed':
        this.failures += 1;
        return this.failures >= HealthProbeTracker.failuresBeforeStopped
          ? { kind: 'stopped' }
          : null;
    }
  }
}

// MARK: - Status text (Swift `MachineState.statusText`)

export function machineStatusText(state: MachineConnectionState, projectCount: number): string {
  switch (state.kind) {
    case 'disconnected':
      return 'Disconnected';
    case 'connecting':
      return 'Connecting…';
    case 'offline':
      return 'Offline';
    case 'connected':
      switch (state.daemon.kind) {
        case 'running':
          return projectCount === 0
            ? 'Connected'
            : `${String(projectCount)} project${projectCount === 1 ? '' : 's'}`;
        case 'stopped':
          return 'Daemon stopped';
        case 'incompatible':
          return `Incompatible (protocol ${String(state.daemon.version)})`;
      }
  }
}

export function connectionStatesEqual(
  a: MachineConnectionState,
  b: MachineConnectionState,
): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'offline' && b.kind === 'offline') return a.reason === b.reason;
  if (a.kind === 'connected' && b.kind === 'connected') {
    const [x, y] = [a.daemon, b.daemon];
    if (x.kind === 'incompatible' || y.kind === 'incompatible') {
      return x.kind === 'incompatible' && y.kind === 'incompatible' && x.version === y.version;
    }
    return x.kind === y.kind;
  }
  return true;
}
