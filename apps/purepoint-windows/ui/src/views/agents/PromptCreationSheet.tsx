// Port of apps/purepoint-macos/purepoint-macos/Views/Agents/PromptCreationSheet.swift (WIN-8.7).
import { useState } from 'react';
import {
  AgentTypes,
  PROMPT_SCOPE_CHOICES,
  PromptScope,
  type PromptScopeChoice,
} from '../../model/agentsHubModels.ts';
import type { AgentsHubState } from '../../state/agentsHubState.ts';
import { useAppState } from '../shell/appContext.ts';
import { Field, ProjectPicker, Segmented, Select, Sheet, TextInput } from './hubComponents.tsx';

export function PromptCreationSheet({
  hub,
  onDismiss,
}: {
  hub: AgentsHubState;
  onDismiss: () => void;
}) {
  const app = useAppState();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [scope, setScope] = useState<PromptScopeChoice>('global');
  const [agentType, setAgentType] = useState('');
  const [body, setBody] = useState('');
  const [command, setCommand] = useState('');
  const [projectRoot, setProjectRoot] = useState(() => app.activeLocalProjectRoot ?? '');

  const trimmedName = name.trim();
  const create = () => {
    const trimmedCommand = command.trim();
    const cmd = agentType === 'terminal' && trimmedCommand !== '' ? trimmedCommand : null;
    const saveRoot = scope === 'project' ? projectRoot : (app.projects[0]?.projectRoot ?? '');
    void (async () => {
      await hub.saveTemplate({
        projectRoot: saveRoot,
        projectRoots: app.libraryProjectRoots,
        name: trimmedName,
        description,
        agent: agentType,
        body,
        scope: PromptScope.wireValue(scope),
        command: cmd,
      });
      onDismiss();
    })();
  };

  return (
    <Sheet title="New Prompt" canSubmit={trimmedName !== ''} onCancel={onDismiss} onSubmit={create}>
      <TextInput label="Name" value={name} onChange={setName} />
      <TextInput label="Description" value={description} onChange={setDescription} />
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
      <Field label="Agent type">
        <Select
          label="Agent type"
          value={agentType}
          options={AgentTypes.withAny.map((t) => ({ value: t, title: t === '' ? 'Any' : t }))}
          onChange={setAgentType}
        />
      </Field>
      {agentType === 'terminal' && (
        <TextInput
          label="Command"
          placeholder="Command (e.g. npm run dev)"
          value={command}
          onChange={setCommand}
        />
      )}
      <textarea
        className="hub-textarea"
        aria-label="Prompt body"
        value={body}
        spellCheck={false}
        rows={7}
        onChange={(event) => {
          setBody(event.target.value);
        }}
      />
    </Sheet>
  );
}
