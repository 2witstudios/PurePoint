// Runs a block of tests with the process in one IANA time zone (Node re-reads TZ on assignment),
// for the local-time calendar math of the Schedule screen.
import { afterAll, beforeAll, describe } from 'vitest';

/** Node's environment (the app's tsconfig has no Node types). */
const env = (globalThis as unknown as { process: { env: Record<string, string | undefined> } })
  .process.env;

export function inTimeZone(zone: string, body: () => void): void {
  describe(`in ${zone}`, () => {
    let saved = '';
    beforeAll(() => {
      // Deleting TZ does not reset Node's zone, so restore the resolved name instead.
      saved = env.TZ ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
      env.TZ = zone;
    });
    afterAll(() => {
      env.TZ = saved;
    });
    body();
  });
}
