// Port of apps/purepoint-macos/purepoint-macos/Views/Agents/SwarmCreationSheet.swift (WIN-8.7).
import { useState } from 'react';
import { useStore } from 'zustand';
import {
  makeSwarmDefinition,
  makeSwarmRosterItem,
  PROMPT_SCOPE_CHOICES,
  PromptScope,
  type PromptScopeChoice,
  type SwarmRosterItem,
} from '../../model/agentsHubModels.ts';
import type { AgentsHubState } from '../../state/agentsHubState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useAppState } from '../shell/appContext.ts';
import {
  Field,
  ProjectPicker,
  SectionLabel,
  Segmented,
  Sheet,
  Stepper,
  TextInput,
  Toggle,
} from './hubComponents.tsx';

export function SwarmCreationSheet({
  hub,
  onDismiss,
}: {
  hub: AgentsHubState;
  onDismiss: () => void;
}) {
  const app = useAppState();
  const agents = useStore(hub.store, (s) => s.agents);
  const [name, setName] = useState('');
  const [worktreeCount, setWorktreeCount] = useState(2);
  const [worktreeTemplate, setWorktreeTemplate] = useState('');
  const [roster, setRoster] = useState<SwarmRosterItem[]>([]);
  const [includeTerminal, setIncludeTerminal] = useState(true);
  const [scope, setScope] = useState<PromptScopeChoice>('global');
  const [projectRoot, setProjectRoot] = useState(() => app.activeLocalProjectRoot ?? '');

  const trimmedName = name.trim();
  const agentListId = 'hub-agent-def-names';
  const update = (id: string, change: Partial<SwarmRosterItem>) => {
    setRoster((items) => items.map((item) => (item.id === id ? { ...item, ...change } : item)));
  };

  const create = () => {
    const def = makeSwarmDefinition({
      name: trimmedName,
      worktreeCount,
      worktreeTemplate,
      roster,
      includeTerminal,
      scope: PromptScope.wireValue(scope),
    });
    const saveRoot = scope === 'project' ? projectRoot : (app.projects[0]?.projectRoot ?? '');
    void (async () => {
      await hub.saveSwarmDef({ projectRoot: saveRoot, projectRoots: app.libraryProjectRoots, def });
      onDismiss();
    })();
  };

  return (
    <Sheet
      title="New Swarm"
      canSubmit={trimmedName !== '' && roster.length > 0}
      onCancel={onDismiss}
      onSubmit={create}
    >
      <TextInput label="Name" value={name} onChange={setName} />
      <Field label={`Worktrees: ${String(worktreeCount)}`}>
        <Stepper
          label="Worktrees"
          value={worktreeCount}
          min={1}
          max={8}
          onChange={setWorktreeCount}
        />
      </Field>
      <TextInput
        label="Worktree template"
        value={worktreeTemplate}
        onChange={setWorktreeTemplate}
      />
      <div className="hub-roster" aria-label="Roster">
        <SectionLabel>Roster</SectionLabel>
        <datalist id={agentListId}>
          {[...new Set(agents.map((a) => a.name))].map((n) => (
            <option key={n} value={n} />
          ))}
        </datalist>
        {roster.map((item, index) => (
          <div className="hub-roster-row" key={item.id} data-testid="roster-row">
            <TextInput
              label={`Agent def ${String(index + 1)}`}
              placeholder="Agent def"
              value={item.agentDef}
              list={agentListId}
              onChange={(agentDef) => {
                update(item.id, { agentDef });
              }}
            />
            <TextInput
              label={`Role ${String(index + 1)}`}
              placeholder="Role"
              value={item.role}
              onChange={(role) => {
                update(item.id, { role });
              }}
            />
            <Stepper
              label={`Quantity ${String(index + 1)}`}
              value={item.quantity}
              min={1}
              max={8}
              onChange={(quantity) => {
                update(item.id, { quantity });
              }}
            />
            <button
              type="button"
              className="hub-icon-button hub-destructive"
              aria-label={`Remove agent ${String(index + 1)}`}
              onClick={() => {
                setRoster((items) => items.filter((other) => other.id !== item.id));
              }}
            >
              <Icon name="minus.circle.fill" size={14} />
            </button>
          </div>
        ))}
        <button
          type="button"
          className="hub-link-button"
          onClick={() => {
            setRoster((items) => [
              ...items,
              makeSwarmRosterItem({ agentDef: '', role: '', quantity: 1 }),
            ]);
          }}
        >
          <Icon name="plus.circle" size={14} />
          Add Agent
        </button>
      </div>
      <Toggle label="Include terminal" checked={includeTerminal} onChange={setIncludeTerminal} />
      <Field label="Scope">
        <Segmented
          label="Scope"
          value={scope}
          options={PROMPT_SCOPE_CHOICES}
          title={PromptScope.title}
          onChange={setScope}
        />
      </Field>
      {scope === 'project' && (
        <ProjectPicker projects={app.projects} value={projectRoot} onChange={setProjectRoot} />
      )}
    </Sheet>
  );
}
