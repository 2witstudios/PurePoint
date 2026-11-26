/** The commit the app was built from (vite.config.ts `define`); `dev` outside a git checkout. */
declare const __PUREPOINT_BUILD__: string;

/**
 * The end-to-end test hooks (`__PU_OPENER_SPY__`, `__PU_DIALOG_SPY__`, `__PU_TERMINAL_STATS__`)
 * are read: true in debug builds (`tauri build --debug`, which the e2e run uses), the dev server
 * and Vitest; false in release builds, where the hooks are compiled out (vite.config.ts `define`).
 */
declare const __PU_TEST_HOOKS__: boolean;
