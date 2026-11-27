// Property test for the workspace reconciler (WIN-6.1): for random manifests and stored layouts,
// every live agent occupies exactly one tab of one pane of one workspace, and reconciling the
// result again changes nothing. Inputs come from a small seeded generator, so a failure names the
// seed that reproduces it.

import {
  PROJECT_ROOT,
  adoptingWorkspace,
  agentContent,
  closeTab,
  contentAgentId,
  fileContent,
  liveAgent,
  makeWorkspace,
  moveTab,
  newTab,
  normalizeWorkspace,
  pointGuardWorkspace,
  reconcileWorkspaces,
  selectTab,
  splitPane,
  workspaceAgentIds,
  workspaceSurfaces,
  worktreeContainer,
  type LiveAgent,
  type SurfaceContent,
  type Workspace,
  type WorkspaceContainer,
} from './workspace';
import { allLeafIds, leaf } from './paneSplit';

const ITERATIONS = 2000;
const BASE_SEED = 0x5eed_6101;

/** mulberry32: a tiny, fast, seedable PRNG. */
function rng(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

class Gen {
  private readonly next: () => number;
  constructor(seed: number) {
    this.next = rng(seed);
  }
  int(maxExclusive: number): number {
    return Math.floor(this.next() * maxExclusive);
  }
  chance(p: number): boolean {
    return this.next() < p;
  }
  pick<T>(items: readonly T[]): T {
    const item = items[this.int(items.length)];
    if (item === undefined) throw new Error('pick from an empty list');
    return item;
  }
  shuffle<T>(items: readonly T[]): T[] {
    const copy = [...items];
    for (let i = copy.length - 1; i > 0; i--) {
      const j = this.int(i + 1);
      [copy[i], copy[j]] = [copy[j] as T, copy[i] as T];
    }
    return copy;
  }
}

const AGENT_POOL = Array.from({ length: 10 }, (_, i) => `ag-${String(i)}`);
const CONTAINERS: WorkspaceContainer[] = [
  PROJECT_ROOT,
  PROJECT_ROOT,
  worktreeContainer('wt-0'),
  worktreeContainer('wt-1'),
];

function randomContent(gen: Gen): SurfaceContent {
  const roll = gen.int(10);
  if (roll < 6) return agentContent(gen.pick(AGENT_POOL));
  if (roll < 8) return { kind: 'empty' };
  return fileContent(gen.chance(0.5) ? '/tmp/notes.md' : null);
}

function randomWorkspace(gen: Gen, index: number): Workspace {
  const roll = gen.int(10);
  let workspace: Workspace;
  if (roll < 5) {
    workspace = adoptingWorkspace(gen.pick(AGENT_POOL), gen.pick(CONTAINERS));
  } else if (roll < 6) {
    workspace = pointGuardWorkspace('/p');
  } else {
    workspace = makeWorkspace({
      id: `ws-custom-${String(index)}`,
      container: gen.pick(CONTAINERS),
      root: leaf(0),
      focusedLeafId: 0,
      nextLeafId: 1,
    });
  }

  const ops = gen.int(8);
  for (let op = 0; op < ops; op++) {
    const leaves = allLeafIds(workspace.root);
    const surfaces = workspaceSurfaces(workspace);
    switch (gen.int(5)) {
      case 0:
        workspace = splitPane(
          workspace,
          gen.pick(leaves),
          gen.chance(0.5) ? 'vertical' : 'horizontal',
          randomContent(gen),
        ).workspace;
        break;
      case 1:
        workspace = newTab(workspace, gen.pick(leaves), randomContent(gen)).workspace;
        break;
      case 2:
        // Closing the last tab leaves no pane: such a layout is never stored, so skip it.
        if (surfaces.length > 1)
          workspace = closeTab(workspace, gen.pick(surfaces).surface.id).workspace;
        break;
      case 3:
        if (surfaces.length > 0) {
          workspace = moveTab(
            workspace,
            gen.pick(surfaces).surface.id,
            gen.pick(leaves),
            gen.chance(0.5) ? gen.int(4) : null,
          );
        }
        break;
      default:
        if (surfaces.length > 0) workspace = selectTab(workspace, gen.pick(surfaces).surface.id);
    }
  }
  // A stored layout is decoded through Swift's `Workspace.init`, which normalizes.
  return normalizeWorkspace(workspace);
}

interface Scenario {
  readonly stored: Workspace[];
  readonly live: LiveAgent[];
  readonly reserved: Set<string>;
}

function randomScenario(seed: number): Scenario {
  const gen = new Gen(seed);
  const stored: Workspace[] = [];
  const count = gen.int(6);
  for (let i = 0; i < count; i++) {
    stored.push(randomWorkspace(gen, i));
    // A duplicated workspace (hand-edited or corrupt file).
    if (gen.chance(0.1)) stored.push(randomWorkspace(gen, i));
  }

  const workspaceIds = [...stored.map((workspace) => workspace.id), 'ws-gone'];
  const live = gen
    .shuffle(AGENT_POOL)
    .slice(0, gen.int(AGENT_POOL.length + 1))
    .map((id) =>
      liveAgent(
        id,
        gen.pick(CONTAINERS),
        gen.chance(0.3) ? { workspace: gen.pick(workspaceIds), surface: gen.int(8) } : null,
      ),
    );

  const reserved = new Set(stored.filter(() => gen.chance(0.2)).map((workspace) => workspace.id));
  return { stored, live, reserved };
}

/**
 * A stored workspace whose ID is `ws-<agentId>` but which no longer holds that live agent: the
 * agent is adopted under a suffixed ID (WIN-6.2). Counted so the generator provably covers it.
 */
function hasAdoptionIdCollision({ stored, live }: Scenario): boolean {
  const storedIds = new Set(stored.map((workspace) => workspace.id));
  const firstHolder = new Map<string, string>();
  const seen = new Set<string>();
  for (const workspace of stored) {
    // The reconciler skips a repeated workspace ID, so only the first copy can hold an agent.
    if (seen.has(workspace.id)) continue;
    seen.add(workspace.id);
    for (const agentId of workspaceAgentIds(workspace)) {
      if (!firstHolder.has(agentId)) firstHolder.set(agentId, workspace.id);
    }
  }
  return live.some(
    (agent) => storedIds.has(`ws-${agent.id}`) && firstHolder.get(agent.id) !== `ws-${agent.id}`,
  );
}

function expectEveryAgentInExactlyOneTab(
  result: Workspace[],
  live: LiveAgent[],
  seed: number,
): void {
  const liveIds = new Set(live.map((agent) => agent.id));
  const occurrences = new Map<string, number>();
  for (const workspace of result) {
    // Every pane is a leaf of the tree and the reverse, so a tab is in exactly one pane.
    expect(
      Object.keys(workspace.panes)
        .map(Number)
        .sort((a, b) => a - b),
      `seed ${String(seed)}: panes match leaves`,
    ).toEqual(allLeafIds(workspace.root).sort((a, b) => a - b));
    for (const { surface } of workspaceSurfaces(workspace)) {
      const agentId = contentAgentId(surface.content);
      if (agentId === null) continue;
      expect(liveIds.has(agentId), `seed ${String(seed)}: tab holds dead agent ${agentId}`).toBe(
        true,
      );
      occurrences.set(agentId, (occurrences.get(agentId) ?? 0) + 1);
    }
  }
  for (const agent of live) {
    expect(occurrences.get(agent.id), `seed ${String(seed)}: ${agent.id} tab count`).toBe(1);
  }
  expect(
    new Set(result.map((workspace) => workspace.id)).size,
    `seed ${String(seed)}: duplicate workspace ids`,
  ).toBe(result.length);
}

describe('reconcileWorkspaces properties', () => {
  it(`places every live agent in exactly one tab and is idempotent (${String(ITERATIONS)} seeds)`, () => {
    let collisions = 0;
    for (let i = 0; i < ITERATIONS; i++) {
      const seed = BASE_SEED + i;
      const scenario = randomScenario(seed);
      if (hasAdoptionIdCollision(scenario)) collisions += 1;
      const { stored, live, reserved } = scenario;

      const once = reconcileWorkspaces(stored, live, reserved);
      expectEveryAgentInExactlyOneTab(once, live, seed);
      expect(
        reconcileWorkspaces(once, live, reserved),
        `seed ${String(seed)}: not idempotent`,
      ).toEqual(once);
      expect(
        reconcileWorkspaces(stored, live, reserved),
        `seed ${String(seed)}: not deterministic`,
      ).toEqual(once);
    }
    // The generator must actually produce adoption-ID collisions, which are no longer skipped.
    expect(collisions).toBeGreaterThan(0);
  });

  it('givenAdoptedAgentWhoseWorkspaceIdIsTakenShouldChooseAUniqueId (WIN-6.2 regression)', () => {
    // ws-ag-a now holds only ag-b; ag-a is live but in no tab, so it is adopted. Swift adopts it
    // as ws-ag-a too, publishing two workspaces with one ID.
    let workspace = adoptingWorkspace('ag-a', PROJECT_ROOT);
    workspace = newTab(workspace, 0, agentContent('ag-b')).workspace;
    workspace = closeTab(workspace, 0).workspace;
    const taken = { ...adoptingWorkspace('ag-z', PROJECT_ROOT), id: 'ws-ag-a-2' };
    const live = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-b', PROJECT_ROOT),
      liveAgent('ag-z', PROJECT_ROOT),
    ];

    const result = reconcileWorkspaces([workspace, taken], live);

    expect(result.map((w) => w.id)).toEqual(['ws-ag-a', 'ws-ag-a-2', 'ws-ag-a-3']);
    expect(workspaceAgentIds(result[2] ?? workspace)).toEqual(['ag-a']);
    // Stable: the adopted workspace now holds its agent, so it keeps its ID.
    expect(reconcileWorkspaces(result, live)).toEqual(result);
    expect(reconcileWorkspaces([workspace, taken], live)).toEqual(result);
  });
});
