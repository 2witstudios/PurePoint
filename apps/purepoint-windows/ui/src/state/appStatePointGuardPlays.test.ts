// The Point Guard home folder lookup: a known home is read once, a failed read is retried.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const { invoke } = await import('@tauri-apps/api/core');
const mockedInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.resetModules();
  mockedInvoke.mockReset();
});

async function freshRoot() {
  const module = await import('./appStatePointGuardPlays.ts');
  return module.pointGuardPromptRoot;
}

describe('pointGuardPromptRoot', () => {
  it('given a known home, should read it once', async () => {
    mockedInvoke.mockResolvedValue('C:\\Users\\me');
    const root = await freshRoot();
    await expect(root()).resolves.toBe('C:\\Users\\me');
    await expect(root()).resolves.toBe('C:\\Users\\me');
    expect(mockedInvoke).toHaveBeenCalledTimes(1);
  });

  it('given a failed read, should ask again on the next call', async () => {
    mockedInvoke.mockRejectedValueOnce(new Error('backend down'));
    mockedInvoke.mockResolvedValueOnce(null);
    mockedInvoke.mockResolvedValueOnce('C:\\Users\\me');
    const root = await freshRoot();
    await expect(root()).rejects.toThrow('backend down');
    await expect(root()).rejects.toThrow('The home folder is unknown.');
    await expect(root()).resolves.toBe('C:\\Users\\me');
    await expect(root()).resolves.toBe('C:\\Users\\me');
    expect(mockedInvoke).toHaveBeenCalledTimes(3);
  });
});
