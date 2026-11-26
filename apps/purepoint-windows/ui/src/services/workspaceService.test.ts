// Ports DaemonWorkspaceServiceTests.swift (WIN-6.0), plus the `loadWorkspace` mapping.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const { parseDaemonResponse } = await import('./daemonConnection');
const { DaemonWorkspaceService, DaemonWorkspaceError, manifestPath, workspaceSnapshot } =
  await import('./workspaceService');
type Response = import('../protocol').Response;
type WorktreeEntry = import('../protocol').WorktreeEntry;

/** Decode a worktree array the way a `status_report` carries it. */
function decodeWorktrees(json: string): WorktreeEntry[] {
  const response = parseDaemonResponse(`{"type":"status_report","worktrees":${json},"agents":[]}`);
  if (response.type !== 'status_report') throw new Error(`did not decode: ${response.type}`);
  return response.worktrees;
}

describe('DaemonWorkspaceServiceTests', () => {
  it('testParseWorktreesValid', () => {
    const entries = decodeWorktrees(
      '[{"id":"wt-1","name":"feature-auth","path":"/tmp/project/.pu/worktrees/wt-1","branch":"pu/feature-auth","baseBranch":"release/next","status":"active","agents":{"ag-1":{"id":"ag-1","name":"claude-1","agentType":"claude","status":"running","prompt":"add auth","startedAt":"2026-01-01T00:00:00Z"}},"createdAt":"2026-01-01T00:00:00Z"}]',
    );
    const models = DaemonWorkspaceService.parseWorktrees(entries);
    expect(models).toHaveLength(1);
    expect(models[0]?.id).toBe('wt-1');
    expect(models[0]?.name).toBe('feature-auth');
    expect(models[0]?.branch).toBe('pu/feature-auth');
    expect(models[0]?.baseBranch).toBe('release/next');
    expect(models[0]?.status).toBe('active');
    expect(models[0]?.agents).toHaveLength(1);
    expect(models[0]?.agents[0]?.id).toBe('ag-1');
  });

  it('testParseWorktreesMissingFieldsFailsDecode', () => {
    // Missing required "branch": the entire status_report fails to decode.
    const response = parseDaemonResponse(
      '{"type":"status_report","worktrees":[{"id":"wt-1","name":"test","path":"/tmp","status":"active","agents":{},"createdAt":"2026-01-01T00:00:00Z"}],"agents":[]}',
    );
    expect(response).toEqual({ type: 'unknown', responseType: 'parse_error' });
  });

  it('testParseWorktreesEmpty', () => {
    expect(DaemonWorkspaceService.parseWorktrees([])).toEqual([]);
  });

  it('testParseWorktreesNoAgents', () => {
    const entries = decodeWorktrees(
      '[{"id":"wt-1","name":"empty-wt","path":"/tmp/project/.pu/worktrees/wt-1","branch":"pu/empty","status":"active","agents":{},"createdAt":"2026-01-01T00:00:00Z"}]',
    );
    const models = DaemonWorkspaceService.parseWorktrees(entries);
    expect(models).toHaveLength(1);
    expect(models[0]?.agents).toEqual([]);
  });
});

describe('DaemonWorkspaceService.loadWorkspace', () => {
  const signal = (worktreeId: string | null) =>
    ({
      identity: {
        agent_id: 'ag-1',
        worktree_id: worktreeId,
        launch_id: 'l',
        generation: 1,
        native_session_id: 's',
      },
    }) as never;

  it('sends status and maps root agents and worktrees', async () => {
    const send = vi.fn<(request: unknown) => Promise<Response>>().mockResolvedValue({
      type: 'status_report',
      worktrees: decodeWorktrees(
        '[{"id":"wt-1","name":"w","path":"C:\\\\p\\\\.pu\\\\worktrees\\\\wt-1","branch":"pu/w","status":"active","createdAt":"t","agents":{"b":{"id":"b","name":"","agentType":"codex","status":"running","startedAt":"2","prompt":null},"a":{"id":"a","name":"A","agentType":"claude","status":"running","startedAt":"1","prompt":"p"}}}]',
      ),
      agents: [
        {
          id: 'ag-1',
          name: 'root',
          agent_type: 'claude',
          status: 'running',
          pid: null,
          exit_code: null,
          idle_seconds: null,
          worktree_id: null,
          started_at: 't',
          suspended: true,
          signal: signal('wt-9'),
        },
      ],
    });
    const service = new DaemonWorkspaceService({ send });
    const snapshot = await service.loadWorkspace('C:\\p');
    expect(send).toHaveBeenCalledWith({ type: 'status', project_root: 'C:\\p' });
    expect(snapshot.rootAgents).toHaveLength(1);
    expect(snapshot.rootAgents[0]).toMatchObject({
      id: 'ag-1',
      prompt: '',
      suspended: true,
      sessionId: null,
      // A root agent's signal must name no worktree.
      signal: null,
    });
    // Worktree agents are ordered by start time.
    expect(snapshot.worktrees[0]?.agents.map((agent) => agent.id)).toEqual(['a', 'b']);
  });

  it('keeps a worktree agent signal only when it names that worktree', () => {
    const entries = decodeWorktrees(
      '[{"id":"wt-1","name":"w","path":"/p","branch":"b","status":"active","createdAt":"t","agents":{"x":{"id":"x","name":"x","agentType":"claude","status":"running","startedAt":"1","signal":{"identity":{"agent_id":"x","worktree_id":"wt-2","launch_id":"l","generation":1,"native_session_id":"s"}}}}}]',
    );
    expect(workspaceSnapshot(entries, []).worktrees[0]?.agents[0]?.signal).toBeNull();
  });

  it('throws daemon errors and unexpected responses', async () => {
    const error = new DaemonWorkspaceService({
      send: () => Promise.resolve({ type: 'error', code: 'X', message: 'nope' }),
    });
    await expect(error.loadWorkspace('/p')).rejects.toMatchObject({
      kind: 'daemonError',
      message: 'nope',
    });
    const odd = new DaemonWorkspaceService({ send: () => Promise.resolve({ type: 'ok' }) });
    const rejection = odd.loadWorkspace('/p');
    await expect(rejection).rejects.toBeInstanceOf(DaemonWorkspaceError);
    await expect(rejection).rejects.toMatchObject({
      kind: 'unexpectedResponse',
      message: 'Unexpected response from daemon: {"type":"ok"}',
    });
  });

  it('names the manifest under .pu with the root separator', () => {
    expect(manifestPath('C:\\work\\app')).toBe('C:\\work\\app\\.pu\\manifest.json');
    expect(manifestPath('/home/u/app/')).toBe('/home/u/app/.pu/manifest.json');
    expect(new DaemonWorkspaceService({ send: vi.fn() }).manifestPath('/a')).toBe(
      '/a/.pu/manifest.json',
    );
  });
});
