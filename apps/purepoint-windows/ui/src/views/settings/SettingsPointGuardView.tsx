// Settings › Point Guard (WIN-8.11, WIN-9.5). The Mac pane is `PointGuardSetupView` (providers,
// model, working folder, phone pairing); given the app's chat, this pane hosts it (WIN-9.5) beside
// what Windows adds:
// - the owned runtime's connection status, with Start / Stop / Restart (state/pointGuardService.ts);
// - importing a pairing (a pasted pairing code, or an address and secret) through the
//   `pointguard_pairing_import` command, which validates and saves it in Credential Manager
//   (services/pairingSecret.ts);
// - the launch command agents start with and whether they skip permission prompts: Claude's All
//   Projects command and its `--dangerously-skip-permissions` argument, the same machine settings
//   the Agents pane edits (`update_global_agent_settings`).
import { useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import type { AgentConfigState } from '../../state/agentConfigState.ts';
import { PointGuardServiceState } from '../../state/pointGuardService.ts';
import { type PairingImport, importPairing } from '../../services/pairingSecret.ts';
import { SettingsCaption, SettingsGroup, SettingsToggle } from './SettingsGroup.tsx';
import type { PiChatModel } from '../../state/piChat.ts';
import { PointGuardSetupView } from '../chat/PointGuardSetupView.tsx';
import '../chat/pointGuard.css';

export const SKIP_PERMISSIONS_FLAG = '--dangerously-skip-permissions';
const POINT_GUARD_AGENT = 'claude';

/** Whether launch args include the skip-permissions flag as a word. */
export function skipsPermissions(args: string): boolean {
  return args.split(/\s+/).includes(SKIP_PERMISSIONS_FLAG);
}

/** Launch args with the skip-permissions flag added (at the front) or removed. */
export function withSkipPermissions(args: string, skip: boolean): string {
  const words = args.split(/\s+/).filter((word) => word !== '' && word !== SKIP_PERMISSIONS_FLAG);
  return (skip ? [SKIP_PERMISSIONS_FLAG, ...words] : words).join(' ');
}

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

type PairingMode = 'code' | 'address';

function PairingImportForm({
  onImport,
}: {
  onImport: (request: PairingImport) => Promise<string>;
}) {
  const [mode, setMode] = useState<PairingMode>('code');
  const [code, setCode] = useState('');
  const [address, setAddress] = useState('');
  const [secret, setSecret] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const ready = mode === 'code' ? code.trim() !== '' : address.trim() !== '' && secret !== '';
  const submit = async () => {
    setBusy(true);
    setResult(null);
    try {
      const endpoint = await onImport(
        mode === 'code' ? { code: code.trim() } : { endpoint: address.trim(), secret },
      );
      setResult({ ok: true, text: `Saved pairing for ${endpoint}.` });
      setCode('');
      setSecret('');
    } catch (error) {
      setResult({ ok: false, text: describeError(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="settings-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (ready && !busy) void submit();
      }}
    >
      <div className="settings-segmented" role="radiogroup" aria-label="Pairing import">
        <button
          type="button"
          role="radio"
          aria-checked={mode === 'code'}
          onClick={() => {
            setMode('code');
          }}
        >
          Pairing code
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={mode === 'address'}
          onClick={() => {
            setMode('address');
          }}
        >
          Address and secret
        </button>
      </div>
      {mode === 'code' ? (
        <textarea
          className="settings-text-field settings-code-field"
          aria-label="Pairing code"
          placeholder='{"type":"pi-mobile-pairing","version":1,…}'
          spellCheck={false}
          value={code}
          onChange={(event) => {
            setCode(event.target.value);
          }}
        />
      ) : (
        <>
          <input
            className="settings-text-field"
            aria-label="Point Guard address"
            placeholder="ws://100.100.1.2:8787/v1"
            spellCheck={false}
            value={address}
            onChange={(event) => {
              setAddress(event.target.value);
            }}
          />
          <input
            className="settings-text-field"
            type="password"
            aria-label="Pairing secret"
            placeholder="Pairing secret"
            autoComplete="off"
            value={secret}
            onChange={(event) => {
              setSecret(event.target.value);
            }}
          />
        </>
      )}
      <div className="settings-row">
        <button type="submit" className="settings-button" disabled={!ready || busy}>
          Import
        </button>
        {result !== null && (
          <span className={result.ok ? 'settings-secondary' : 'settings-error'} role="status">
            {result.text}
          </span>
        )}
      </div>
    </form>
  );
}

export function SettingsPointGuardView({
  config,
  service: injected,
  chat,
  onImport = importPairing,
}: {
  config: AgentConfigState;
  service?: PointGuardServiceState;
  /** The app's chat: with it the pane hosts the setup view (provider, model, folder, phone). */
  chat?: PiChatModel;
  onImport?: (request: PairingImport) => Promise<string>;
}) {
  const service = useMemo(() => injected ?? new PointGuardServiceState(), [injected]);
  const pg = useStore(service.store);
  const agents = useStore(config.store);

  useEffect(() => {
    if (injected !== undefined) return;
    void service.attach().catch((error: unknown) => {
      service.error = describeError(error);
    });
    return () => {
      service.detach();
    };
  }, [service, injected]);

  useEffect(() => {
    void config.loadGlobalSettings();
  }, [config]);

  const status = pg.ready ? 'Connected' : pg.running ? pg.phase : 'Not running';
  const command = agents.machineCommands[POINT_GUARD_AGENT] ?? '';
  const args = agents.machineLaunchArgs[POINT_GUARD_AGENT] ?? '';
  const launchDisabled = !agents.globalSettingsLoaded || agents.isSavingGlobalSettings;
  const saveLaunch = (nextCommand: string, nextArgs: string) =>
    config.updateMachineAgent({
      agentName: POINT_GUARD_AGENT,
      command: nextCommand,
      launchArgs: nextArgs,
      projectRoot: null,
    });

  return (
    <div className="settings-pane" data-testid="settings-point-guard">
      <h2 className="settings-title">Point Guard</h2>

      <SettingsGroup label="Connection">
        <div className="settings-row">
          <span
            className="settings-status-dot"
            data-state={pg.ready ? 'ready' : pg.running ? 'starting' : 'stopped'}
          />
          <span data-testid="point-guard-status">{status}</span>
          <span className="settings-spacer" />
          {pg.running ? (
            <>
              <button type="button" className="settings-button" onClick={() => void service.stop()}>
                Stop Point Guard
              </button>
              <button
                type="button"
                className="settings-button"
                onClick={() => void service.restart()}
              >
                Restart Point Guard
              </button>
            </>
          ) : (
            <button type="button" className="settings-button" onClick={() => void service.start()}>
              Start / Retry
            </button>
          )}
        </div>
        {pg.remoteEndpoint !== null && (
          <SettingsCaption>Phones reach this PC at {pg.remoteEndpoint}.</SettingsCaption>
        )}
        <SettingsCaption>
          Point Guard is included with PurePoint and updated with the app. Your provider and
          conversations are saved on this PC.
        </SettingsCaption>
        {pg.error !== null && <p className="settings-error">{pg.error}</p>}
      </SettingsGroup>

      {chat !== undefined && (
        <SettingsGroup label="Setup">
          <PointGuardSetupView service={service} chat={chat} embedded />
        </SettingsGroup>
      )}

      <SettingsGroup label="Pairing">
        <SettingsCaption>
          Import a pairing from another Point Guard: paste its pairing code, or enter its address
          and pairing secret. The secret is saved in Windows Credential Manager.
        </SettingsCaption>
        <PairingImportForm onImport={onImport} />
      </SettingsGroup>

      <SettingsGroup label="Agent launch">
        <fieldset className="settings-form" disabled={launchDisabled}>
          <label className="settings-field-row">
            <span>Launch command</span>
            <input
              className="settings-text-field"
              aria-label="Point Guard launch command"
              placeholder={POINT_GUARD_AGENT}
              spellCheck={false}
              defaultValue={command}
              key={command}
              onBlur={(event) => {
                if (event.target.value !== command) void saveLaunch(event.target.value, args);
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') event.currentTarget.blur();
              }}
            />
          </label>
          <SettingsToggle
            label="Skip permission prompts"
            checked={skipsPermissions(args)}
            onChange={(skip) => void saveLaunch(command, withSkipPermissions(args, skip))}
          />
          <SettingsCaption>
            Claude Code for all projects: launches with {SKIP_PERMISSIONS_FLAG} when on. Worktrees
            isolate git state, not your files or network. Edit every agent in Settings › Agents.
          </SettingsCaption>
          {agents.globalError !== null && <p className="settings-error">{agents.globalError}</p>}
        </fieldset>
      </SettingsGroup>
    </div>
  );
}
