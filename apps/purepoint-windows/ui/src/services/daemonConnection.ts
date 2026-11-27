// Port of apps/purepoint-macos/purepoint-macos/Services/DaemonConnection.swift (WIN-6.0), with the
// front-end half of Services/DaemonClient.swift and Services/DaemonTransport.swift.
//
// In the app every daemon request goes through the Tauri backend (`src-tauri/src/daemon/`), which
// owns the named-pipe connection, the newline framing and the strict serde decoding; the front end
// is handed decoded `Response`s. What this module keeps is the seam the app's state talks to
// (`DaemonRequester`, one per daemon endpoint), and the Swift client's transport-independent parts
// for any in-process byte stream: the line reader, hex coding, `DaemonClient.parse` and a
// line-framed client over an injectable transport (Swift's `DaemonTransport`), which the tests
// drive with an in-memory daemon exactly as DaemonTransportTests does.

import type {
  AgentDefInfo,
  AgentEntry,
  AgentStatusReport,
  Request,
  Response,
  SignalSnapshot,
  SpawnPlacement,
  SwarmDefInfo,
  TemplateInfo,
  WorktreeEntry,
  WorktreeStatus,
} from '../protocol';
import { normalizeAgentStatus } from '../model/agentStatus';
import { daemonRequest } from './daemon';

// MARK: - Requesters

/** One daemon the app talks to (Swift `DaemonClient.send`). Error replies resolve as `error`. */
export interface DaemonRequester {
  send(request: Request): Promise<Response>;
}

/** The local daemon, through the backend (`daemon_request`), which starts it if needed. */
export const localDaemonClient: DaemonRequester = { send: daemonRequest };

// A machine's daemon is `machineDaemonClient` (services/machines.ts, WIN-8.14): the backend's
// ssh relay carries every request to it.

// MARK: - Errors

export type DaemonClientErrorCode =
  'eof' | 'cancelled' | 'notRunning' | 'connectTimeout' | 'busy' | 'requestTimeout';

const ERROR_DESCRIPTIONS: Readonly<Record<DaemonClientErrorCode, string>> = {
  eof: 'Connection to daemon closed',
  cancelled: 'Connection cancelled',
  notRunning: 'Daemon is not running',
  connectTimeout: 'Timed out connecting to daemon',
  busy: 'Daemon is at its connection limit',
  requestTimeout: 'Timed out waiting for the daemon',
};

/** Swift `DaemonClientError`, with its `errorDescription`s. */
export class DaemonClientError extends Error {
  readonly code: DaemonClientErrorCode;

  constructor(code: DaemonClientErrorCode, message: string = ERROR_DESCRIPTIONS[code]) {
    super(message);
    this.name = 'DaemonClientError';
    this.code = code;
  }
}

export function isDaemonClientError(
  error: unknown,
  code?: DaemonClientErrorCode,
): error is DaemonClientError {
  return error instanceof DaemonClientError && (code === undefined || error.code === code);
}

// MARK: - Hex

const HEX_DIGITS = '0123456789abcdef';

/** Swift `Data.hexString`: lowercase, two digits per byte. */
export function hexString(bytes: Uint8Array): string {
  let out = '';
  for (const byte of bytes) out += HEX_DIGITS.charAt(byte >> 4) + HEX_DIGITS.charAt(byte & 0x0f);
  return out;
}

/**
 * Swift `Data(hexString:)`: two digits at a time; a pair that is not hex is skipped, and a lone
 * last digit is a byte of its own.
 */
export function bytesFromHex(hex: string): Uint8Array {
  const out: number[] = [];
  for (let index = 0; index < hex.length; index += 2) {
    const pair = hex.slice(index, index + 2);
    if (/^[0-9a-fA-F]{1,2}$/.test(pair)) out.push(parseInt(pair, 16));
  }
  return Uint8Array.from(out);
}

// MARK: - Byte streams and the line reader

/** Swift `DaemonByteStream`: one connection. `receive` rejects with `eof` once it has closed. */
export interface DaemonByteStream {
  send(data: Uint8Array): Promise<void>;
  receive(): Promise<Uint8Array>;
  close(): void;
}

/** Swift `DaemonTransport`: opens one stream per request or subscription. */
export interface DaemonTransport {
  openStream(): Promise<DaemonByteStream>;
}

const NEWLINE = 0x0a;

/** Swift `DaemonLineReader`: newline-delimited frames out of arbitrary chunks. */
export class DaemonLineReader {
  private buffer = new Uint8Array(0);
  private scanOffset = 0;

  constructor(private readonly stream: DaemonByteStream) {}

  /** The next line without its newline; rejects with `eof` when the stream ends first. */
  async readLine(): Promise<Uint8Array> {
    for (;;) {
      const newline = this.buffer.indexOf(NEWLINE, this.scanOffset);
      if (newline !== -1) {
        const line = this.buffer.slice(this.scanOffset, newline);
        this.scanOffset = newline + 1;
        // Compact when the consumed portion exceeds half the buffer.
        if (this.scanOffset > this.buffer.length / 2) {
          this.buffer = this.buffer.slice(this.scanOffset);
          this.scanOffset = 0;
        }
        return line;
      }
      const chunk = await this.stream.receive();
      const next = new Uint8Array(this.buffer.length + chunk.length);
      next.set(this.buffer);
      next.set(chunk, this.buffer.length);
      this.buffer = next;
    }
  }
}

// MARK: - Parsing (Swift `DaemonClient.parse`)

/** What `parse` returns for a line it cannot decode (`parse_error`) or a type it does not know. */
export interface UnknownResponse {
  readonly type: 'unknown';
  readonly responseType: string;
}

export type ParsedResponse = Response | UnknownResponse;

class DecodeError extends Error {}

type Decoder<T> = (value: unknown, path: string) => T;

function fail(path: string): never {
  throw new DecodeError(path);
}

const string: Decoder<string> = (v, p) => (typeof v === 'string' ? v : fail(p));
const number: Decoder<number> = (v, p) =>
  typeof v === 'number' && Number.isFinite(v) ? v : fail(p);
const boolean: Decoder<boolean> = (v, p) => (typeof v === 'boolean' ? v : fail(p));

function object(v: unknown, p: string): Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : fail(p);
}

/** `decodeIfPresent ?? fallback`: absent or null is the fallback, anything else must decode. */
function optional<T, F>(decode: Decoder<T>, fallback: F): Decoder<T | F> {
  return (v, p) => (v === undefined || v === null ? fallback : decode(v, p));
}

/** Swift `try? decodeIfPresent`: anything that does not decode is null. */
function lenient<T>(decode: Decoder<T>): Decoder<T | null> {
  return (v, p) => {
    if (v === undefined || v === null) return null;
    try {
      return decode(v, p);
    } catch {
      return null;
    }
  };
}

function array<T>(decode: Decoder<T>): Decoder<T[]> {
  return (v, p) =>
    Array.isArray(v) ? v.map((item, i) => decode(item, `${p}[${String(i)}]`)) : fail(p);
}

function record<T>(decode: Decoder<T>): Decoder<Record<string, T>> {
  return (v, p) => {
    const source = object(v, p);
    const out: Record<string, T> = {};
    for (const [key, item] of Object.entries(source)) out[key] = decode(item, `${p}.${key}`);
    return out;
  };
}

function oneOf<T extends string>(values: readonly T[]): Decoder<T> {
  return (v, p) => (values.includes(v as T) ? (v as T) : fail(p));
}

/** A field of `o`, decoded. */
function at<T>(o: Record<string, unknown>, key: string, decode: Decoder<T>, path: string): T {
  return decode(o[key], `${path}.${key}`);
}

const placement: Decoder<SpawnPlacement> = (v, p) => {
  const o = object(v, p);
  return { workspace: at(o, 'workspace', string, p), surface: at(o, 'surface', number, p) };
};

/** Signals are opaque to parsing; a malformed one is dropped as Swift's `try?` drops it. */
const signal: Decoder<SignalSnapshot> = (v, p) => object(v, p) as unknown as SignalSnapshot;

/** Swift accepts every raw status and normalizes legacy ones (`AgentStatus.normalized`). */
const agentStatus: Decoder<AgentEntry['status']> = (v, p) => normalizeAgentStatus(string(v, p));

const WORKTREE_STATUSES: readonly WorktreeStatus[] = [
  'active',
  'merging',
  'merged',
  'failed',
  'cleaned',
];

/** Swift `AgentStatusReport.init(from:)`, with its defaults. */
export const decodeAgentStatusReport: Decoder<AgentStatusReport> = (v, p) => {
  const o = object(v, p);
  const report: AgentStatusReport = {
    id: at(o, 'id', string, p),
    name: at(o, 'name', string, p),
    agent_type: at(o, 'agent_type', optional(string, 'unknown'), p),
    status: at(o, 'status', agentStatus, p),
    pid: at(o, 'pid', optional(number, null), p),
    exit_code: at(o, 'exit_code', optional(number, null), p),
    idle_seconds: at(o, 'idle_seconds', optional(number, null), p),
    worktree_id: at(o, 'worktree_id', optional(string, null), p),
    started_at: at(o, 'started_at', optional(string, ''), p),
    suspended: at(o, 'suspended', optional(boolean, false), p),
    signal: at(o, 'signal', lenient(signal), p),
    placement: at(o, 'placement', lenient(placement), p),
  };
  const sessionId = at(o, 'session_id', optional(string, undefined), p);
  const prompt = at(o, 'prompt', optional(string, undefined), p);
  if (sessionId !== undefined) report.session_id = sessionId;
  if (prompt !== undefined) report.prompt = prompt;
  return report;
};

/** Swift `AgentEntry.init(from:)` (ManifestModel.swift). */
export const decodeAgentEntry: Decoder<AgentEntry> = (v, p) => {
  const o = object(v, p);
  const suspendedAt = at(o, 'suspendedAt', optional(string, undefined), p);
  const entry: AgentEntry = {
    id: at(o, 'id', string, p),
    name: at(o, 'name', string, p),
    agentType: at(o, 'agentType', string, p),
    status: at(o, 'status', agentStatus, p),
    prompt: at(o, 'prompt', optional(string, null), p),
    startedAt: at(o, 'startedAt', string, p),
    // pu-core infers `suspended` from `suspendedAt` for old manifests.
    suspended: at(o, 'suspended', optional(boolean, suspendedAt !== undefined), p),
    signal: at(o, 'signal', lenient(signal), p),
    placement: at(o, 'placement', lenient(placement), p),
  };
  const optionalFields = {
    completedAt: at(o, 'completedAt', optional(string, undefined), p),
    exitCode: at(o, 'exitCode', optional(number, undefined), p),
    error: at(o, 'error', optional(string, undefined), p),
    pid: at(o, 'pid', optional(number, undefined), p),
    sessionId: at(o, 'sessionId', optional(string, undefined), p),
    suspendedAt,
    command: at(o, 'command', optional(string, undefined), p),
  };
  for (const [key, value] of Object.entries(optionalFields)) {
    if (value !== undefined) Object.assign(entry, { [key]: value });
  }
  return entry;
};

/** Swift `WorktreeEntry.init(from:)`: `branch`, `path` and the rest are required. */
export const decodeWorktreeEntry: Decoder<WorktreeEntry> = (v, p) => {
  const o = object(v, p);
  const entry: WorktreeEntry = {
    id: at(o, 'id', string, p),
    name: at(o, 'name', string, p),
    path: at(o, 'path', string, p),
    branch: at(o, 'branch', string, p),
    baseBranch: at(o, 'baseBranch', optional(string, null), p),
    status: at(o, 'status', oneOf(WORKTREE_STATUSES), p),
    agents: at(o, 'agents', optional(record(decodeAgentEntry), {}), p),
    createdAt: at(o, 'createdAt', string, p),
  };
  const mergedAt = at(o, 'mergedAt', optional(string, undefined), p);
  if (mergedAt !== undefined) entry.mergedAt = mergedAt;
  return entry;
};

const strings = array(string);

const templateInfo: Decoder<TemplateInfo> = (v, p) => {
  const o = object(v, p);
  return {
    name: at(o, 'name', string, p),
    description: at(o, 'description', string, p),
    agent: at(o, 'agent', string, p),
    source: at(o, 'source', string, p),
    variables: at(o, 'variables', optional(strings, []), p),
    command: at(o, 'command', optional(string, null), p),
  };
};

const agentDefInfo: Decoder<AgentDefInfo> = (v, p) => {
  const o = object(v, p);
  return {
    name: at(o, 'name', string, p),
    agent_type: at(o, 'agent_type', string, p),
    template: at(o, 'template', optional(string, null), p),
    inline_prompt: at(o, 'inline_prompt', optional(string, null), p),
    tags: at(o, 'tags', optional(strings, []), p),
    scope: at(o, 'scope', string, p),
    available_in_command_dialog: at(o, 'available_in_command_dialog', optional(boolean, false), p),
    icon: at(o, 'icon', optional(string, null), p),
    command: at(o, 'command', optional(string, null), p),
  };
};

const swarmDefInfo: Decoder<SwarmDefInfo> = (v, p) => {
  const o = object(v, p);
  return {
    name: at(o, 'name', string, p),
    worktree_count: at(o, 'worktree_count', number, p),
    worktree_template: at(o, 'worktree_template', optional(string, ''), p),
    roster: at(
      o,
      'roster',
      array((entry, q) => {
        const r = object(entry, q);
        return {
          agent_def: at(r, 'agent_def', string, q),
          role: at(r, 'role', string, q),
          quantity: at(r, 'quantity', optional(number, 1), q),
        };
      }),
      p,
    ),
    include_terminal: at(o, 'include_terminal', optional(boolean, false), p),
    scope: at(o, 'scope', string, p),
  };
};

type Variant<K extends Response['type']> = Extract<Response, { type: K }>;
type VariantDecoder<K extends Response['type']> = (o: Record<string, unknown>) => Variant<K>;

/** The message passed through unchanged once its `type` is known (no field the app reads). */
function passthrough<K extends Response['type']>(): VariantDecoder<K> {
  return (o) => o as Variant<K>;
}

const statusPayload = (o: Record<string, unknown>) => ({
  worktrees: at(o, 'worktrees', optional(array(decodeWorktreeEntry), []), '$'),
  agents: at(o, 'agents', optional(array(decodeAgentStatusReport), []), '$'),
});

/**
 * Every response type, so `tsc` fails when the protocol gains one. Decoded field by field where
 * the app's state reads the message; passed through where only its type matters here.
 */
const DECODERS: { readonly [K in Response['type']]: VariantDecoder<K> } = {
  channel_history: passthrough(),
  channel_message: passthrough(),
  inventory: passthrough(),
  agent_resolved: passthrough(),
  project_order: (o) => ({
    type: 'project_order',
    project_roots: at(o, 'project_roots', strings, '$'),
  }),
  health_report: (o) => ({
    type: 'health_report',
    pid: at(o, 'pid', number, '$'),
    uptime_seconds: at(o, 'uptime_seconds', number, '$'),
    protocol_version: at(o, 'protocol_version', number, '$'),
    projects: at(o, 'projects', optional(strings, []), '$'),
    agent_count: at(o, 'agent_count', optional(number, 0), '$'),
  }),
  init_result: (o) => ({ type: 'init_result', created: at(o, 'created', boolean, '$') }),
  project_removed: passthrough(),
  projects_subscribed: () => ({ type: 'projects_subscribed' }),
  projects_changed: (o) => ({
    type: 'projects_changed',
    projects: at(o, 'projects', strings, '$'),
  }),
  spawn_result: (o) => ({
    type: 'spawn_result',
    worktree_id: at(o, 'worktree_id', optional(string, null), '$'),
    agent_id: at(o, 'agent_id', string, '$'),
    status: at(o, 'status', agentStatus, '$'),
  }),
  status_report: (o) => ({ type: 'status_report', ...statusPayload(o) }),
  agent_status: (o) => ({ type: 'agent_status', ...decodeAgentStatusReport(o, '$') }),
  kill_result: passthrough(),
  suspend_result: passthrough(),
  resume_result: passthrough(),
  logs_result: passthrough(),
  read_result: passthrough(),
  attach_ready: (o) => ({
    type: 'attach_ready',
    buffered_bytes: at(o, 'buffered_bytes', number, '$'),
  }),
  resync: () => ({ type: 'resync' }),
  pty_size: (o) => ({
    type: 'pty_size',
    cols: at(o, 'cols', number, '$'),
    rows: at(o, 'rows', number, '$'),
    owner_client_id: at(o, 'owner_client_id', optional(string, null), '$'),
  }),
  output: (o) => ({
    type: 'output',
    agent_id: at(o, 'agent_id', string, '$'),
    data: at(o, 'data', string, '$'),
  }),
  grid_subscribed: () => ({ type: 'grid_subscribed' }),
  grid_layout: passthrough(),
  grid_event: passthrough(),
  status_subscribed: () => ({ type: 'status_subscribed' }),
  status_event: (o) => ({ type: 'status_event', ...statusPayload(o) }),
  rename_result: passthrough(),
  assign_trigger_result: passthrough(),
  create_worktree_result: (o) => ({
    type: 'create_worktree_result',
    worktree_id: at(o, 'worktree_id', string, '$'),
  }),
  delete_worktree_result: passthrough(),
  template_list: (o) => ({
    type: 'template_list',
    templates: at(o, 'templates', array(templateInfo), '$'),
  }),
  template_detail: (o) => ({
    type: 'template_detail',
    name: at(o, 'name', string, '$'),
    description: at(o, 'description', string, '$'),
    agent: at(o, 'agent', string, '$'),
    body: at(o, 'body', string, '$'),
    source: at(o, 'source', string, '$'),
    variables: at(o, 'variables', optional(strings, []), '$'),
    command: at(o, 'command', optional(string, null), '$'),
  }),
  agent_def_list: (o) => ({
    type: 'agent_def_list',
    agent_defs: at(o, 'agent_defs', array(agentDefInfo), '$'),
  }),
  agent_def_detail: passthrough(),
  swarm_def_list: (o) => ({
    type: 'swarm_def_list',
    swarm_defs: at(o, 'swarm_defs', array(swarmDefInfo), '$'),
  }),
  swarm_def_detail: passthrough(),
  run_swarm_result: (o) => ({
    type: 'run_swarm_result',
    spawned_agents: at(o, 'spawned_agents', strings, '$'),
  }),
  run_swarm_partial: passthrough(),
  schedule_list: passthrough(),
  schedule_detail: passthrough(),
  global_agent_settings_report: passthrough(),
  config_report: passthrough(),
  trigger_list: passthrough(),
  trigger_detail: passthrough(),
  gate_result: passthrough(),
  diff_result: passthrough(),
  pulse_report: passthrough(),
  fs_list_result: passthrough(),
  fs_stat_result: (o) => ({
    type: 'fs_stat_result',
    path: at(o, 'path', string, '$'),
    exists: at(o, 'exists', boolean, '$'),
    is_dir: at(o, 'is_dir', boolean, '$'),
    size: at(o, 'size', number, '$'),
    modified_ms: at(o, 'modified_ms', number, '$'),
  }),
  fs_read_result: (o) => ({
    type: 'fs_read_result',
    path: at(o, 'path', string, '$'),
    data_base64: at(o, 'data_base64', string, '$'),
    size: at(o, 'size', number, '$'),
    modified_ms: at(o, 'modified_ms', number, '$'),
    truncated: at(o, 'truncated', boolean, '$'),
  }),
  fs_write_result: (o) => ({
    type: 'fs_write_result',
    path: at(o, 'path', string, '$'),
    size: at(o, 'size', number, '$'),
    modified_ms: at(o, 'modified_ms', number, '$'),
  }),
  fs_subscribed: () => ({ type: 'fs_subscribed' }),
  fs_event: passthrough(),
  attach_file_result: passthrough(),
  git_exec_result: passthrough(),
  ok: () => ({ type: 'ok' }),
  shutting_down: () => ({ type: 'shutting_down' }),
  error: (o) => ({
    type: 'error',
    code: at(o, 'code', string, '$'),
    message: at(o, 'message', string, '$'),
  }),
};

const PARSE_ERROR: UnknownResponse = { type: 'unknown', responseType: 'parse_error' };

/**
 * Swift `DaemonClient.parse`: one line of daemon JSON. A line that is not JSON, or whose known
 * type does not decode, is `unknown("parse_error")`; a type this build does not know is
 * `unknown(<type>)`.
 */
export function parseDaemonResponse(line: Uint8Array | string): ParsedResponse {
  let value: unknown;
  try {
    value = JSON.parse(typeof line === 'string' ? line : new TextDecoder().decode(line));
  } catch {
    return PARSE_ERROR;
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return PARSE_ERROR;
  const o = value as Record<string, unknown>;
  if (typeof o.type !== 'string') return PARSE_ERROR;
  if (!Object.hasOwn(DECODERS, o.type)) return { type: 'unknown', responseType: o.type };
  const decode = DECODERS[o.type as Response['type']] as (o: Record<string, unknown>) => Response;
  try {
    return decode(o);
  } catch (error) {
    if (error instanceof DecodeError) return PARSE_ERROR;
    throw error;
  }
}

// MARK: - A line-framed client over a transport

/** The default request timeout (Swift `DaemonClient.send`'s per-request budget). */
export const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;

/**
 * Swift `DaemonClient(transport:)`: one stream per request, the request as a JSON line, the first
 * reply line parsed. A stream that ends first rejects with `eof`; one that never answers with
 * `requestTimeout`.
 */
export class DaemonClient {
  constructor(readonly transport: DaemonTransport) {}

  async send(
    request: Request,
    timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS,
  ): Promise<ParsedResponse> {
    const stream = await this.transport.openStream();
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await stream.send(new TextEncoder().encode(JSON.stringify(request) + '\n'));
      const timeout = new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          reject(new DaemonClientError('requestTimeout'));
        }, timeoutMs);
      });
      const line = await Promise.race([new DaemonLineReader(stream).readLine(), timeout]);
      return parseDaemonResponse(line);
    } finally {
      clearTimeout(timer);
      stream.close();
    }
  }
}
