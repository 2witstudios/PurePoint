import type { GridCommand, KillTarget, Request, Response } from '.';
import { PROTOCOL_VERSION } from '.';
import { requestInputs, requestOutputs, responseOutputs } from './generated/samples';

// `generated/samples.ts` holds JSON pu-core parses and writes; `npm run typecheck` checks it
// against the generated types. The constants below pin the shapes those samples cannot show.

const minimalSpawn: Request = { type: 'spawn', project_root: '/p', prompt: 'go' };

// @ts-expect-error `prompt` has no serde default, so it is required.
const spawnWithoutPrompt: Request = { type: 'spawn', project_root: '/p' };

// @ts-expect-error hex-encoded PTY bytes are a string, not a byte array.
const byteArrayOutput: Response = { type: 'output', agent_id: 'ag-1', data: [104, 105] };

// u64 fields arrive from JSON.parse as numbers, not bigints.
const health: Response = {
  type: 'health_report',
  pid: 1,
  uptime_seconds: 2,
  protocol_version: PROTOCOL_VERSION,
  projects: [],
  agent_count: 0,
};
// @ts-expect-error large integers are typed `number`.
const bigintHealth: Response = { ...health, uptime_seconds: 2n };

// `#[serde(default)]` without `skip_serializing_if` in a response: always sent.
// @ts-expect-error `killed` is always present in `project_removed`.
const removedWithoutKilled: Response = {
  type: 'project_removed',
  project_root: '/p',
  removed: true,
  suspended: [],
};

const killTargets: KillTarget[] = ['all', 'all_worktrees', { agent: 'ag-1' }, { worktree: 'wt-1' }];
// @ts-expect-error a GridCommand is tagged by `action`, not `type`.
const wrongTag: GridCommand = { type: 'get_layout' };

describe('generated protocol types', () => {
  it('match the pu-core protocol version', () => {
    expect(PROTOCOL_VERSION).toBe(14);
  });

  it('carry samples for every direction', () => {
    expect(requestInputs.length).toBe(requestOutputs.length);
    expect(responseOutputs.length).toBeGreaterThan(20);
    for (const message of [...requestInputs, ...requestOutputs, ...responseOutputs]) {
      expect(typeof message.type).toBe('string');
    }
  });

  it('keep hand-written constants alongside the samples', () => {
    expect([
      minimalSpawn,
      spawnWithoutPrompt,
      byteArrayOutput,
      bigintHealth,
      removedWithoutKilled,
      wrongTag,
    ]).toHaveLength(6);
    expect(killTargets).toHaveLength(4);
  });
});
