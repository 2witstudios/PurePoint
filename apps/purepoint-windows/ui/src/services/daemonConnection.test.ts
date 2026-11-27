// Ports DaemonClientParseTests.swift, HexEncodingTests.swift and the platform-neutral cases of
// DaemonTransportTests.swift (WIN-6.0). Titles carry the Swift test names.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const {
  DaemonClient,
  DaemonClientError,
  DaemonLineReader,
  bytesFromHex,
  hexString,
  parseDaemonResponse,
} = await import('./daemonConnection');
type DaemonByteStream = import('./daemonConnection').DaemonByteStream;
type DaemonTransport = import('./daemonConnection').DaemonTransport;
type ParsedResponse = import('./daemonConnection').ParsedResponse;

const utf8 = (text: string) => new TextEncoder().encode(text);
const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes);

function expectType<K extends ParsedResponse['type']>(
  response: ParsedResponse,
  type: K,
): Extract<ParsedResponse, { type: K }> {
  expect(response.type).toBe(type);
  return response as Extract<ParsedResponse, { type: K }>;
}

describe('DaemonClientParseTests', () => {
  it('testParseHealthReport', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"health_report","pid":1234,"uptime_seconds":60,"protocol_version":1,"agent_count":3}',
      ),
      'health_report',
    );
    expect(response.pid).toBe(1234);
    expect(response.uptime_seconds).toBe(60);
    expect(response.protocol_version).toBe(1);
    expect(response.agent_count).toBe(3);
  });

  it('testParseStatusReport', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"status_report","worktrees":[],"agents":[{"id":"ag-1","name":"test","status":"running"}]}',
      ),
      'status_report',
    );
    expect(response.worktrees).toEqual([]);
    expect(response.agents).toHaveLength(1);
    expect(response.agents[0]?.id).toBe('ag-1');
    expect(response.agents[0]?.name).toBe('test');
    expect(response.agents[0]?.status).toBe('running');
  });

  it('testParseStatusReportWithAgentOptionalFields', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"status_report","worktrees":[],"agents":[{"id":"ag-1","name":"test","status":"completed","pid":9876,"exit_code":0,"idle_seconds":120,"worktree_id":"wt-1"}]}',
      ),
      'status_report',
    );
    expect(response.agents).toHaveLength(1);
    const agent = response.agents[0];
    expect(agent?.pid).toBe(9876);
    expect(agent?.exit_code).toBe(0);
    expect(agent?.idle_seconds).toBe(120);
    expect(agent?.worktree_id).toBe('wt-1');
    // Legacy `completed` normalizes as Swift's `AgentStatus.normalized` does.
    expect(agent?.status).toBe('broken');
  });

  it('testParseStatusReportWithWorktrees', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"status_report","worktrees":[{"id":"wt-1","name":"feature-auth","path":"/tmp/wt","branch":"pu/feature-auth","status":"active","agents":{"ag-1":{"id":"ag-1","name":"claude-1","agentType":"claude","status":"running","prompt":"add auth","startedAt":"2026-01-01T00:00:00Z"}},"createdAt":"2026-01-01T00:00:00Z"}],"agents":[]}',
      ),
      'status_report',
    );
    expect(response.worktrees).toHaveLength(1);
    expect(response.worktrees[0]?.id).toBe('wt-1');
    expect(response.worktrees[0]?.branch).toBe('pu/feature-auth');
    expect(Object.keys(response.worktrees[0]?.agents ?? {})).toHaveLength(1);
    expect(response.worktrees[0]?.agents['ag-1']?.name).toBe('claude-1');
    expect(response.agents).toEqual([]);
  });

  it('testParsePtySizeOwnershipAndResync', () => {
    const size = expectType(
      parseDaemonResponse('{"type":"pty_size","cols":100,"rows":30,"owner_client_id":"mac:test"}'),
      'pty_size',
    );
    expect(size.cols).toBe(100);
    expect(size.rows).toBe(30);
    expect(size.owner_client_id).toBe('mac:test');
    expectType(parseDaemonResponse('{"type":"resync"}'), 'resync');
  });

  // The attach request is built and sent by the backend (`src-tauri/src/attach/session.rs`);
  // `given_attach_should_forward_ready_replay_live_resync_and_sizes_in_order` asserts its
  // `client_id`, `replay: "snapshot"` and the explicit `claim_size`.
  it.todo('testIdentifiedAttachOptsIntoSnapshotAndClaimIsExplicit (Rust attach session tests)');
  // AppKit `ScrollableTerminal` view behavior: the xterm.js terminal view's leaf (WIN-7.x).
  it.todo('testReportedSizeDoesNotReplaceLocalWindowDimensions (terminal view leaf)');
  it.todo('testRemoteTerminalKeepsSelectionAndLinksOnViewingMac (terminal view leaf)');
  it.todo('testTerminalLinkHitUsesPhysicalCellsAndPreservesOSCHyperlinks (terminal view leaf)');

  it('testParseAttachReady', () => {
    const response = expectType(
      parseDaemonResponse('{"type":"attach_ready","buffered_bytes":4096}'),
      'attach_ready',
    );
    expect(response.buffered_bytes).toBe(4096);
  });

  it('testParseOutputDecodesHexData', () => {
    // "hello" = 68656c6c6f
    const response = expectType(
      parseDaemonResponse('{"type":"output","agent_id":"ag-1","data":"68656c6c6f"}'),
      'output',
    );
    expect(response.agent_id).toBe('ag-1');
    expect(text(bytesFromHex(response.data))).toBe('hello');
  });

  it('testParseError', () => {
    const response = expectType(
      parseDaemonResponse('{"type":"error","code":"AGENT_NOT_FOUND","message":"no such agent"}'),
      'error',
    );
    expect(response.code).toBe('AGENT_NOT_FOUND');
    expect(response.message).toBe('no such agent');
  });

  it('testParseOk', () => {
    expectType(parseDaemonResponse('{"type":"ok"}'), 'ok');
  });

  it('testParseShuttingDown', () => {
    expectType(parseDaemonResponse('{"type":"shutting_down"}'), 'shutting_down');
  });

  it('testParseUnknownType', () => {
    const response = expectType(
      parseDaemonResponse('{"type":"future_response_type","data":123}'),
      'unknown',
    );
    expect(response.responseType).toBe('future_response_type');
  });

  it('testParseMalformedJSON', () => {
    const response = expectType(parseDaemonResponse(utf8('not valid json')), 'unknown');
    expect(response.responseType).toBe('parse_error');
  });

  it('testParseTemplateList', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"template_list","templates":[{"name":"review","description":"Code review","agent":"claude","source":"local","variables":["BRANCH"]}]}',
      ),
      'template_list',
    );
    expect(response.templates).toHaveLength(1);
    const template = response.templates[0];
    expect(template?.name).toBe('review');
    expect(template?.description).toBe('Code review');
    expect(template?.agent).toBe('claude');
    expect(template?.source).toBe('local');
    expect(template?.variables).toEqual(['BRANCH']);
  });

  it('testParseTemplateDetail', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"template_detail","name":"review","description":"Code review","agent":"claude","body":"Review {{BRANCH}}.","source":"local","variables":["BRANCH"]}',
      ),
      'template_detail',
    );
    expect(response.name).toBe('review');
    expect(response.description).toBe('Code review');
    expect(response.agent).toBe('claude');
    expect(response.body).toBe('Review {{BRANCH}}.');
    expect(response.source).toBe('local');
    expect(response.variables).toEqual(['BRANCH']);
    expect(response.command).toBeNull();
  });

  it('testParseAgentDefList', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"agent_def_list","agent_defs":[{"name":"reviewer","agent_type":"claude","template":"review","tags":["review"],"scope":"local","available_in_command_dialog":true,"icon":"shield"}]}',
      ),
      'agent_def_list',
    );
    expect(response.agent_defs).toHaveLength(1);
    const def = response.agent_defs[0];
    expect(def?.name).toBe('reviewer');
    expect(def?.agent_type).toBe('claude');
    expect(def?.template).toBe('review');
    expect(def?.tags).toEqual(['review']);
    expect(def?.scope).toBe('local');
    expect(def?.available_in_command_dialog).toBe(true);
    expect(def?.icon).toBe('shield');
  });

  it('testParseAgentDefListWithNulls', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"agent_def_list","agent_defs":[{"name":"basic","agent_type":"claude","template":null,"tags":[],"scope":"global","available_in_command_dialog":false,"icon":null}]}',
      ),
      'agent_def_list',
    );
    expect(response.agent_defs).toHaveLength(1);
    const def = response.agent_defs[0];
    expect(def?.name).toBe('basic');
    expect(def?.template).toBeNull();
    expect(def?.tags).toEqual([]);
    expect(def?.available_in_command_dialog).toBe(false);
    expect(def?.icon).toBeNull();
  });

  it('testParseSwarmDefList', () => {
    const response = expectType(
      parseDaemonResponse(
        '{"type":"swarm_def_list","swarm_defs":[{"name":"full-stack","worktree_count":3,"worktree_template":"feature","roster":[{"agent_def":"reviewer","role":"review","quantity":2}],"include_terminal":true,"scope":"local"}]}',
      ),
      'swarm_def_list',
    );
    expect(response.swarm_defs).toHaveLength(1);
    const swarm = response.swarm_defs[0];
    expect(swarm?.name).toBe('full-stack');
    expect(swarm?.worktree_count).toBe(3);
    expect(swarm?.worktree_template).toBe('feature');
    expect(swarm?.roster).toHaveLength(1);
    expect(swarm?.roster[0]?.agent_def).toBe('reviewer');
    expect(swarm?.roster[0]?.role).toBe('review');
    expect(swarm?.roster[0]?.quantity).toBe(2);
    expect(swarm?.include_terminal).toBe(true);
    expect(swarm?.scope).toBe('local');
  });

  it('testParseRunSwarmResult', () => {
    const response = expectType(
      parseDaemonResponse('{"type":"run_swarm_result","spawned_agents":["ag-abc","ag-def"]}'),
      'run_swarm_result',
    );
    expect(response.spawned_agents).toEqual(['ag-abc', 'ag-def']);
  });

  it('testParseTemplateListEmpty', () => {
    const response = expectType(
      parseDaemonResponse('{"type":"template_list","templates":[]}'),
      'template_list',
    );
    expect(response.templates).toEqual([]);
  });
});

describe('HexEncodingTests', () => {
  it('testHexRoundTrip', () => {
    const original = utf8('Hello, World!');
    // Compared as plain arrays: jsdom's TextEncoder returns another realm's Uint8Array.
    expect(Array.from(bytesFromHex(hexString(original)))).toEqual(Array.from(original));
  });

  it('testEmptyData', () => {
    expect(hexString(new Uint8Array())).toBe('');
    expect(bytesFromHex('')).toHaveLength(0);
  });

  it('testOddLengthHex', () => {
    // "ab" → 0xAB, then "c" alone → 0x0C.
    expect(bytesFromHex('abc')).toEqual(Uint8Array.from([0xab, 0x0c]));
  });

  it('testInvalidHexChars', () => {
    // "68" → 'h', "zz" skipped, "6f" → 'o'.
    expect(bytesFromHex('68zz6f')).toEqual(Uint8Array.from([0x68, 0x6f]));
  });

  it('testBinaryData', () => {
    const binary = Uint8Array.from([0x00, 0xff, 0x80, 0x01]);
    expect(hexString(binary)).toBe('00ff8001');
    expect(bytesFromHex('00ff8001')).toEqual(binary);
  });
});

// MARK: - In-memory fixture (DaemonTransportTests' InMemoryPipe / InMemoryByteStream)

class InMemoryPipe {
  private chunks: Uint8Array[] = [];
  private finished = false;
  private waiter: { resolve: (d: Uint8Array) => void; reject: (e: Error) => void } | null = null;

  push(data: Uint8Array): void {
    if (this.waiter !== null) {
      const waiter = this.waiter;
      this.waiter = null;
      waiter.resolve(data);
      return;
    }
    this.chunks.push(data);
  }

  finish(): void {
    this.finished = true;
    const waiter = this.waiter;
    this.waiter = null;
    waiter?.reject(new DaemonClientError('eof'));
  }

  pull(): Promise<Uint8Array> {
    const chunk = this.chunks.shift();
    if (chunk !== undefined) return Promise.resolve(chunk);
    if (this.finished) return Promise.reject(new DaemonClientError('eof'));
    return new Promise((resolve, reject) => {
      this.waiter = { resolve, reject };
    });
  }
}

class InMemoryByteStream implements DaemonByteStream {
  private constructor(
    private readonly inbound: InMemoryPipe,
    private readonly outbound: InMemoryPipe,
  ) {}

  static pair(): [InMemoryByteStream, InMemoryByteStream] {
    const a = new InMemoryPipe();
    const b = new InMemoryPipe();
    return [new InMemoryByteStream(a, b), new InMemoryByteStream(b, a)];
  }

  send(data: Uint8Array): Promise<void> {
    this.outbound.push(data);
    return Promise.resolve();
  }

  receive(): Promise<Uint8Array> {
    return this.inbound.pull();
  }

  close(): void {
    this.inbound.finish();
    this.outbound.finish();
  }
}

function inMemoryTransport(serve: (server: InMemoryByteStream) => Promise<void>): DaemonTransport {
  return {
    openStream: () => {
      const [client, server] = InMemoryByteStream.pair();
      void serve(server).catch(() => {});
      return Promise.resolve(client);
    },
  };
}

/**
 * Answers one `health` request with a health report split across chunks, as a socket may deliver
 * it, followed by bytes past the newline that must be ignored.
 */
async function healthDaemon(server: InMemoryByteStream): Promise<void> {
  const request = JSON.parse(text(await new DaemonLineReader(server).readLine())) as {
    type?: string;
  };
  if (request.type !== 'health') {
    await server.send(utf8('{"type":"error","code":"BAD","message":"unexpected"}\n'));
    return;
  }
  await server.send(utf8('{"type":"health_report","pid":42,'));
  await server.send(utf8('"uptime_seconds":7,"protocol_version":1,"agent_count":2}\n'));
  await server.send(utf8('trailing'));
}

function expectHealth(response: ParsedResponse): void {
  const health = expectType(response, 'health_report');
  expect(health.pid).toBe(42);
  expect(health.uptime_seconds).toBe(7);
  expect(health.protocol_version).toBe(1);
  expect(health.agent_count).toBe(2);
}

describe('DaemonTransportTests', () => {
  it('sendRoundTripsRequestAndResponseOverTransport', async () => {
    const client = new DaemonClient(inMemoryTransport(healthDaemon));
    expectHealth(await client.send({ type: 'health' }));
  });

  // JS has no blocking send; Swift's `sendBlocking` bridges the same `send` onto a thread.
  it('sendBlockingRoundTripsOverBridgedTransport (as send)', async () => {
    const client = new DaemonClient(inMemoryTransport(healthDaemon));
    expectHealth(await client.send({ type: 'health' }, 5000));
  });

  it('sendBlockingTimesOutWhenDaemonNeverAnswers (as send with a timeout)', async () => {
    const silent = inMemoryTransport(async (server) => {
      await server.receive();
    });
    const client = new DaemonClient(silent);
    await expect(client.send({ type: 'health' }, 50)).rejects.toMatchObject({
      code: 'requestTimeout',
      message: 'Timed out waiting for the daemon',
    });
  });

  it('sendSurfacesEofWhenDaemonClosesStream', async () => {
    const closing = inMemoryTransport(async (server) => {
      await server.receive();
      server.close();
    });
    const client = new DaemonClient(closing);
    await expect(client.send({ type: 'health' })).rejects.toMatchObject({
      code: 'eof',
      message: 'Connection to daemon closed',
    });
  });

  it('lineReaderSplitsAndJoinsChunks', async () => {
    const [client, server] = InMemoryByteStream.pair();
    const reader = new DaemonLineReader(client);
    await server.send(utf8('one\ntw'));
    await server.send(utf8('o\nthree\n'));
    server.close();
    expect(text(await reader.readLine())).toBe('one');
    expect(text(await reader.readLine())).toBe('two');
    expect(text(await reader.readLine())).toBe('three');
    await expect(reader.readLine()).rejects.toMatchObject({ code: 'eof' });
  });

  // `defaultTransportIsUnixSocketAtClientPath`, `unixSendBlockingWithoutListenerIsNotRunning` and
  // `macOSEndpointsResolveToUnixSocketsAtTodaysPaths` are macOS-only. The named-pipe equivalents
  // (`given_no_daemon_should_return_not_running`, …) live in `src-tauri/src/daemon/client.rs`; the
  // long-lived stream cases (grid reconnect, status `daemon_lost`, subscribe error, attach end and
  // stop) in `src-tauri/src/subscriptions/tests.rs` and `src-tauri/src/attach/session_tests.rs`.

  it('injectedProviderResolvesMachineEndpointsWithoutChangingCallSites', async () => {
    // The seam here is the per-project `DaemonRequester`: a machine's projects are handed a client
    // over whatever transport the caller injects, and send through it unchanged.
    const seen: string[] = [];
    const provider = (machineId: string) => {
      seen.push(machineId);
      return new DaemonClient(inMemoryTransport(healthDaemon));
    };
    const machineId = '6f1c2b3a-1111-4222-8333-944455556666';
    const client = provider(machineId);
    expect(client.transport).toBeDefined();
    expect(seen).toEqual([machineId]);
    expectHealth(await client.send({ type: 'health' }));
  });
});

describe('DaemonClientError', () => {
  it('keeps the Swift error descriptions', () => {
    expect(new DaemonClientError('busy').message).toBe('Daemon is at its connection limit');
    expect(new DaemonClientError('notRunning').message).toBe('Daemon is not running');
    expect(new DaemonClientError('connectTimeout').message).toBe('Timed out connecting to daemon');
    expect(new DaemonClientError('cancelled').message).toBe('Connection cancelled');
  });
});
