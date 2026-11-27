// Port of apps/purepoint-macos/purepoint-macos/Models/ProjectKey.swift (WIN-6.5).
//
// Only the identity half is ported here: `ProjectKey` and the endpoint/path naming. How an
// endpoint is reached (the Unix socket or SSH transport, `makeClient`, `ensureDirectory`) is the
// Rust backend's job on Windows, so those members have no TS counterpart.

/**
 * Project identity: `(machineId, projectRoot)` (MACH-005). `machineId === null` is the local
 * machine.
 *
 * The string form (`projectKeyId`) is what the registry, sidebar, selection and persisted UI state
 * key projects by. A local key is the bare root; a remote key is `machine:<uuid>:<root>`, which can
 * never collide with a local root (roots are absolute).
 */
export interface ProjectKey {
  /** Lowercase UUID of the machine that owns the project, or `null` for this computer. */
  readonly machineId: string | null;
  /** Absolute path of the project root on the machine that owns it. */
  readonly root: string;
}

export const PROJECT_KEY_REMOTE_PREFIX = 'machine:';

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Whether `value` parses as a UUID (Swift `UUID(uuidString:)`, either case). */
export function isUuid(value: string): boolean {
  return UUID_PATTERN.test(value);
}

export function projectKey(root: string, machineId: string | null = null): ProjectKey {
  return { machineId: machineId === null ? null : machineId.toLowerCase(), root };
}

export function isRemoteKey(key: ProjectKey): boolean {
  return key.machineId !== null;
}

export function projectKeyId(key: ProjectKey): string {
  if (key.machineId === null) return key.root;
  return PROJECT_KEY_REMOTE_PREFIX + key.machineId.toLowerCase() + ':' + key.root;
}

/** Parse a key's string form. Anything that is not a well-formed remote key is a local root. */
export function parseProjectKey(id: string): ProjectKey {
  if (id.startsWith(PROJECT_KEY_REMOTE_PREFIX)) {
    const rest = id.slice(PROJECT_KEY_REMOTE_PREFIX.length);
    const colon = rest.indexOf(':');
    if (colon !== -1) {
      const uuid = rest.slice(0, colon);
      if (isUuid(uuid)) return projectKey(rest.slice(colon + 1), uuid);
    }
  }
  return projectKey(id);
}

/**
 * The last path component, as `URL(fileURLWithPath:).lastPathComponent` gives it: trailing
 * separators are ignored and the filesystem root names itself. Both separators count, since a
 * Windows root uses `\` and a remote machine's root uses `/`.
 */
export function lastPathComponent(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  if (trimmed === '') return path === '' ? '' : path.slice(0, 1);
  const index = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return trimmed.slice(index + 1);
}

export function projectName(key: ProjectKey): string {
  return lastPathComponent(key.root);
}

// MARK: - Endpoints

/**
 * Where a project's daemon listens. Local is `<pu>/daemon.sock`; a machine's endpoint is the local
 * end of its SSH forward, `<pu>/machines/<id>/daemon.sock`. On Windows the backend owns the actual
 * paths (DEC-3), so `puDirectory` is an input rather than `$HOME/.pu`.
 */
export interface DaemonEndpoint {
  readonly machineId: string | null;
  readonly socketPath: string;
}

export function isLocalEndpoint(endpoint: DaemonEndpoint): boolean {
  return endpoint.machineId === null;
}

export function localEndpoint(puDirectory: string): DaemonEndpoint {
  return { machineId: null, socketPath: puDirectory + '/daemon.sock' };
}

export function machineEndpoint(machineId: string, puDirectory: string): DaemonEndpoint {
  return {
    machineId: machineId.toLowerCase(),
    socketPath: machineSocketPath(machineId, puDirectory),
  };
}

/** Filesystem locations shared with the CLI (`crates/pu-core/src/paths.rs`). */
export function machinesDirectory(puDirectory: string): string {
  return puDirectory + '/machines';
}

/** Machine ids are written lowercase, as the CLI's UUID formatter does. */
export function machineDirectory(machineId: string, puDirectory: string): string {
  return machinesDirectory(puDirectory) + '/' + machineId.toLowerCase();
}

export function machineSocketPath(machineId: string, puDirectory: string): string {
  return machineDirectory(machineId, puDirectory) + '/daemon.sock';
}
