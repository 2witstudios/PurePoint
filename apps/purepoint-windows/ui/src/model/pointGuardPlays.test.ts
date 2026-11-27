// Port of apps/purepoint-macos/purepoint-macosTests/PointGuardPlaysTests.swift (WIN-8.13): resolving
// the court's plays, persisting slot assignments, and the court geometry.
import { describe, expect, it } from 'vitest';
import {
  BUILT_IN_PLAYS,
  CourtGeometry,
  PLAYBOOK_KEY,
  PointGuardPlaybook,
  courtLayout,
  courtPositions,
  facesLeft,
  isCustomPlay,
  logoTilt,
  nearestSlot,
  portraitPositions,
  resolvePlays,
  type PointGuardPrompt,
} from './pointGuardPlays.ts';

const review: PointGuardPrompt = { name: 'review', body: 'Review my open PRs.', needsInput: false };
const branch: PointGuardPrompt = { name: 'branch', body: 'Review {{BRANCH}}.', needsInput: true };

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

describe('resolving plays', () => {
  it('plays the built-ins in unassigned slots', () => {
    const plays = resolvePlays([], [review]);
    expect(plays).toEqual(BUILT_IN_PLAYS);
    expect(plays.every((play) => play.prompt !== '' && !play.prompt.includes('['))).toBe(true);
  });

  it('plays the saved prompt in an assigned slot', () => {
    const plays = resolvePlays([{ slot: 2, name: 'review' }], [review]);
    expect(plays[2]?.title).toBe('review');
    expect(plays[2]?.prompt).toBe('Review my open PRs.');
    expect(plays[2]?.source).toEqual({ kind: 'saved', name: 'review' });
    expect(plays[2] !== undefined && isCustomPlay(plays[2])).toBe(true);
    expect(plays[0]).toEqual(BUILT_IN_PLAYS[0]);
  });

  it('falls back to the built-in for a deleted or input-needing prompt', () => {
    const assignments = [
      { slot: 0, name: 'gone' },
      { slot: 1, name: 'branch' },
    ];
    const plays = resolvePlays(assignments, [review, branch]);
    expect(plays[0]).toEqual(BUILT_IN_PLAYS[0]);
    expect(plays[1]).toEqual(BUILT_IN_PLAYS[1]);
  });

  it('keeps assigned titles until the prompts load', () => {
    const plays = resolvePlays([{ slot: 1, name: 'review' }], null);
    expect(plays[1]?.title).toBe('review');
    expect(plays[1]?.source).toEqual({ kind: 'saved', name: 'review' });
    expect(plays[1]?.prompt).toBe('');
    expect(plays[0]).toEqual(BUILT_IN_PLAYS[0]);
  });
});

describe('PointGuardPlaybook', () => {
  it('persists assignments across playbooks', () => {
    const storage = memoryStorage();
    const playbook = new PointGuardPlaybook(storage);
    playbook.assign(3, review);
    playbook.assign(3, { name: 'standup', body: 'Standup notes.', needsInput: false });
    expect(new PointGuardPlaybook(storage).assignments).toEqual([{ slot: 3, name: 'standup' }]);

    playbook.reset(3);
    expect(new PointGuardPlaybook(storage).assignments).toEqual([]);
    expect(storage.map.get(PLAYBOOK_KEY)).toBe('[]');
  });

  it('never assigns prompts needing input or unknown slots', () => {
    const playbook = new PointGuardPlaybook(memoryStorage());
    playbook.assign(0, branch);
    playbook.assign(4, review);
    expect(playbook.assignments).toEqual([]);
  });

  it('ignores a corrupt saved list', () => {
    const storage = memoryStorage();
    storage.setItem(PLAYBOOK_KEY, '{"slot":1}');
    expect(new PointGuardPlaybook(storage).assignments).toEqual([]);
  });
});

describe('court geometry', () => {
  it('keeps the wings beside a wide composer', () => {
    const size = { width: 1440, height: 800 };
    const positions = courtPositions(size, 720, 560);
    expect(positions[0]).toEqual({ x: 1440 * 0.16, y: 800 * 0.61 });
    expect(positions[1]?.y).toBe(positions[0]?.y);
  });

  it('drops the wings below a narrow composer', () => {
    const positions = courtPositions({ width: 900, height: 800 }, 720, 560);
    expect(positions[0]?.y).toBe(608);
    expect(positions[1]?.y).toBe(608);
    expect(positions[2]?.y).toBe(800 * 0.84);
  });

  it('drops the wings as soon as the column reaches their labels', () => {
    const positions = courtPositions({ width: 1200, height: 800 }, 720, 560);
    expect(positions[0]?.y).toBe(608);
    expect(positions[1]?.y).toBe(608);
  });

  it('keeps the low blocks below the composer in a short pane', () => {
    const positions = courtPositions({ width: 1000, height: 550 }, 720, 446);
    expect(positions.every((point) => point.y === 494)).toBe(true);
  });

  it('never lets a play leave the bottom of the pane', () => {
    const positions = courtPositions({ width: 1000, height: 550 }, 720, 500);
    expect(positions.every((point) => point.y === 510)).toBe(true);
  });

  it('lights only a play within reach', () => {
    const positions = [
      { x: 100, y: 100 },
      { x: 400, y: 100 },
    ];
    expect(nearestSlot({ x: 380, y: 120 }, positions)).toBe(1);
    expect(nearestSlot({ x: 250, y: 400 }, positions)).toBeNull();
  });

  it('clamps the logo tilt', () => {
    const size = { width: 1000, height: 800 };
    expect(logoTilt({ x: 500, y: 0 }, size)).toBe(0);
    expect(logoTilt({ x: 1000, y: 0 }, size)).toBe(8);
    expect(logoTilt({ x: 0, y: 0 }, size)).toBe(-8);
  });

  it('faces a target clearly to one side', () => {
    expect(facesLeft({ x: 100, y: 400 }, 500, false)).toBe(true);
    expect(facesLeft({ x: 900, y: 400 }, 500, true)).toBe(false);
  });

  it('keeps its facing near the centre or without a target', () => {
    expect(facesLeft({ x: 510, y: 400 }, 500, true)).toBe(true);
    expect(facesLeft({ x: 490, y: 400 }, 500, false)).toBe(false);
    expect(facesLeft(null, 500, true)).toBe(true);
  });

  it('turns around when the logo moves past a still target', () => {
    const pointer = { x: 400, y: 300 };
    expect(facesLeft(pointer, 500, false)).toBe(true);
    expect(facesLeft(pointer, 300, true)).toBe(false);
  });

  it('runs the portrait court only under a docked composer', () => {
    const portrait = { width: 500, height: 900 };
    expect(courtLayout(portrait, false, true)).toBe('portrait');
    expect(courtLayout(portrait, false, false)).toBe('stacked');
  });

  it('keeps the wide court in a wide pane', () => {
    expect(courtLayout({ width: 874, height: 560 }, false, true)).toBe('wide');
    expect(courtLayout({ width: 900, height: 700 }, false, false)).toBe('wide');
  });

  it('never stacks the plays under a docked composer', () => {
    const sizes = [
      { width: 320, height: 460 },
      { width: 1000, height: 300 },
      { width: 500, height: 400 },
      { width: 500, height: 900 },
    ];
    for (const size of sizes) {
      expect(courtLayout(size, false, true)).not.toBe('stacked');
      expect(courtLayout(size, true, true)).not.toBe('stacked');
    }
    expect(courtLayout({ width: 500, height: 400 }, false, true)).toBe('portrait');
  });

  it('stacks small and compact desktop panes', () => {
    expect(courtLayout({ width: 320, height: 460 }, false, false)).toBe('stacked');
    expect(courtLayout({ width: 1000, height: 900 }, true, false)).toBe('stacked');
  });

  it('lets a dragged ball reach only a nearby play', () => {
    const positions = [
      { x: 100, y: 100 },
      { x: 400, y: 100 },
    ];
    const reach = CourtGeometry.touchReach;
    expect(nearestSlot({ x: 380, y: 120 }, positions, reach)).toBe(1);
    expect(nearestSlot({ x: 250, y: 100 }, positions, reach)).toBeNull();
  });

  it('sits portrait plays on their anchors under the heading', () => {
    expect(portraitPositions({ width: 500, height: 900 }, 300)).toEqual([
      { x: 105, y: 603 },
      { x: 395, y: 603 },
      { x: 165, y: 774 },
      { x: 335, y: 774 },
    ]);
  });

  it('keeps portrait plays inside a short narrow pane', () => {
    const size = { width: 360, height: 520 };
    const positions = portraitPositions(size, 340);
    expect(positions[0]).toEqual({ x: 78, y: 388 });
    expect(positions[1]).toEqual({ x: 282, y: 388 });
    expect(positions[2]?.y).toBe(480);
    const [, , left, right] = positions;
    expect((right?.x ?? 0) - (left?.x ?? 0)).toBeGreaterThanOrEqual(
      2 * CourtGeometry.labelHalfWidth,
    );
    const half = CourtGeometry.labelHalfWidth;
    expect(positions.every((point) => point.x - half >= 0)).toBe(true);
    expect(positions.every((point) => point.x + half <= size.width)).toBe(true);
  });
});
