import { describe, expect, it } from 'vitest';
import {
  lastPathComponent,
  machineEndpoint,
  parseProjectKey,
  projectKey,
  projectKeyId,
  projectName,
} from './projectKey.ts';

// No Swift suite covers ProjectKey directly; these pin the string form the sidebar keys by.
describe('ProjectKey', () => {
  const uuid = 'AB4B5A3C-1D2E-4F60-8A9B-0C1D2E3F4A5B';

  it('givenALocalRootShouldUseTheBareRootAsItsId', () => {
    expect(projectKeyId(projectKey('/tmp/p'))).toBe('/tmp/p');
    expect(parseProjectKey('/tmp/p')).toEqual({ machineId: null, root: '/tmp/p' });
  });

  it('givenARemoteKeyShouldRoundTripWithALowercaseMachineId', () => {
    const id = projectKeyId(projectKey('/srv/app', uuid));
    expect(id).toBe(`machine:${uuid.toLowerCase()}:/srv/app`);
    expect(parseProjectKey(id)).toEqual({ machineId: uuid.toLowerCase(), root: '/srv/app' });
  });

  it('givenAMalformedRemoteKeyShouldReadItAsALocalRoot', () => {
    expect(parseProjectKey('machine:not-a-uuid:/x')).toEqual({
      machineId: null,
      root: 'machine:not-a-uuid:/x',
    });
  });

  it('givenARootShouldNameTheProjectByItsLastComponent', () => {
    expect(projectName(projectKey('/tmp/confirm/'))).toBe('confirm');
    expect(lastPathComponent('C:\\Users\\me\\repo')).toBe('repo');
    expect(lastPathComponent('/')).toBe('/');
  });

  it('givenAMachineShouldPutItsSocketUnderItsLowercaseDirectory', () => {
    expect(machineEndpoint(uuid, '/home/me/.pu').socketPath).toBe(
      `/home/me/.pu/machines/${uuid.toLowerCase()}/daemon.sock`,
    );
  });
});
