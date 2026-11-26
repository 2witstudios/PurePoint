// Port of apps/purepoint-macos/purepoint-macos/Views/Agents/AgentCreationSheet.swift (WIN-8.7).
import { useState } from 'react';
import { useStore } from 'zustand';
import {
  AGENT_PROMPT_SOURCE_MODES,
  agentPromptSourceModeTitle,
  AgentTypes,
  makeAgentDefinition,
  parseTags,
  PROMPT_SCOPE_CHOICES,
  PromptScope,
  type AgentPromptSourceMode,
  type PromptScopeChoice,
} from '../../model/agentsHubModels.ts';
import type { AgentsHubState } from '../../state/agentsHubState.ts';
import { useAppState } from '../shell/appContext.ts';
import {
  Field,
  ProjectPicker,
  Segmented,
  Select,
  Sheet,
  TextInput,
  Toggle,
} from './hubComponents.tsx';

export function AgentCreationSheet({
  hub,
  onDismiss,
}: {
  hub: AgentsHubState;
  onDismiss: () => void;
}) {
  const app = useAppState();
  const prompts = useStore(hub.store, (s) => s.prompts);
  const [name, setName] = useState('');
  const [agentType, setAgentType] = useState('claude');
  const [promptMode, setPromptMode] = useState<AgentPromptSourceMode>('library');
  const [templateName, setTemplateName] = useState('');
  const [inlinePrompt, setInlinePrompt] = useState('');
  const [tags, setTags] = useState('');
  const [scope, setScope] = useState<PromptScopeChoice>('global');
  const [availableInCommandDialog, setAvailableInCommandDialog] = useState(true);
  const [command, setCommand] = useState('');
  const [projectRoot, setProjectRoot] = useState(() => app.activeLocalProjectRoot ?? '');

  const trimmedName = name.trim();
  const isTerminal = agentType === 'terminal';
  const templateListId = 'hub-template-names';

  const create = () => {
    const trimmedCommand = command.trim();
    const def = makeAgentDefinition({
      name: trimmedName,
      agentType,
      template: !isTerminal && promptMode === 'library' ? templateName : null,
      inlinePrompt: !isTerminal && promptMode === 'inline' ? inlinePrompt : null,
      tags: parseTags(tags),
      scope: PromptScope.wireValue(scope),
      availableInCommandDialog,
      command: isTerminal && trimmedCommand !== '' ? trimmedCommand : null,
    });
    const saveRoot = scope === 'project' ? projectRoot : (app.projects[0]?.projectRoot ?? '');
    void (async () => {
      await hub.saveAgentDef({ projectRoot: saveRoot, projectRoots: app.libraryProjectRoots, def });
      onDismiss();
    })();
  };

  return (
    <Sheet title="New Agent" canSubmit={trimmedName !== ''} onCancel={onDismiss} onSubmit={create}>
      <TextInput label="Name" value={name} onChange={setName} />
      <Field label="Agent type">
        <Select
          label="Agent type"
          value={agentType}
          options={AgentTypes.all.map((t) => ({ value: t, title: t }))}
          onChange={setAgentType}
        />
      </Field>
      {isTerminal ? (
        <TextInput
          label="Command"
          placeholder="Command (e.g. npm run dev)"
          value={command}
          onChange={setCommand}
        />
      ) : (
        <>
          <Field label="Prompt source">
            <Segmented
              label="Prompt source"
              value={promptMode}
              options={AGENT_PROMPT_SOURCE_MODES}
              title={agentPromptSourceModeTitle}
              onChange={setPromptMode}
            />
          </Field>
          {promptMode === 'library' ? (
            <>
              <TextInput
                label="Template name"
                value={templateName}
                onChange={setTemplateName}
                list={templateListId}
              />
              <datalist id={templateListId}>
                {[...new Set(prompts.map((p) => p.name))].map((n) => (
                  <option key={n} value={n} />
                ))}
              </datalist>
            </>
          ) : (
            <textarea
              className="hub-textarea"
              aria-label="Inline prompt"
              value={inlinePrompt}
              spellCheck={false}
              rows={5}
              onChange={(event) => {
                setInlinePrompt(event.target.value);
              }}
            />
          )}
        </>
      )}
      <TextInput
        label="Tags"
        placeholder="Tags (comma-separated)"
        value={tags}
        onChange={setTags}
      />
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
      <Toggle
        label="Available in command dialog"
        checked={availableInCommandDialog}
        onChange={setAvailableInCommandDialog}
      />
    </Sheet>
  );
}
