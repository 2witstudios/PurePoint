// Port of apps/purepoint-macos/purepoint-macos/Views/Settings/SettingsAgentsView.swift (WIN-8.11):
// agent status tracking, then each agent's launch command and arguments for all projects
// (`update_global_agent_settings`, ~/.pu/agent-settings.yaml) and for the open project
// (`update_agent_config`, .pu/config.yaml), through state/agentConfigState.ts and the daemon.
import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { AgentConfigInfo } from '../../protocol';
import type { AppState } from '../../state/appState.ts';
import {
  AgentLaunchDefaults,
  type AgentConfigState,
  agentTypeOf,
} from '../../state/agentConfigState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { SettingsCaption, SettingsGroup, SettingsToggle } from './SettingsGroup.tsx';

/** One agent in a launch table: what is stored at this level, and what runs when it is empty. */
export interface AgentLaunchRowModel {
  readonly agentType: string;
  readonly command: string;
  readonly launchArgs: string;
  readonly commandHint: string;
  readonly launchArgsHint: string;
  readonly removable: boolean;
}

/** A project row's grey hints are exactly what runs when its field is left empty. */
export function projectRow(payload: AgentConfigInfo): AgentLaunchRowModel {
  // An explicit empty `launchArgs` in .pu/config.yaml launches the bare command.
  const launchArgsHint =
    payload.project_launch_args === '' ? '' : (payload.machine_launch_args ?? '');
  return {
    agentType: agentTypeOf(payload),
    command: payload.project_command ?? '',
    launchArgs: payload.project_launch_args ?? '',
    commandHint: payload.inherited_command ?? payload.command,
    launchArgsHint,
    removable: payload.origin === 'project',
  };
}

/** Two text fields, saved together when either is submitted or the row loses focus. */
function AgentLaunchRow({
  command,
  launchArgs,
  commandHint,
  launchArgsHint,
  label,
  onSave,
  onRemove,
}: {
  command: string;
  launchArgs: string;
  commandHint: string;
  launchArgsHint: string;
  label: string;
  onSave: (command: string, args: string) => Promise<void>;
  onRemove: (() => Promise<void>) | null;
}) {
  const [commandText, setCommandText] = useState(command);
  const [argsText, setArgsText] = useState(launchArgs);
  // Swift `.onChange(of:)`: a saved value from the daemon replaces the field's text.
  const [shown, setShown] = useState({ command, launchArgs });
  if (shown.command !== command || shown.launchArgs !== launchArgs) {
    if (shown.command !== command) setCommandText(command);
    if (shown.launchArgs !== launchArgs) setArgsText(launchArgs);
    setShown({ command, launchArgs });
  }
  const row = useRef<HTMLDivElement>(null);

  const save = () => {
    if (commandText === command && argsText === launchArgs) return;
    void onSave(commandText, argsText);
  };

  return (
    <div
      className="settings-launch-row"
      ref={row}
      data-agent={label}
      onBlur={(event) => {
        // Moving between the row's two fields keeps focus in the row; leaving it saves.
        if (!row.current?.contains(event.relatedTarget)) save();
      }}
    >
      <input
        className="settings-text-field settings-launch-command"
        aria-label={`Launch command ${label}`}
        placeholder={commandHint}
        spellCheck={false}
        value={commandText}
        onChange={(event) => {
          setCommandText(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') save();
        }}
      />
      <input
        className="settings-text-field settings-launch-args"
        aria-label={`Arguments ${label}`}
        placeholder={launchArgsHint}
        spellCheck={false}
        value={argsText}
        onChange={(event) => {
          setArgsText(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') save();
        }}
      />
      {onRemove !== null ? (
        <button
          type="button"
          className="settings-icon-button"
          aria-label={`Remove agent ${label}`}
          onClick={() => void onRemove()}
        >
          <Icon name="xmark" size={12} />
        </button>
      ) : (
        <span className="settings-launch-remove" />
      )}
    </div>
  );
}

let draftCounter = 0;

/** Launch command and arguments, one row per agent, plus rows for new agents. */
export function AgentLaunchTable({
  caption,
  rows,
  disabled = false,
  testId,
  onSave,
  onAdd,
  onRemove,
}: {
  caption: string;
  rows: readonly AgentLaunchRowModel[];
  disabled?: boolean;
  testId: string;
  onSave: (row: AgentLaunchRowModel, command: string, args: string) => Promise<void>;
  onAdd: (command: string, args: string) => Promise<void>;
  onRemove: (row: AgentLaunchRowModel) => Promise<void>;
}) {
  /** New rows not saved yet: an agent exists once its command is entered. */
  const [drafts, setDrafts] = useState<number[]>([]);
  const dropDraft = (draft: number) => {
    setDrafts((current) => current.filter((d) => d !== draft));
  };
  return (
    <fieldset className="settings-launch-table" disabled={disabled} data-testid={testId}>
      <SettingsCaption>{caption}</SettingsCaption>
      <div className="settings-launch-header">
        <span className="settings-launch-command">Launch command</span>
        <span className="settings-launch-args">Arguments</span>
        <span className="settings-launch-remove" />
      </div>
      {rows.map((row) => (
        <AgentLaunchRow
          key={row.agentType}
          label={row.agentType}
          command={row.command}
          launchArgs={row.launchArgs}
          commandHint={row.commandHint}
          launchArgsHint={row.launchArgsHint}
          onSave={(command, args) => onSave(row, command, args)}
          onRemove={row.removable ? () => onRemove(row) : null}
        />
      ))}
      {drafts.map((draft) => (
        <AgentLaunchRow
          key={`draft-${String(draft)}`}
          label={`new ${String(draft)}`}
          command=""
          launchArgs=""
          commandHint="command"
          launchArgsHint=""
          onSave={async (command, args) => {
            if (command.trim() === '') return;
            dropDraft(draft);
            await onAdd(command, args);
          }}
          onRemove={() => {
            dropDraft(draft);
            return Promise.resolve();
          }}
        />
      ))}
      <button
        type="button"
        className="settings-button"
        onClick={() => {
          draftCounter += 1;
          const draft = draftCounter;
          setDrafts((current) => [...current, draft]);
        }}
      >
        <Icon name="plus" size={12} /> Add agent
      </button>
    </fieldset>
  );
}

function ProjectAgents({ config, projectRoot }: { config: AgentConfigState; projectRoot: string }) {
  const state = useStore(config.store);
  useEffect(() => {
    void config.load(projectRoot);
  }, [config, projectRoot]);

  return (
    <>
      {state.error !== null && (
        <div className="settings-warning-row">
          <Icon name="exclamationmark.triangle.fill" className="settings-warning" />
          <span className="settings-secondary">{state.error}</span>
        </div>
      )}
      {state.isLoading && state.agents.length === 0 ? (
        <div className="settings-progress" role="progressbar" aria-label="Loading agents" />
      ) : (
        <SettingsGroup
          label={
            <span className="settings-label-icon">
              <Icon name="folder" size={12} /> Agents — This Project
            </span>
          }
        >
          <AgentLaunchTable
            testId="agents-project"
            caption="Empty fields use the All Projects value, shown in grey."
            rows={state.agents.map(projectRow)}
            onSave={(row, command, launchArgs) =>
              config.updateProjectAgent({
                projectRoot,
                agentName: row.agentType,
                command,
                launchArgs,
              })
            }
            onAdd={(command, launchArgs) =>
              config.addProjectAgent({ projectRoot, command, launchArgs })
            }
            onRemove={(row) => config.removeProjectAgent(projectRoot, row.agentType)}
          />
        </SettingsGroup>
      )}
    </>
  );
}

export function SettingsAgentsView({ app, config }: { app: AppState; config: AgentConfigState }) {
  const state = useStore(config.store);
  const projectRoot = useStore(app.store, () => app.activeLocalProjectRoot);
  useEffect(() => {
    void config.loadGlobalSettings();
  }, [config]);

  const globalDisabled = !state.globalSettingsLoaded || state.isSavingGlobalSettings;
  const machineRows: AgentLaunchRowModel[] = config.machineAgentNames.map((name) => ({
    agentType: name,
    command: state.machineCommands[name] ?? '',
    launchArgs: state.machineLaunchArgs[name] ?? '',
    commandHint: name,
    launchArgsHint: '',
    removable: !AgentLaunchDefaults.builtIns.includes(name),
  }));

  return (
    <div className="settings-pane" data-testid="settings-agents">
      <SettingsGroup
        label={
          <span className="settings-label-icon">
            <Icon name="desktopcomputer" size={12} /> Machine Settings
          </span>
        }
      >
        <SettingsToggle
          label="Agent status tracking"
          checked={state.agentStatusTracking}
          disabled={globalDisabled}
          onChange={(enabled) => void config.updateAgentStatusTracking(enabled)}
        />
        <SettingsCaption>
          Show whether Claude Code, Codex, OpenCode and Point Guard agents are working, idle or
          waiting on you. Works with any version of those tools. Applies to new and resumed
          sessions.
        </SettingsCaption>
        {state.globalError !== null && (
          <div className="settings-error-row">
            <span className="settings-error">{state.globalError}</span>
            <button
              type="button"
              className="settings-button"
              onClick={() => void config.loadGlobalSettings()}
            >
              Retry
            </button>
          </div>
        )}
      </SettingsGroup>

      <SettingsGroup
        label={
          <span className="settings-label-icon">
            <Icon name="desktopcomputer" size={12} /> Agents — All Projects
          </span>
        }
      >
        <AgentLaunchTable
          testId="agents-machine"
          caption="What runs in the terminal: launch command, then arguments."
          rows={machineRows}
          disabled={globalDisabled}
          onSave={(row, command, launchArgs) =>
            config.updateMachineAgent({
              agentName: row.agentType,
              command,
              launchArgs,
              projectRoot,
            })
          }
          onAdd={(command, launchArgs) =>
            config.addMachineAgent({ command, launchArgs, projectRoot })
          }
          onRemove={(row) => config.removeMachineAgent(row.agentType, projectRoot)}
        />
      </SettingsGroup>

      {projectRoot !== null ? (
        <ProjectAgents config={config} projectRoot={projectRoot} />
      ) : (
        <div className="settings-unavailable">
          <Icon name="folder.badge.questionmark" size={28} />
          <strong>No Project Open</strong>
          <span className="settings-secondary">Open a project to set its agents.</span>
        </div>
      )}
    </div>
  );
}
