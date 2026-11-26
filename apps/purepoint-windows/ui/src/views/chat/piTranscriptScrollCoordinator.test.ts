// PiTranscriptScrollCoordinator (WIN-9.5): deferred, deduplicated and bounded scroll requests.
import { describe, expect, it } from 'vitest';
import { PiTranscriptScrollCoordinator } from './piTranscriptScrollCoordinator.ts';

function setup() {
  const queue: (() => void)[] = [];
  const coordinator = new PiTranscriptScrollCoordinator((run) => queue.push(run));
  let scrolls = 0;
  const request = (can = true) => {
    coordinator.request(
      () => can,
      () => {
        scrolls++;
      },
    );
  };
  const flush = () => {
    for (const run of queue.splice(0)) run();
  };
  return { coordinator, request, flush, scrolls: () => scrolls };
}

describe('PiTranscriptScrollCoordinator', () => {
  it('defers one scroll and drops requests while one is pending', () => {
    const { coordinator, request, flush, scrolls } = setup();
    coordinator.update({ contentHeight: 1000, viewportHeight: 400, visibleBottom: 400 });
    request();
    request();
    expect(scrolls()).toBe(0);
    flush();
    expect(scrolls()).toBe(1);
  });

  it('tries each size once until the output changes', () => {
    const { coordinator, request, flush, scrolls } = setup();
    coordinator.update({ contentHeight: 1000, viewportHeight: 400, visibleBottom: 400 });
    request();
    flush();
    request();
    flush();
    expect(scrolls()).toBe(1);
    coordinator.outputChanged();
    request();
    flush();
    expect(scrolls()).toBe(2);
  });

  it('skips when already at the bottom, unwanted, cancelled or without a viewport', () => {
    const { coordinator, request, flush, scrolls } = setup();
    coordinator.update({ contentHeight: 1000, viewportHeight: 400, visibleBottom: 1000 });
    request();
    flush();
    coordinator.update({ contentHeight: 1200, viewportHeight: 400, visibleBottom: 600 });
    request(false);
    flush();
    request();
    coordinator.cancel();
    flush();
    coordinator.update({ contentHeight: 1200, viewportHeight: 0, visibleBottom: 0 });
    request();
    flush();
    expect(scrolls()).toBe(0);
  });

  it('starts over after a disclosure toggle or a reset', () => {
    const { coordinator, request, flush, scrolls } = setup();
    coordinator.update({ contentHeight: 1000, viewportHeight: 400, visibleBottom: 400 });
    request();
    flush();
    coordinator.userLayoutChanged();
    request();
    flush();
    coordinator.reset();
    request();
    flush();
    expect(scrolls()).toBe(3);
  });
});
