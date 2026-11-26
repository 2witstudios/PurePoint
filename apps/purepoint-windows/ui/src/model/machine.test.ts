// Port of the platform-free cases of purepoint-macosTests/MachinesTests.swift (WIN-8.14):
// MachineRegistryTests (validation), MachineSSHTests (ssh config aliases), HealthProbeTests, and
// the machine status text. The registry's file format, lock and SSH argv cases are the backend's
// (`src-tauri/src/daemon/machine.rs`, `relay.rs`); ProjectKeyTests are `projectKey.test.ts`.
import { describe, expect, it } from 'vitest';
import {
  HealthProbeTracker,
  INVALID_HOST_MESSAGE,
  INVALID_NAME_MESSAGE,
  connectionStatesEqual,
  isValidMachineName,
  isValidSshHost,
  machineEntryError,
  machineStatusText,
  parseSshDestination,
  sshConfigHostAliases,
  sshHostString,
  suggestedMachineName,
} from './machine';

describe('MachineRegistry validation', () => {
  it('given invalid names or hosts, should reject', () => {
    for (const name of [
      '',
      '-x',
      '.hidden',
      'has space',
      'slash/name',
      'colon:x',
      'a'.repeat(65),
    ]) {
      expect(isValidMachineName(name), name).toBe(false);
    }
    for (const name of ['studio', 'Mac-2', 'build_box.local', '1']) {
      expect(isValidMachineName(name), name).toBe(true);
    }
    for (const host of ['', '-oProxyCommand=x', 'a b', 'a\nb', 'tab\there']) {
      expect(isValidSshHost(host), JSON.stringify(host)).toBe(false);
    }
    for (const host of ['studio-mac', 'me@10.0.0.2', 'me@host:2222']) {
      expect(isValidSshHost(host), host).toBe(true);
    }
  });

  it('given a new machine, should report the first problem with the Mac messages', () => {
    const existing = [{ id: 'a', name: 'Studio', sshHost: 'studio' }];
    expect(machineEntryError('bad name', 'host')).toBe(INVALID_NAME_MESSAGE);
    expect(machineEntryError('ok', '-x')).toBe(INVALID_HOST_MESSAGE);
    expect(machineEntryError('studio', 'other', existing)).toBe(
      'A machine named "Studio" already exists.',
    );
    expect(machineEntryError('build', 'build-box', existing)).toBeNull();
  });
});

describe('SSH', () => {
  it('given an ssh config, should offer concrete host aliases only', () => {
    const config = [
      '# comment',
      'Host studio studio-alt',
      '  HostName 10.0.0.2',
      'Host *.internal',
      'host=build',
      'Host !bad gpu? "quoted"',
      'Match host foo',
      'Host studio',
    ].join('\r\n');
    expect(sshConfigHostAliases(config)).toEqual(['studio', 'studio-alt', 'build', 'quoted']);
  });

  it('given a host, should suggest a machine name', () => {
    expect(suggestedMachineName('studio-mac')).toBe('studio-mac');
    expect(suggestedMachineName('jane@studio.local:2222')).toBe('studio.local');
    expect(suggestedMachineName('box (old)')).toBe('boxold');
  });

  it('given user@host[:port] destinations, should parse and print them', () => {
    expect(parseSshDestination('jane@studio')).toEqual({ user: 'jane', host: 'studio', port: 22 });
    expect(parseSshDestination('jane@studio:2222')).toEqual({
      user: 'jane',
      host: 'studio',
      port: 2222,
    });
    expect(parseSshDestination('me@[fe80::1]:2200')).toEqual({
      user: 'me',
      host: 'fe80::1',
      port: 2200,
    });
    expect(parseSshDestination('me@fe80::1')).toEqual({ user: 'me', host: 'fe80::1', port: 22 });
    expect(parseSshDestination('studio')).toBeNull();
    expect(parseSshDestination('me@host:0')).toBeNull();
    expect(parseSshDestination('me@host:99999')).toBeNull();
    expect(parseSshDestination('-me@host')).toBeNull();
    expect(sshHostString({ user: 'me', host: 'fe80::1', port: 2200 })).toBe('me@[fe80::1]:2200');
    expect(sshHostString({ user: 'me', host: 'box', port: 22 })).toBe('me@box');
  });
});

describe('HealthProbeTracker', () => {
  it('given one or two failed probes, should keep the last state', () => {
    const tracker = new HealthProbeTracker();
    expect(tracker.record({ kind: 'running' })).toEqual({ kind: 'running' });
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.record({ kind: 'running' })).toEqual({ kind: 'running' });
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.record({ kind: 'failed' })).toEqual({ kind: 'stopped' });
  });

  it('given BUSY, should neither report running nor count as a failure', () => {
    const tracker = new HealthProbeTracker();
    expect(tracker.record({ kind: 'busy' })).toBeNull();
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.record({ kind: 'busy' })).toBeNull();
    expect(tracker.record({ kind: 'failed' })).toBeNull();
    expect(tracker.consecutiveFailures).toBe(2);
    expect(tracker.record({ kind: 'incompatible', version: 7 })).toEqual({
      kind: 'incompatible',
      version: 7,
    });
  });
});

describe('machine status', () => {
  it('should read as the Mac sidebar does', () => {
    const running = { kind: 'connected', daemon: { kind: 'running' } } as const;
    expect(machineStatusText({ kind: 'disconnected' }, 0)).toBe('Disconnected');
    expect(machineStatusText({ kind: 'connecting' }, 0)).toBe('Connecting…');
    expect(machineStatusText({ kind: 'offline', reason: 'x' }, 2)).toBe('Offline');
    expect(machineStatusText(running, 0)).toBe('Connected');
    expect(machineStatusText(running, 1)).toBe('1 project');
    expect(machineStatusText(running, 3)).toBe('3 projects');
    expect(machineStatusText({ kind: 'connected', daemon: { kind: 'stopped' } }, 0)).toBe(
      'Daemon stopped',
    );
    expect(
      machineStatusText({ kind: 'connected', daemon: { kind: 'incompatible', version: 9 } }, 0),
    ).toBe('Incompatible (protocol 9)');
  });

  it('should compare connection states by value', () => {
    const stopped = { kind: 'connected', daemon: { kind: 'stopped' } } as const;
    expect(connectionStatesEqual(stopped, { kind: 'connected', daemon: { kind: 'stopped' } })).toBe(
      true,
    );
    expect(connectionStatesEqual(stopped, { kind: 'connected', daemon: { kind: 'running' } })).toBe(
      false,
    );
    expect(
      connectionStatesEqual(
        { kind: 'connected', daemon: { kind: 'incompatible', version: 1 } },
        { kind: 'connected', daemon: { kind: 'incompatible', version: 2 } },
      ),
    ).toBe(false);
    expect(
      connectionStatesEqual({ kind: 'offline', reason: 'a' }, { kind: 'offline', reason: 'b' }),
    ).toBe(false);
    expect(connectionStatesEqual({ kind: 'connecting' }, { kind: 'connecting' })).toBe(true);
  });
});
