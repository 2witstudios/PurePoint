// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/AddMachineSheet.swift (WIN-8.14).
//
// Register a machine reachable over SSH. PurePoint stores only its name and SSH destination (in
// `%USERPROFILE%\.pu\machines.json`, shared with `pu machine`); keys stay with ssh (MACH-001).
//
// Differences from Swift:
// - The Hosts menu is a `<datalist>` on the SSH host field, from `%USERPROFILE%\.ssh\config`.
// - Test Connection runs the relay's `uname -s` probe (there is no remote home to resolve without
//   a forward); it reports whether the machine runs a Unix shell or Windows.
// - Results are announced through a live region (VoiceOver's announcement on the Mac).

import { useEffect, useId, useState } from 'react';
import {
  INVALID_HOST_MESSAGE,
  isValidSshHost,
  machineEntryError,
  sshConfigHostAliases,
  suggestedMachineName,
} from '../../model/machine.ts';
import { readSshConfig, testMachineConnection } from '../../services/machines.ts';
import { useAppState } from '../shell/appContext.ts';

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

export interface AddMachineSheetProps {
  readonly onDismiss: () => void;
  /** The host aliases offered (tests); read from the ssh config by default. */
  readonly loadAliases?: () => Promise<string[]>;
  readonly testConnection?: (sshHost: string) => Promise<string>;
}

const defaultAliases = async () => sshConfigHostAliases(await readSshConfig());

export function AddMachineSheet({
  onDismiss,
  loadAliases = defaultAliases,
  testConnection = testMachineConnection,
}: AddMachineSheetProps) {
  const app = useAppState();
  const titleId = useId();
  const listId = useId();
  const [name, setName] = useState('');
  const [sshHost, setSshHost] = useState('');
  const [aliases, setAliases] = useState<string[]>([]);
  const [testResult, setTestResult] = useState<{ text: string; ok: boolean } | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [isAdding, setIsAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    loadAliases().then(
      (found) => {
        if (live) setAliases(found);
      },
      () => {
        // No ssh config: nothing to offer.
      },
    );
    return () => {
      live = false;
    };
  }, [loadAliases]);

  const trimmedName = name.trim();
  const trimmedHost = sshHost.trim();
  const canAdd = trimmedName !== '' && trimmedHost !== '' && !isAdding;

  const chooseHost = (value: string) => {
    setSshHost(value);
    if (name === '' && aliases.includes(value)) setName(suggestedMachineName(value));
  };

  const runTest = () => {
    if (!isValidSshHost(trimmedHost)) {
      setTestResult({ text: INVALID_HOST_MESSAGE, ok: false });
      return;
    }
    setIsTesting(true);
    setTestResult(null);
    testConnection(trimmedHost).then(
      (text) => {
        setTestResult({ text, ok: true });
        setIsTesting(false);
      },
      (failure: unknown) => {
        setTestResult({ text: errorMessage(failure), ok: false });
        setIsTesting(false);
      },
    );
  };

  const add = () => {
    if (!canAdd) return;
    const known = app.machines.map((machine) => machine.machine);
    const problem = machineEntryError(trimmedName, trimmedHost, known);
    if (problem !== null) {
      setError(problem);
      return;
    }
    setIsAdding(true);
    setError(null);
    app.addMachine(trimmedName, trimmedHost).then(
      () => {
        onDismiss();
      },
      (failure: unknown) => {
        setError(errorMessage(failure));
        setIsAdding(false);
      },
    );
  };

  return (
    <div
      className="sidebar-confirm-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onDismiss();
      }}
    >
      <div
        className="sidebar-confirm sidebar-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-testid="add-machine-sheet"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onDismiss();
          } else if (event.key === 'Enter' && event.target instanceof HTMLInputElement) {
            event.preventDefault();
            add();
          }
        }}
      >
        <h2 id={titleId} className="sidebar-confirm-title">
          Add Machine
        </h2>
        <p className="sidebar-confirm-message">
          Connect to PurePoint on another computer you can reach with ssh. Your ssh config, keys and
          agent are used as they are.
        </p>
        <label className="sidebar-sheet-field">
          <span>Name</span>
          <input
            type="text"
            value={name}
            placeholder="studio"
            spellCheck={false}
            autoFocus
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        </label>
        <label className="sidebar-sheet-field">
          <span>SSH host</span>
          <input
            type="text"
            value={sshHost}
            placeholder="alias or user@host[:port]"
            spellCheck={false}
            list={aliases.length === 0 ? undefined : listId}
            title={aliases.length === 0 ? undefined : 'Hosts from ~/.ssh/config'}
            onChange={(event) => {
              chooseHost(event.target.value);
            }}
          />
        </label>
        {aliases.length > 0 && (
          <datalist id={listId}>
            {aliases.map((alias) => (
              <option key={alias} value={alias} />
            ))}
          </datalist>
        )}
        <div aria-live="polite">
          {testResult !== null && (
            <p className="sidebar-sheet-note" data-ok={testResult.ok ? 'true' : 'false'}>
              {testResult.text}
            </p>
          )}
          {error !== null && (
            <p className="sidebar-sheet-note" data-ok="false" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="sidebar-confirm-buttons">
          <button
            type="button"
            className="sidebar-sheet-leading"
            disabled={isTesting || trimmedHost === ''}
            onClick={runTest}
          >
            {isTesting ? 'Testing…' : 'Test Connection'}
          </button>
          <button type="button" onClick={onDismiss}>
            Cancel
          </button>
          <button type="button" data-default="true" disabled={!canAdd} onClick={add}>
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
