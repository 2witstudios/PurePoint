// Front end of the Point Guard pairing secret (WIN-9.7): `src-tauri/src/secrets/pairing.rs`,
// ported from Services/PiPairingSecret.swift. The secret is kept in Windows Credential
// Manager under service `PurePoint.PointGuard.Pairing`, account = the endpoint.
import { invoke } from '@tauri-apps/api/core';

import type { PiSecretStore } from './pointGuardChat';

/** The chat's secret store, backed by Credential Manager. */
export const credentialManagerSecrets: PiSecretStore = {
  read: (endpoint) => invoke<string>('pointguard_pairing_read', { endpoint }),
  save: async (secret, endpoint) => {
    await invoke('pointguard_pairing_save', { endpoint, secret });
  },
};

/** What Settings imports: a scanned or pasted pairing code, or an address and secret. */
export type PairingImport = { code: string } | { endpoint: string; secret: string };

/**
 * Validate and save an imported pairing secret (the Settings leaf calls this). Resolves with
 * the canonical endpoint, which the chat then connects to with `chat.connectSaved(endpoint)`;
 * rejects with the reason to show.
 */
export function importPairing(request: PairingImport): Promise<string> {
  return invoke<string>('pointguard_pairing_import', { request });
}
