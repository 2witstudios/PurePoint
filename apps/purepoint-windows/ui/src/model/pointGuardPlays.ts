// Port of apps/purepoint-macos/purepoint-macos/Models/PointGuardPlays.swift (WIN-8.13): the four
// plays on the Point Guard court, which saved `pu` prompt each slot plays (stored per device),
// and the court's geometry (where the logo and plays stand, which play the pointer lights).
//
// Differences from Swift: the playbook is a Zustand store over `localStorage` (the Mac's
// UserDefaults key `PP_pointGuardPlays`). Windows has no docked (iPad) composer, so
// `DOCKS_COMPOSER` is false and the portrait court is kept only for parity with the tests.
import { createStore, type StoreApi } from 'zustand/vanilla';
import type { SfSymbolName } from '../theme/icons.ts';

/**
 * A global `pu` prompt the court can offer. Prompts with `{{VAR}}` placeholders need input before
 * they can be sent, so a play can't hold them.
 */
export interface PointGuardPrompt {
  readonly name: string;
  readonly body: string;
  readonly needsInput: boolean;
}

export type PointGuardPlaySource =
  { readonly kind: 'builtIn' } | { readonly kind: 'saved'; readonly name: string };

/** One of the four positions on the court. Clicking it sends `prompt` as-is. */
export interface PointGuardPlay {
  readonly slot: number;
  readonly title: string;
  /** Empty for a saved prompt whose body hasn't been fetched yet. */
  readonly prompt: string;
  readonly symbol: SfSymbolName;
  readonly source: PointGuardPlaySource;
}

export const isCustomPlay = (play: PointGuardPlay): boolean => play.source.kind !== 'builtIn';

const BUILT_IN: PointGuardPlaySource = { kind: 'builtIn' };

/** Slot order: left wing, right wing, left block, right block. */
export const BUILT_IN_PLAYS: readonly PointGuardPlay[] = [
  {
    slot: 0,
    title: 'Catch me up',
    prompt:
      'Give me an update on all my projects — what moved, what’s blocked, and what needs me next.',
    symbol: 'square.stack.3d.up',
    source: BUILT_IN,
  },
  {
    slot: 1,
    title: 'What’s next?',
    prompt: 'Look across my projects and tell me what I should work on next, and why.',
    symbol: 'safari',
    source: BUILT_IN,
  },
  {
    slot: 2,
    title: 'Tidy my desktop',
    prompt:
      'Organize my desktop — group the files into folders that make sense and clear the clutter. Show me the plan before you move anything.',
    symbol: 'square.grid.2x2',
    source: BUILT_IN,
  },
  {
    slot: 3,
    title: 'What’s running?',
    prompt:
      'What’s running on my machine right now? Agents, apps, and anything using a lot of memory or CPU.',
    symbol: 'waveform.path.ecg',
    source: BUILT_IN,
  },
];

export interface PointGuardPlayAssignment {
  readonly slot: number;
  readonly name: string;
}

function savedPlay(slot: number, name: string, body: string): PointGuardPlay {
  return {
    slot,
    title: name,
    prompt: body,
    symbol: 'text.bubble',
    source: { kind: 'saved', name },
  };
}

/**
 * Fills each slot from its assigned prompt. A slot whose prompt was deleted, or now needs input,
 * quietly plays its built-in again. `prompts` is null until the global list has loaded: assigned
 * slots keep their saved titles meanwhile, and their bodies are fetched when played.
 */
export function resolvePlays(
  assignments: readonly PointGuardPlayAssignment[],
  prompts: readonly PointGuardPrompt[] | null,
): PointGuardPlay[] {
  return BUILT_IN_PLAYS.map((builtIn) => {
    const assignment = assignments.findLast((candidate) => candidate.slot === builtIn.slot);
    if (assignment === undefined) return builtIn;
    if (prompts === null) return savedPlay(builtIn.slot, assignment.name, '');
    const prompt = prompts.find((candidate) => candidate.name === assignment.name);
    if (prompt === undefined || prompt.needsInput) return builtIn;
    return savedPlay(builtIn.slot, prompt.name, prompt.body);
  });
}

// MARK: - PointGuardPlaybook

export const PLAYBOOK_KEY = 'PP_pointGuardPlays';

/** Key/value storage (UserDefaults); `localStorage` in the app. */
export interface PlaybookStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface PlaybookFields {
  readonly assignments: readonly PointGuardPlayAssignment[];
  /** The global prompts on offer; null until they have loaded. */
  readonly prompts: readonly PointGuardPrompt[] | null;
}

function decodeAssignments(text: string | null): PointGuardPlayAssignment[] {
  if (text === null) return [];
  try {
    const value: unknown = JSON.parse(text);
    if (!Array.isArray(value)) return [];
    const out: PointGuardPlayAssignment[] = [];
    for (const item of value as unknown[]) {
      if (typeof item !== 'object' || item === null) return [];
      const { slot, name } = item as { slot?: unknown; name?: unknown };
      if (typeof slot !== 'number' || !Number.isInteger(slot) || typeof name !== 'string')
        return [];
      out.push({ slot, name });
    }
    return out;
  } catch {
    return [];
  }
}

function defaultStorage(): PlaybookStorage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** Which saved prompt each court slot plays. Stored per device (APP-018). */
export class PointGuardPlaybook {
  readonly store: StoreApi<PlaybookFields>;
  private readonly storage: PlaybookStorage | null;

  constructor(storage: PlaybookStorage | null = defaultStorage()) {
    this.storage = storage;
    let saved: string | null = null;
    try {
      saved = storage?.getItem(PLAYBOOK_KEY) ?? null;
    } catch {
      saved = null;
    }
    this.store = createStore<PlaybookFields>()(() => ({
      assignments: decodeAssignments(saved),
      prompts: null,
    }));
  }

  get assignments(): readonly PointGuardPlayAssignment[] {
    return this.store.getState().assignments;
  }

  get prompts(): readonly PointGuardPrompt[] | null {
    return this.store.getState().prompts;
  }

  set prompts(value: readonly PointGuardPrompt[] | null) {
    this.store.setState({ prompts: value });
  }

  get plays(): PointGuardPlay[] {
    return resolvePlays(this.assignments, this.prompts);
  }

  assign(slot: number, prompt: PointGuardPrompt): void {
    if (slot < 0 || slot >= BUILT_IN_PLAYS.length || !Number.isInteger(slot) || prompt.needsInput) {
      return;
    }
    this.save([...this.assignments.filter((a) => a.slot !== slot), { slot, name: prompt.name }]);
  }

  reset(slot: number): void {
    this.save(this.assignments.filter((a) => a.slot !== slot));
  }

  private save(assignments: PointGuardPlayAssignment[]): void {
    this.store.setState({ assignments });
    const sorted = [...assignments].sort((a, b) => a.slot - b.slot);
    try {
      this.storage?.setItem(PLAYBOOK_KEY, JSON.stringify(sorted));
    } catch {
      // Storage unavailable: the assignment lasts for this session.
    }
  }
}

let shared: PointGuardPlaybook | null = null;

/** `PointGuardPlaybook.shared`. */
export function sharedPlaybook(): PointGuardPlaybook {
  shared ??= new PointGuardPlaybook();
  return shared;
}

// MARK: - PointGuardCourtGeometry

export interface Point {
  readonly x: number;
  readonly y: number;
}

export interface Size {
  readonly width: number;
  readonly height: number;
}

export type CourtLayout = 'wide' | 'portrait' | 'stacked';

/** Where things stand on the court, in the court view's own pixels. */
export const CourtGeometry = {
  /** The logo is the point guard, at the top of the key. */
  logo: { x: 0.5, y: 0.3 } as Point,
  /** Wings beside the composer, then the low blocks either side of the lane. */
  anchors: [
    { x: 0.16, y: 0.61 },
    { x: 0.84, y: 0.61 },
    { x: 0.38, y: 0.84 },
    { x: 0.62, y: 0.84 },
  ] as readonly Point[],
  /** How close the pointer must be for a play to light up. */
  reach: 150,
  /** How close a ball dragged off the logo must be to a play to pass to it. */
  touchReach: 72,
  /** Half a play's label width, which must stay clear of the centred column. */
  labelHalfWidth: 70,
  /**
   * Below this size the court can't hold the column and its plays, so they stack instead. The
   * height leaves room for the low blocks under a composer with a two-line heading above it.
   */
  minimumSize: { width: 680, height: 600 } as Size,
  /** The iPad docks the composer under the court; the desktop keeps it in the centred column. */
  docksComposer: false,
  portraitLogo: { x: 0.5, y: 0.28 } as Point,
  portraitAnchors: [
    { x: 0.21, y: 0.67 },
    { x: 0.79, y: 0.67 },
    { x: 0.33, y: 0.86 },
    { x: 0.67, y: 0.86 },
  ] as readonly Point[],
  /** How far either side of the logo a target must be before the logo turns to face it. */
  facingDeadZone: 24,
} as const;

export function logoPoint(size: Size, portrait = false): Point {
  const logo = portrait ? CourtGeometry.portraitLogo : CourtGeometry.logo;
  return { x: size.width * logo.x, y: size.height * logo.y };
}

/**
 * The court for a pane of `size`. A docked composer (iPad) always runs a court: portrait in a tall
 * or narrow pane, else wide. The desktop stacks a pane too small for the wide court, or a compact
 * (sidebar) one.
 */
export function courtLayout(size: Size, compact: boolean, docked: boolean): CourtLayout {
  if (docked) {
    return size.height > size.width || size.width < CourtGeometry.minimumSize.width
      ? 'portrait'
      : 'wide';
  }
  if (compact) return 'stacked';
  return size.width >= CourtGeometry.minimumSize.width &&
    size.height >= CourtGeometry.minimumSize.height
    ? 'wide'
    : 'stacked';
}

/** Every play stays under the logo and heading, inside the pane, its label clear of the sides. */
export function portraitPositions(size: Size, headingBottom: number): Point[] {
  const belowHeading = headingBottom + 48;
  const inset = CourtGeometry.labelHalfWidth + 8;
  return CourtGeometry.portraitAnchors.map((anchor, index) => {
    // The blocks keep a row's gap under the wings, as far as the pane allows.
    const floor = index < 2 ? belowHeading : belowHeading + 96;
    const y = Math.min(Math.max(size.height * anchor.y, floor), size.height - 40);
    let x = size.width * anchor.x;
    if (index >= 2) {
      // The low blocks sit side by side, so a narrow pane spreads them a play's width apart.
      const spread = Math.max(Math.abs(x - size.width / 2), CourtGeometry.labelHalfWidth + 4);
      x = size.width / 2 + (anchor.x < 0.5 ? -spread : spread);
    }
    return { x: Math.min(Math.max(x, inset), size.width - inset), y };
  });
}

/**
 * No play may sit on the composer. Wings beside it drop below it once the centred column would
 * reach their labels, and the low blocks always stay below it, as far as the pane's height allows.
 */
export function courtPositions(size: Size, columnWidth: number, composerBottom: number): Point[] {
  const columnLeft = (size.width - Math.min(columnWidth, size.width - 48)) / 2;
  const first = CourtGeometry.anchors[0] ?? { x: 0, y: 0 };
  const wingsCrowded = size.width * first.x + CourtGeometry.labelHalfWidth > columnLeft - 16;
  // A play is centred on its icon-and-label stack, about 70px tall.
  const belowComposer = composerBottom + 48;
  return CourtGeometry.anchors.map((anchor, index) => {
    const y = size.height * anchor.y;
    const clear =
      index < 2 ? (wingsCrowded ? Math.max(y, belowComposer) : y) : Math.max(y, belowComposer);
    return { x: size.width * anchor.x, y: Math.min(clear, size.height - 40) };
  });
}

export function nearestSlot(
  point: Point,
  positions: readonly Point[],
  reach: number = CourtGeometry.reach,
): number | null {
  let best: number | null = null;
  let bestDistance = Infinity;
  positions.forEach((position, index) => {
    const distance = Math.hypot(position.x - point.x, position.y - point.y);
    if (distance < reach && distance < bestDistance) {
      best = index;
      bestDistance = distance;
    }
  });
  return best;
}

/** The logo leans toward the pointer, never more than 8°. */
export function logoTilt(point: Point, size: Size): number {
  if (size.width <= 0) return 0;
  return Math.max(-8, Math.min(8, ((point.x / size.width) * 100 - 50) * 0.18));
}

/**
 * The logo is drawn facing right. It turns to face a target clearly to one side, and otherwise
 * keeps its facing.
 */
export function facesLeft(target: Point | null, logoX: number, current: boolean): boolean {
  if (target === null || Math.abs(target.x - logoX) <= CourtGeometry.facingDeadZone) return current;
  return target.x < logoX;
}
