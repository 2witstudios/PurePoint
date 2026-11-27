import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useConversationListReload } from './useConversationListReload.ts';

interface Props {
  connected: boolean;
  busy: boolean;
  sessionId: string | null;
}

function setup(initial: Props) {
  const load = vi.fn();
  const hook = renderHook(
    (props: Props) => {
      useConversationListReload(load, props.connected, props.busy, props.sessionId);
    },
    { initialProps: initial },
  );
  return { load, rerender: hook.rerender };
}

describe('useConversationListReload', () => {
  it('given a run starting, should not reload; given it finishing, should reload once', () => {
    const { load, rerender } = setup({ connected: true, busy: false, sessionId: 's' });
    expect(load).toHaveBeenCalledTimes(1);
    rerender({ connected: true, busy: true, sessionId: 's' });
    expect(load).toHaveBeenCalledTimes(1);
    rerender({ connected: true, busy: false, sessionId: 's' });
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('given connecting or a new session, should reload; while disconnected, should not', () => {
    const { load, rerender } = setup({ connected: false, busy: false, sessionId: null });
    expect(load).not.toHaveBeenCalled();
    rerender({ connected: true, busy: false, sessionId: null });
    expect(load).toHaveBeenCalledTimes(1);
    rerender({ connected: true, busy: false, sessionId: 'next' });
    expect(load).toHaveBeenCalledTimes(2);
    rerender({ connected: false, busy: true, sessionId: 'next' });
    rerender({ connected: false, busy: false, sessionId: 'next' });
    expect(load).toHaveBeenCalledTimes(2);
  });
});
