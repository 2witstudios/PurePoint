// Port of apps/purepoint-macos/purepoint-macosTests/CommandPaletteOpenCoordinatorTests.swift
// (WIN-6.6). Each title keeps the Swift test function name; the Swift AsyncStreams are
// one-shot promises here.
import { describe, expect, it } from 'vitest';
import { CommandPaletteOpenCoordinator } from './commandPaletteOpenCoordinator';

function signal(): { wait: Promise<void>; fire: () => void } {
  let fire = (): void => undefined;
  const wait = new Promise<void>((resolve) => {
    fire = resolve;
  });
  return { wait, fire };
}

describe('CommandPaletteOpenCoordinatorTests', () => {
  it('sameTurnRequestsOnlyOpenOnce', async () => {
    const coordinator = new CommandPaletteOpenCoordinator();
    let opens = 0;
    const first = coordinator.request(() => {
      opens += 1;
    });
    expect(first).not.toBeNull();
    const duplicate = coordinator.request(() => {
      opens += 1;
    });
    expect(duplicate).toBeNull();
    await first;
    expect(opens).toBe(1);
  });

  it('requestsDuringLoadingCoalesceAndLaterRequestsCanOpen', async () => {
    const coordinator = new CommandPaletteOpenCoordinator();
    const started = signal();
    const resume = signal();
    let loads = 0;
    let opens = 0;

    const first = coordinator.request(async () => {
      loads += 1;
      started.fire();
      await resume.wait;
      opens += 1;
    });
    expect(first).not.toBeNull();
    await started.wait;

    // Different entry points use this same coordinator; retries must not load or toggle.
    for (let i = 0; i < 3; i += 1) {
      const retry = coordinator.request(() => {
        loads += 1;
        opens += 1;
      });
      expect(retry).toBeNull();
    }
    expect(loads).toBe(1);
    expect(opens).toBe(0);

    resume.fire();
    await first;
    expect(opens).toBe(1);

    const next = coordinator.request(() => {
      opens += 1;
    });
    expect(next).not.toBeNull();
    await next;
    expect(opens).toBe(2);
  });
});
