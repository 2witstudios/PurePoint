// Port of the `GlobalAgentSettingsTests` suite in
// apps/purepoint-macos/purepoint-macosTests/purepoint_macosTests.swift (WIN-8.7).
//
// Swift encodes `DaemonRequest` cases with `JSONEncoder`; here requests are the generated protocol
// objects, so the encoding cases check the request builders' JSON.
import { describe, expect, it } from 'vitest';
import type { AgentConfigInfo, Request, Response } from '../protocol';
import { allVariants } from '../model/agentVariant';
import { parseDaemonResponse } from '../services/daemonConnection';
import {
  AgentConfigState,
  AgentLaunchDefaults,
  agentTypeOf,
  readGlobalAgentSettings,
  updateAgentConfigRequest,
  updateGlobalAgentSettingsRequest,
} from './agentConfigState';

const json = (request: Request) => JSON.parse(JSON.stringify(request)) as Record<string, unknown>;

function report(fields: {
  agentStatusTracking: boolean;
  launchArgs?: Record<string, string>;
  commands?: Record<string, string>;
}): Response {
  // An older daemon leaves `launch_args` / `commands` out; the generated type requires them.
  return {
    type: 'global_agent_settings_report',
    codex_yolo: false,
    agent_status_tracking: fields.agentStatusTracking,
    ...(fields.launchArgs === undefined ? {} : { launch_args: fields.launchArgs }),
    ...(fields.commands === undefined ? {} : { commands: fields.commands }),
  } as Response;
}

function agentPayload(fields: Partial<AgentConfigInfo> & { name: string }): AgentConfigInfo {
  return {
    command: fields.name,
    launch_args: null,
    resolved_launch_args: [],
    interactive: true,
    ...fields,
  };
}

describe('GlobalAgentSettingsTests', () => {
  it('projectlessSettingsEnsureDaemonBeforeLoadAndSave', async () => {
    const calls: string[] = [];
    const state = new AgentConfigState({
      ensureDaemon: () => {
        calls.push('ensure');
        return Promise.resolve();
      },
      sendRequest: (request) => {
        switch (request.type) {
          case 'get_global_agent_settings':
            calls.push('load');
            return Promise.resolve(report({ agentStatusTracking: true }));
          case 'update_global_agent_settings':
            calls.push('save');
            return Promise.resolve(
              report({ agentStatusTracking: request.agent_status_tracking ?? true }),
            );
          default:
            throw new Error('Unexpected project request');
        }
      },
    });
    await state.loadGlobalSettings();
    expect(state.globalSettingsLoaded).toBe(true);
    await state.updateAgentStatusTracking(false);
    expect(state.agentStatusTracking).toBe(false);
    expect(calls).toEqual(['ensure', 'load', 'ensure', 'save']);
  });

  it('projectlessSettingsCanRetryDaemonStartupFailure', async () => {
    let attempts = 0;
    let requests = 0;
    const state = new AgentConfigState({
      ensureDaemon: () => {
        attempts += 1;
        return attempts === 1 ? Promise.reject(new Error('Startup')) : Promise.resolve();
      },
      sendRequest: () => {
        requests += 1;
        return Promise.resolve(report({ agentStatusTracking: true }));
      },
    });
    await state.loadGlobalSettings();
    expect(state.globalSettingsLoaded).toBe(false);
    expect(state.globalError).not.toBeNull();
    expect(requests).toBe(0);
    await state.loadGlobalSettings();
    expect(state.globalSettingsLoaded).toBe(true);
    expect(state.globalError).toBeNull();
    expect(requests).toBe(1);
  });

  it('projectLaunchArgsAreSentAsOneString', () => {
    const sent = json(
      updateAgentConfigRequest({
        projectRoot: '/p',
        agentName: 'claude',
        launchArgs: "--model opus --append-system-prompt 'be terse'",
      }),
    );
    expect(sent.launch_args).toBe("--model opus --append-system-prompt 'be terse'");
    const cleared = json(
      updateAgentConfigRequest({ projectRoot: '/p', agentName: 'claude', launchArgs: null }),
    );
    // null clears the project value
    expect(cleared).toHaveProperty('launch_args', null);
  });

  it('machineLaunchArgsSaveSendsOnlyThatAgent', async () => {
    const sent = json(
      updateGlobalAgentSettingsRequest({
        launchArgs: { claude: '--dangerously-skip-permissions' },
      }),
    );
    expect(sent.launch_args).toEqual({ claude: '--dangerously-skip-permissions' });
    expect(sent).not.toHaveProperty('codex_yolo');
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: (request) => {
        if (request.type !== 'update_global_agent_settings') {
          return Promise.resolve({ type: 'error', code: 'TEST', message: 'Unexpected request' });
        }
        return Promise.resolve(
          report({
            agentStatusTracking: true,
            launchArgs: request.launch_args ?? {},
            commands: request.commands ?? {},
          }),
        );
      },
    });
    await state.updateMachineAgent({
      agentName: 'claude',
      command: '',
      launchArgs: '--model opus',
      projectRoot: null,
    });
    expect(state.machineLaunchArgs).toEqual({ claude: '--model opus' });
    expect(state.globalError).toBeNull();
  });

  it('oldDaemonMachineLaunchArgsSaveReportsUpdateNeeded', async () => {
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: (request) =>
        Promise.resolve(
          request.type === 'get_global_agent_settings'
            ? report({ agentStatusTracking: true, launchArgs: { claude: '--model opus' } })
            : // A protocol-v10 daemon ignores machine launch args and omits them from its report.
              report({ agentStatusTracking: true }),
        ),
    });
    await state.loadGlobalSettings();
    await state.updateMachineAgent({
      agentName: 'claude',
      command: '',
      launchArgs: '--model sonnet',
      projectRoot: null,
    });
    expect(state.globalError).toContain('Update the PurePoint daemon');
    // An old daemon's report must not wipe the field
    expect(state.machineLaunchArgs).toEqual({ claude: '--model opus' });
  });

  it('oldDaemonConfigReportReportsUpdateNeeded', async () => {
    const oldAgent = agentPayload({ name: 'claude' });
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: () =>
        Promise.resolve({ type: 'config_report', default_agent: 'claude', agents: [oldAgent] }),
    });
    await state.load('/p');
    expect(state.error).toContain('Update the PurePoint daemon');
    await state.updateProjectAgent({
      projectRoot: '/p',
      agentName: 'claude',
      command: '',
      launchArgs: '--model opus',
    });
    expect(state.error).toContain('Update the PurePoint daemon');
  });

  it('configReportDecodesLaunchArgStrings', () => {
    const response = parseDaemonResponse(
      '{"type":"config_report","default_agent":"claude","agents":[{"name":"claude","command":"claude","launch_args":null,"resolved_launch_args":["--model","opus"],"interactive":true,"machine_launch_args":"--model opus","resolved_command":"claude --model opus"}]}',
    );
    if (response.type !== 'config_report') throw new Error('Expected config report');
    const claude = response.agents[0];
    expect(claude?.project_launch_args ?? null).toBeNull();
    expect(claude?.machine_launch_args).toBe('--model opus');
    expect(claude?.resolved_command).toBe('claude --model opus');
  });

  it('globalSettingsProtocolRoundTrip', () => {
    const sent = json(updateGlobalAgentSettingsRequest({ agentStatusTracking: true }));
    expect(sent.type).toBe('update_global_agent_settings');
    expect(sent).not.toHaveProperty('codex_yolo');
    expect(sent).not.toHaveProperty('project_root');
    expect(sent).not.toHaveProperty('launch_args');
    // Daemons from before Codex YOLO was removed still send `codex_yolo`; it is ignored.
    const response = parseDaemonResponse(
      '{"type":"global_agent_settings_report","codex_yolo":true}',
    );
    if (response.type !== 'global_agent_settings_report') {
      throw new Error('Expected global settings response');
    }
    const settings = readGlobalAgentSettings(response);
    expect(settings.agentStatusTracking).toBe(true); // Older daemons always track status
    expect(settings.launchArgs).toBeNull(); // Older daemons have no machine launch args
    expect(settings.commands).toBeNull(); // Older daemons have no machine commands
  });

  it('projectAgentSaveSendsCommandAndArgsTogether', () => {
    const sent = json(
      updateAgentConfigRequest({
        projectRoot: '/p',
        agentName: 'codex',
        launchArgs: '--search',
        command: 'scripts/agent-launch.sh codex',
      }),
    );
    expect(sent.command).toBe('scripts/agent-launch.sh codex');
    expect(sent.launch_args).toBe('--search');
    expect(sent).not.toHaveProperty('remove');
    const removed = json(
      updateAgentConfigRequest({
        projectRoot: '/p',
        agentName: 'aider',
        launchArgs: null,
        remove: true,
      }),
    );
    expect(removed.remove).toBe(true);
    // Left out, the project command is kept
    expect(removed).not.toHaveProperty('command');
  });

  it('blankProjectArgsKeepAnExplicitBareLaunch', async () => {
    const bare = agentPayload({
      name: 'claude',
      launch_args: [],
      project_launch_args: '',
      machine_launch_args: '--dangerously-skip-permissions',
      resolved_command: 'claude',
      inherited_command: 'claude',
      origin: 'builtin',
      id: 'claude',
    });
    const inherits = agentPayload({
      name: 'codex',
      resolved_command: 'codex',
      inherited_command: 'codex',
      origin: 'builtin',
      id: 'codex',
    });
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: () =>
        Promise.resolve({
          type: 'config_report',
          default_agent: 'claude',
          agents: [bare, inherits],
        }),
    });
    await state.load('/p');
    // Editing only the command keeps the bare launch
    expect(state.projectLaunchArgs('claude', ' ')).toBe('');
    // A blank field inherits
    expect(state.projectLaunchArgs('codex', '')).toBeNull();
    expect(state.projectLaunchArgs('codex', ' --search ')).toBe('--search');
  });

  it('addedMachineAgentIsNamedAfterItsCommandAndOffered', async () => {
    let sent: Request | null = null;
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: (request) => {
        sent = request;
        if (request.type !== 'update_global_agent_settings') {
          return Promise.resolve({ type: 'error', code: 'TEST', message: 'Unexpected request' });
        }
        return Promise.resolve(
          report({
            agentStatusTracking: true,
            launchArgs: request.launch_args ?? {},
            commands: request.commands ?? {},
          }),
        );
      },
    });
    await state.addMachineAgent({ command: 'npx aider', launchArgs: '--yes', projectRoot: null });
    const request = sent as Request | null;
    if (request?.type !== 'update_global_agent_settings') {
      throw new Error('Expected a machine settings save');
    }
    expect(request.commands).toEqual({ aider: 'npx aider' });
    expect(request.launch_args).toEqual({ aider: '--yes' });
    expect(state.machineAgentNames).toEqual(['claude', 'codex', 'opencode', 'aider']);
    expect(
      allVariants(state.addedAgents).some((v) => v.id === 'aider' && v.subtitle === 'npx aider'),
    ).toBe(true);
  });

  it('newAgentNamesComeFromTheProgramTheyRun', () => {
    expect(AgentLaunchDefaults.agentName('aider', [])).toBe('aider');
    expect(AgentLaunchDefaults.agentName('scripts/agent-launch.sh goose', [])).toBe('goose');
    expect(AgentLaunchDefaults.agentName('~/bin/My-Tool --fast', [])).toBe('my-tool');
    expect(AgentLaunchDefaults.agentName('run-agent.sh', [])).toBe('run-agent');
    expect(AgentLaunchDefaults.agentName('claude --model opus', ['claude'])).toBe('claude-2');
    expect(AgentLaunchDefaults.agentName('--', [])).toBe('agent');
  });

  it('configReportDecodesCommandsAndOrigin', () => {
    const response = parseDaemonResponse(
      '{"type":"config_report","default_agent":"claude","agents":[{"id":"aider","name":"aider","command":"aider","launch_args":null,"resolved_launch_args":[],"interactive":true,"resolved_command":"aider","project_command":"aider","inherited_command":"aider","origin":"project"}]}',
    );
    if (response.type !== 'config_report') throw new Error('Expected config report');
    const aider = response.agents[0];
    if (aider === undefined) throw new Error('Expected an agent');
    expect(agentTypeOf(aider)).toBe('aider');
    expect(aider.project_command).toBe('aider');
    expect(aider.inherited_command).toBe('aider');
    expect(aider.origin).toBe('project');
  });

  it('statusTrackingUpdateSendsOnlyItsOwnField', async () => {
    const sent = json(updateGlobalAgentSettingsRequest({ agentStatusTracking: false }));
    expect(sent.agent_status_tracking).toBe(false);
    expect(sent).not.toHaveProperty('codex_yolo');
    const state = new AgentConfigState({
      ensureDaemon: () => Promise.resolve(),
      sendRequest: () => Promise.resolve(report({ agentStatusTracking: false })),
    });
    await state.updateAgentStatusTracking(false);
    expect(state.agentStatusTracking).toBe(false);
  });
});
