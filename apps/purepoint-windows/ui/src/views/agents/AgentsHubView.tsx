// Port of apps/purepoint-macos/purepoint-macos/Views/Agents/AgentsHubView.swift (WIN-8.7).
//
// The Agents hub: tabs for the prompt library, agent definitions, swarms and triggers, each a
// list beside the selected item's detail. The Mac starts agents and swarms from the command
// palette; this hub can also spawn a prompt or an agent definition directly (as a root agent of
// the item's project) until the palette (WIN-7.x) lands.
import { useEffect, useState, type ReactNode } from 'react';
import { useStore } from 'zustand';
import {
  AGENTS_HUB_TABS,
  AgentTypes,
  agentsHubTabTitle,
  PROMPT_SCOPE_CHOICES,
  PromptScope,
  scopeBadge,
  swarmDefinitionTotalAgents,
  type AgentDefinition,
  type AgentsHubTab,
  type PromptScopeChoice,
  type SavedPrompt,
  type SwarmDefinition,
} from '../../model/agentsHubModels.ts';
import {
  agentsHubStateFor,
  selectedAgentOf,
  selectedPromptOf,
  selectedSwarmOf,
  type AgentsHubState,
} from '../../state/agentsHubState.ts';
import { triggersStateFor } from '../../state/triggersState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useAppState } from '../shell/appContext.ts';
import { InlineErrorBanner } from '../shell/DaemonErrorBanner.tsx';
import { AgentCreationSheet } from './AgentCreationSheet.tsx';
import {
  Badge,
  CodeSection,
  CommandHintBar,
  MasterDetail,
  SectionLabel,
  Segmented,
  Select,
  SurfaceCard,
  symbolOr,
  UsageRow,
} from './hubComponents.tsx';
import { PromptCreationSheet } from './PromptCreationSheet.tsx';
import { SwarmCreationSheet } from './SwarmCreationSheet.tsx';
import { TriggersTab } from '../triggers/TriggersTab.tsx';
import './agents.css';

/** What the hub needs from the app: where to load from and where to act. */
interface HubContext {
  hub: AgentsHubState;
  projectRoots: string[];
  /** Swift `anyProjectRoot`: the active local project's root, or `''`. */
  anyProjectRoot: string;
}

export function AgentsHubView() {
  const app = useAppState();
  const hub = agentsHubStateFor(app);
  const triggers = triggersStateFor(app);
  const projectCount = useStore(app.store, (s) => s.projects.length);
  const isLoading = useStore(hub.store, (s) => s.isLoading);
  const error = useStore(hub.store, (s) => s.error);
  const showingCreatePrompt = useStore(hub.store, (s) => s.showingCreatePrompt);
  const showingCreateAgent = useStore(hub.store, (s) => s.showingCreateAgent);
  const showingCreateSwarm = useStore(hub.store, (s) => s.showingCreateSwarm);
  const [activeTab, setActiveTab] = useState<AgentsHubTab>('agents');

  const projectRoots = app.libraryProjectRoots;
  const context: HubContext = {
    hub,
    projectRoots,
    anyProjectRoot: app.activeLocalProjectRoot ?? '',
  };

  // Swift `.task` + `.onChange(of: appState.projects.count)`.
  const rootsKey = projectRoots.join('\n');
  useEffect(() => {
    const roots = rootsKey === '' ? [] : rootsKey.split('\n');
    void (async () => {
      await hub.loadAll(roots);
      await triggers.loadTriggers(roots);
    })();
  }, [hub, triggers, rootsKey, projectCount]);

  let content: ReactNode;
  switch (activeTab) {
    case 'prompts':
      content = <PromptsContent context={context} />;
      break;
    case 'agents':
      content = <AgentsContent context={context} />;
      break;
    case 'swarms':
      content = <SwarmsContent context={context} />;
      break;
    case 'triggers':
      content = <TriggersTab state={triggers} projectRoots={projectRoots} />;
      break;
  }

  return (
    <div className="agents-hub" data-testid="agents-hub">
      <header className="hub-header">
        <Icon name="cpu" size={15} className="hub-secondary" />
        <div className="hub-header-text">
          <h2 className="hub-header-title">Agents</h2>
          <p className="hub-header-subtitle">
            Reusable prompts, command-dialog agents, and multi-worktree swarms.
          </p>
        </div>
        <Badge text="Hub" tint="blue" />
      </header>
      <div className="hub-tab-bar">
        <div className="hub-segmented hub-tabs" role="tablist" aria-label="Agents hub sections">
          {AGENTS_HUB_TABS.map((tab) => (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={tab === activeTab}
              className={tab === activeTab ? 'hub-segment hub-segment-on' : 'hub-segment'}
              onClick={() => {
                setActiveTab(tab);
              }}
            >
              {agentsHubTabTitle(tab)}
            </button>
          ))}
        </div>
      </div>
      {isLoading ? (
        <div className="hub-empty hub-empty-detail" role="status">
          Loading...
        </div>
      ) : (
        <div className="hub-content">
          {error !== null && (
            <InlineErrorBanner
              message={error}
              onDismiss={() => {
                hub.dismissError();
              }}
            />
          )}
          {content}
        </div>
      )}
      {showingCreatePrompt && (
        <PromptCreationSheet
          hub={hub}
          onDismiss={() => {
            hub.setShowingCreate('prompt', false);
          }}
        />
      )}
      {showingCreateAgent && (
        <AgentCreationSheet
          hub={hub}
          onDismiss={() => {
            hub.setShowingCreate('agent', false);
          }}
        />
      )}
      {showingCreateSwarm && (
        <SwarmCreationSheet
          hub={hub}
          onDismiss={() => {
            hub.setShowingCreate('swarm', false);
          }}
        />
      )}
    </div>
  );
}

// MARK: - Spawn feedback

/** A Spawn / Run button and the agents its last press started. */
function LaunchButton({
  title,
  disabled,
  launch,
  prominent = false,
}: {
  title: string;
  disabled: boolean;
  launch: () => Promise<readonly string[]>;
  prominent?: boolean;
}) {
  const [running, setRunning] = useState(false);
  const [started, setStarted] = useState<readonly string[] | null>(null);
  return (
    <>
      {started !== null && started.length > 0 && (
        <span className="hub-spawned" data-testid="hub-spawned">
          Started {started.join(', ')}
        </span>
      )}
      <button
        type="button"
        className={prominent ? 'hub-button hub-button-prominent' : 'hub-button'}
        disabled={disabled || running}
        onClick={() => {
          setRunning(true);
          void launch().then((ids) => {
            setStarted(ids);
            setRunning(false);
          });
        }}
      >
        {title}
      </button>
    </>
  );
}

// MARK: - Prompts

function PromptsContent({ context }: { context: HubContext }) {
  const { hub } = context;
  const prompts = useStore(hub.store, (s) => s.prompts);
  const selectedId = useStore(hub.store, (s) => s.selectedPromptId);
  const selected = useStore(hub.store, selectedPromptOf);
  return (
    <MasterDetail
      title="Prompt library"
      items={prompts}
      selectedId={selectedId}
      onSelect={(id) => {
        hub.selectPrompt(id);
      }}
      onCreate={() => {
        hub.setShowingCreate('prompt', true);
      }}
      createLabel="New prompt"
      emptyListMessage="No prompts yet"
      emptyDetailMessage="Select a prompt or create one to get started."
      row={(prompt) => <PromptListRow prompt={prompt} />}
      detail={
        selected === null ? null : (
          <PromptDetail key={selected.id} prompt={selected} context={context} />
        )
      }
    />
  );
}

function PromptListRow({ prompt }: { prompt: SavedPrompt }) {
  return (
    <>
      <span className="hub-row-line">
        <span className="hub-row-name">{prompt.name}</span>
        <Badge
          text={scopeBadge(prompt.source, prompt.projectName)}
          tint={prompt.source === 'global' ? 'blue' : 'green'}
        />
      </span>
      <span className="hub-row-subtitle">
        {prompt.description === '' ? prompt.agent : prompt.description}
      </span>
    </>
  );
}

/**
 * Swift `promptDetailView` + `syncPromptEditor`. Keyed by prompt, so each selection starts from
 * the prompt's values; a field the user has not touched follows the prompt as its detail loads.
 */
function PromptDetail({ prompt, context }: { prompt: SavedPrompt; context: HubContext }) {
  const { hub, projectRoots, anyProjectRoot } = context;
  const [draft, setDraft] = useState<string | null>(null);
  const [scope, setScope] = useState<PromptScopeChoice>(
    prompt.source === 'global' ? 'global' : 'project',
  );
  const [agent, setAgent] = useState<string | null>(null);
  const [command, setCommand] = useState<string | null>(null);
  const root = prompt.projectRoot ?? anyProjectRoot;

  useEffect(() => {
    void hub.loadPromptDetail(root, prompt.name);
  }, [hub, root, prompt.name]);

  const body = draft ?? prompt.body;
  const promptAgent = agent ?? prompt.agent;
  const promptCommand = command ?? prompt.command ?? '';

  return (
    <div className="hub-detail-scroll" data-testid="prompt-detail">
      <CommandHintBar
        icon="text.alignleft"
        text="Prompts can be stored globally or inside a project, then assigned to agents and swarms."
      />
      <SurfaceCard title="Prompt editor" subtitle="Edit reusable prompt template.">
        <div className="hub-toolbar">
          <Segmented
            label="Scope"
            value={scope}
            options={PROMPT_SCOPE_CHOICES}
            title={PromptScope.title}
            onChange={setScope}
          />
          <span className="hub-spacer" />
          <Select
            label="Agent"
            value={promptAgent}
            options={AgentTypes.withAny.map((t) => ({ value: t, title: t === '' ? 'Any' : t }))}
            onChange={setAgent}
          />
          {promptAgent === 'terminal' && (
            <input
              type="text"
              className="hub-input"
              aria-label="Command"
              placeholder="Command (e.g. npm run dev)"
              value={promptCommand}
              onChange={(event) => {
                setCommand(event.target.value);
              }}
            />
          )}
          <button
            type="button"
            className="hub-button"
            onClick={() => {
              const trimmed = promptCommand.trim();
              void hub.saveTemplate({
                projectRoot: root,
                projectRoots,
                name: prompt.name,
                description: prompt.description,
                agent: promptAgent,
                body,
                scope: PromptScope.wireValue(scope),
                command: promptAgent === 'terminal' && trimmed !== '' ? trimmed : null,
              });
            }}
          >
            Save
          </button>
          <button
            type="button"
            className="hub-button hub-destructive"
            onClick={() => {
              void hub.deleteTemplate({
                projectRoot: root,
                projectRoots,
                name: prompt.name,
                scope: prompt.source,
              });
            }}
          >
            Delete
          </button>
        </div>
        <textarea
          className="hub-textarea hub-editor"
          aria-label="Prompt body"
          value={body}
          spellCheck={false}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
        />
        <div className="hub-row-line">
          <div>
            <SectionLabel>Detected variables</SectionLabel>
            <div className="hub-badges">
              {prompt.variables.map((variable) => (
                <Badge key={variable} text={variable} tint="orange" />
              ))}
            </div>
          </div>
          <span className="hub-secondary hub-small">Source: {prompt.source}</span>
        </div>
      </SurfaceCard>
      <SurfaceCard title="Prompt usage" subtitle="Selected prompt details.">
        <UsageRow title="Agent" value={prompt.agent === '' ? 'Any' : prompt.agent} icon="cpu" />
        <UsageRow
          title="Description"
          value={prompt.description === '' ? 'None' : prompt.description}
          icon="text.alignleft"
        />
        <UsageRow title="Source" value={prompt.source} icon="globe" />
        <div className="hub-actions">
          <span className="hub-secondary hub-small">
            Starts a {prompt.agent === '' ? 'claude' : prompt.agent} agent with this prompt.
          </span>
          <span className="hub-spacer" />
          <LaunchButton
            title="Spawn"
            prominent
            disabled={root === ''}
            launch={async () => {
              const id = await hub.spawnFromPrompt(root, prompt);
              return id === null ? [] : [id];
            }}
          />
        </div>
      </SurfaceCard>
    </div>
  );
}

// MARK: - Agents

function AgentsContent({ context }: { context: HubContext }) {
  const { hub } = context;
  const agents = useStore(hub.store, (s) => s.agents);
  const selectedId = useStore(hub.store, (s) => s.selectedAgentId);
  const selected = useStore(hub.store, selectedAgentOf);
  return (
    <MasterDetail
      title="Custom agents"
      items={agents}
      selectedId={selectedId}
      onSelect={(id) => {
        hub.selectAgent(id);
      }}
      onCreate={() => {
        hub.setShowingCreate('agent', true);
      }}
      createLabel="New agent"
      emptyListMessage="No agents yet"
      emptyDetailMessage="Select an agent or create one to get started."
      row={(agent) => <AgentListRow agent={agent} />}
      detail={selected === null ? null : <AgentDetail agent={selected} context={context} />}
    />
  );
}

function AgentListRow({ agent }: { agent: AgentDefinition }) {
  return (
    <>
      <span className="hub-row-line">
        <Icon name={symbolOr(agent.icon, 'cpu')} size={14} className="hub-secondary" />
        <span className="hub-row-name">{agent.name}</span>
        <Badge
          text={scopeBadge(agent.scope, agent.projectName)}
          tint={agent.scope === 'global' ? 'blue' : 'green'}
        />
      </span>
      <span className="hub-row-subtitle">{agent.agentType}</span>
    </>
  );
}

function AgentDetail({ agent, context }: { agent: AgentDefinition; context: HubContext }) {
  const { hub, projectRoots, anyProjectRoot } = context;
  const root = agent.projectRoot ?? anyProjectRoot;
  return (
    <div className="hub-detail-scroll" data-testid="agent-detail">
      <CommandHintBar icon="command" text="ctrl+n -> @agentname -> spawn agent session" />
      <SurfaceCard title={agent.name} subtitle={`${agent.agentType} agent. Scope: ${agent.scope}.`}>
        <div className="hub-row-line">
          <Icon name={symbolOr(agent.icon, 'cpu')} size={14} className="hub-secondary" />
          {agent.tags.map((tag) => (
            <Badge key={tag} text={tag} tint="purple" />
          ))}
          <Badge text={agent.scope} tint="green" />
          <span className="hub-spacer" />
          <span className="hub-secondary hub-small">
            {agent.availableInCommandDialog ? 'In command dialog' : 'Not in command dialog'}
          </span>
        </div>
        <CodeSection label="Prompt template" content={agent.template} />
        <CodeSection label="Inline prompt" content={agent.inlinePrompt} />
        <CodeSection label="Command" content={agent.command} />
        <div className="hub-actions">
          <span className="hub-spacer" />
          <LaunchButton
            title="Spawn"
            prominent
            disabled={root === ''}
            launch={async () => {
              const id = await hub.spawnAgentDef(root, agent);
              return id === null ? [] : [id];
            }}
          />
          <button
            type="button"
            className="hub-button hub-destructive"
            onClick={() => {
              void hub.deleteAgentDef({
                projectRoot: root,
                projectRoots,
                name: agent.name,
                scope: agent.scope,
              });
            }}
          >
            Delete
          </button>
        </div>
      </SurfaceCard>
    </div>
  );
}

// MARK: - Swarms

function SwarmsContent({ context }: { context: HubContext }) {
  const { hub } = context;
  const swarms = useStore(hub.store, (s) => s.swarms);
  const selectedId = useStore(hub.store, (s) => s.selectedSwarmId);
  const selected = useStore(hub.store, selectedSwarmOf);
  return (
    <MasterDetail
      title="Swarms"
      items={swarms}
      selectedId={selectedId}
      onSelect={(id) => {
        hub.selectSwarm(id);
      }}
      onCreate={() => {
        hub.setShowingCreate('swarm', true);
      }}
      createLabel="New swarm"
      emptyListMessage="No swarms yet"
      emptyDetailMessage="Select a swarm or create one to get started."
      row={(swarm) => <SwarmListRow swarm={swarm} />}
      detail={selected === null ? null : <SwarmDetail swarm={selected} context={context} />}
    />
  );
}

function SwarmListRow({ swarm }: { swarm: SwarmDefinition }) {
  return (
    <>
      <span className="hub-row-line">
        <span className="hub-row-name">{swarm.name}</span>
        <Badge
          text={scopeBadge(swarm.scope, swarm.projectName)}
          tint={swarm.scope === 'global' ? 'blue' : 'green'}
        />
      </span>
      <span className="hub-row-subtitle">
        {swarm.worktreeCount} worktrees · {swarmDefinitionTotalAgents(swarm)} agents
      </span>
    </>
  );
}

function SwarmDetail({ swarm, context }: { swarm: SwarmDefinition; context: HubContext }) {
  const { hub, projectRoots, anyProjectRoot } = context;
  const root = swarm.projectRoot ?? anyProjectRoot;
  return (
    <div className="hub-detail-scroll" data-testid="swarm-detail">
      <CommandHintBar icon="command" text="ctrl+n -> @swarmname -> execute composition" />
      <SurfaceCard
        title={swarm.name}
        subtitle={`${String(swarm.worktreeCount)} worktrees · ${String(swarmDefinitionTotalAgents(swarm))} agents total`}
      >
        <div className="hub-row-line">
          {swarm.worktreeTemplate !== '' && <Badge text={swarm.worktreeTemplate} tint="blue" />}
          <Badge
            text={swarm.includeTerminal ? 'Terminal attached' : 'No terminal'}
            tint={swarm.includeTerminal ? 'green' : 'gray'}
          />
        </div>
        {swarm.roster.length > 0 && (
          <div className="hub-code-section">
            <SectionLabel>Roster</SectionLabel>
            {swarm.roster.map((item) => (
              <div className="hub-roster-item" key={item.id}>
                <span className="hub-row-name">{item.agentDef}</span>
                <span className="hub-secondary hub-small">{item.role}</span>
                <span className="hub-spacer" />
                <span className="hub-secondary hub-small">x{item.quantity}</span>
              </div>
            ))}
          </div>
        )}
        <div className="hub-code-section">
          <SectionLabel>Composition summary</SectionLabel>
          <SwarmDiagram swarm={swarm} />
        </div>
        <div className="hub-actions">
          <span className="hub-spacer" />
          <LaunchButton
            title="Run"
            prominent
            disabled={root === ''}
            launch={() => hub.runSwarm(root, swarm.name)}
          />
          <button
            type="button"
            className="hub-button hub-destructive"
            onClick={() => {
              void hub.deleteSwarmDef({
                projectRoot: root,
                projectRoots,
                name: swarm.name,
                scope: swarm.scope,
              });
            }}
          >
            Delete
          </button>
        </div>
      </SurfaceCard>
    </div>
  );
}

/** Swift `SwarmDiagramView`: one column per worktree, each listing the roster. */
function SwarmDiagram({ swarm }: { swarm: SwarmDefinition }) {
  const count = Math.max(swarm.worktreeCount, 1);
  return (
    <div className="hub-diagram">
      {Array.from({ length: count }, (_, i) => i + 1).map((index) => (
        <div className="hub-diagram-column" key={index}>
          <span className="hub-diagram-title">Worktree {index}</span>
          <span className="hub-diagram-template">
            {swarm.worktreeTemplate.replaceAll('{index}', String(index))}
          </span>
          {swarm.roster.map((item) => (
            <span className="hub-diagram-agent" key={item.id}>
              <Icon name="cpu" size={11} className="hub-secondary" />
              {item.agentDef} x{item.quantity}
            </span>
          ))}
          {swarm.includeTerminal && (
            <>
              <hr className="hub-divider" />
              <span className="hub-secondary hub-small">Terminal panel</span>
            </>
          )}
        </div>
      ))}
    </div>
  );
}
