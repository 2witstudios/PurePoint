// Terminal byte accounting for the end-to-end tests (WIN-7.3): how many output bytes each pane
// received and xterm.js parsed, and an FNV-1a checksum of them. The checksum costs a pass over
// every byte, so it runs only while `globalThis.__PU_TERMINAL_STATS__` is true (the e2e sets it),
// and only in debug builds (`__PU_TEST_HOOKS__`).

import { testHooksEnabled } from '../../services/testHooks';

export const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

/** 32-bit FNV-1a over `bytes`, continuing from `hash`. */
export function fnv1a(hash: number, bytes: Uint8Array): number {
  let h = hash >>> 0;
  for (const byte of bytes) {
    h ^= byte;
    h = Math.imul(h, FNV_PRIME);
  }
  return h >>> 0;
}

interface StatsGlobal {
  __PU_TERMINAL_STATS__?: boolean;
}

export function statsEnabled(): boolean {
  return testHooksEnabled() && (globalThis as StatsGlobal).__PU_TERMINAL_STATS__ === true;
}
