// Port of apps/purepoint-macos/purepoint-macos/State/MachineState+OpenProject.swift (WIN-8.14),
// with the folder-browsing request of the iPad's remote folder picker (`DaemonRequest.fsBrowse`,
// RemoteFolderPicker+iOS.swift), which the Windows Open Remote Project picker uses too.

import type { Request } from '../protocol';
import { projectKey, projectKeyId } from '../model/projectKey';
import { isDaemonRunning } from '../model/sidebarItem';
import type { DaemonRequester } from '../services/daemonConnection';
import type { MachineStateOwner } from './machineState';
import type { MachineConnectionState } from '../model/sidebarItem';

/** A project action the machine's daemon refused, with its message. */
export class MachineOpenProjectError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'MachineOpenProjectError';
  }
}

/** What the open helpers need of a machine (`LiveMachineState`). */
export interface OpenProjectMachine {
  readonly machine: { readonly id: string; readonly name: string };
  readonly state: MachineConnectionState;
  readonly appState: MachineStateOwner;
  unhideProject(root: string): void;
  refreshProjects(): Promise<void>;
}

/** The longest listing the picker asks for (Swift `fsBrowse`'s default). */
export const FS_BROWSE_MAX_ENTRIES = 20_000;

/**
 * Swift `DaemonRequest.fsBrowse(path:includeHidden:)`: a directories-only `fs_list`, never
 * recursive. `~` names the daemon user's home.
 */
export function fsBrowseRequest(path: string, includeHidden: boolean): Request {
  return {
    type: 'fs_list',
    path,
    include_hidden: includeHidden,
    max_entries: FS_BROWSE_MAX_ENTRIES,
    directories_only: true,
  };
}

const BUSY_RETRIES = 4;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/**
 * Open a folder on a machine as a project: register it with the machine's daemon (`init`, the
 * request a local open sends), then re-read the daemon's projects so it joins the sidebar under
 * the machine. Rejects with the daemon's message when it refuses the folder.
 */
export async function openMachineProject(
  machine: OpenProjectMachine,
  root: string,
  client: DaemonRequester,
  sleep: (ms: number) => Promise<void> = delay,
): Promise<void> {
  for (let attempt = 0; ;) {
    const response = await client.send({ type: 'init', project_root: root });
    if (response.type === 'init_result') break;
    if (response.type === 'error') {
      // At its connection limit: transient, so back off and retry, as the local open does.
      if (response.code === 'BUSY' && attempt < BUSY_RETRIES) {
        attempt += 1;
        await sleep(attempt * 1000);
        continue;
      }
      throw new MachineOpenProjectError(response.message);
    }
    throw new MachineOpenProjectError(`${machine.machine.name} did not register ${root}.`);
  }
  // A folder the user hid earlier comes back when they open it again.
  machine.unhideProject(root);
  // Otherwise the project arrives with the inventory read when the daemon is back.
  if (isDaemonRunning(machine.state)) await machine.refreshProjects();
  machine.appState.initializeActiveProjectIfNeeded(
    projectKeyId(projectKey(root, machine.machine.id)),
  );
}

/**
 * Remove a project from the machine for every device (`remove_project`). Its folder and `.pu/`
 * are kept, so opening it again brings it back. The daemon refuses while agents run unless
 * `suspendAgents`, which suspends them so they can be resumed once it is added again.
 */
export async function removeMachineProject(
  machine: OpenProjectMachine,
  root: string,
  suspendAgents: boolean,
  client: DaemonRequester,
): Promise<void> {
  const response = await client.send({
    type: 'remove_project',
    project_root: root,
    suspend_agents: suspendAgents,
  });
  if (response.type === 'error') throw new MachineOpenProjectError(response.message);
  if (response.type !== 'project_removed') {
    throw new MachineOpenProjectError(`${machine.machine.name} did not remove ${root}.`);
  }
  // Nothing left to hide: a later open shows it again everywhere.
  machine.unhideProject(root);
  if (isDaemonRunning(machine.state)) await machine.refreshProjects();
}
