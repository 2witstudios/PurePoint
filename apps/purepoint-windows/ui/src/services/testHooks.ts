// Whether this build honours the end-to-end test hooks on `window` (`__PU_OPENER_SPY__`,
// `__PU_DIALOG_SPY__`, `__PU_TERMINAL_STATS__`). `__PU_TEST_HOOKS__` is a build-time constant
// (vite.config.ts `define`): true for debug builds (the e2e app), the dev server and Vitest, false
// for release bundles, where the hooks' code is dropped and a page script cannot use them to
// silence the opener or the folder picker.
export function testHooksEnabled(): boolean {
  return __PU_TEST_HOOKS__;
}
