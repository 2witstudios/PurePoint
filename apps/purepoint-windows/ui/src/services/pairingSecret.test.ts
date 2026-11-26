// Command wiring of the pairing secret (WIN-9.7); the Credential Manager store is tested in
// Rust (`secrets::pairing`), including one live round trip under a test-only service name.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

const { credentialManagerSecrets, importPairing } = await import('./pairingSecret');

describe('pairing secret', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it('given the chat store should read and save by endpoint', async () => {
    invoke.mockResolvedValueOnce('secret');
    expect(await credentialManagerSecrets.read('ws://127.0.0.1:1/v1')).toBe('secret');
    expect(invoke).toHaveBeenLastCalledWith('pointguard_pairing_read', {
      endpoint: 'ws://127.0.0.1:1/v1',
    });
    invoke.mockResolvedValueOnce(null);
    await credentialManagerSecrets.save('s'.repeat(32), 'ws://127.0.0.1:1/v1');
    expect(invoke).toHaveBeenLastCalledWith('pointguard_pairing_save', {
      endpoint: 'ws://127.0.0.1:1/v1',
      secret: 's'.repeat(32),
    });
  });

  it('given a Settings import should pass the code or the typed pair through', async () => {
    invoke.mockResolvedValue('wss://pc.tail1.ts.net/v1');
    expect(await importPairing({ code: '{"type":"pi-mobile-pairing"}' })).toBe(
      'wss://pc.tail1.ts.net/v1',
    );
    expect(invoke).toHaveBeenLastCalledWith('pointguard_pairing_import', {
      request: { code: '{"type":"pi-mobile-pairing"}' },
    });
    await importPairing({ endpoint: 'ws://100.100.1.2:1/v1', secret: 'k' });
    expect(invoke).toHaveBeenLastCalledWith('pointguard_pairing_import', {
      request: { endpoint: 'ws://100.100.1.2:1/v1', secret: 'k' },
    });
  });
});
