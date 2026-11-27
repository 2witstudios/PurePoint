// Port of apps/purepoint-macos/purepoint-macos/Views/CommandPalette/CommandPalettePanel.swift
// (WIN-7.5): the floating command palette. A 500px rounded panel over the window, centred and
// 60px above the middle at its full height (keeping its top edge as it resizes), that lists the
// built-in variants, agent definitions, swarms and files, filters them as you type, and walks
// the selection → naming → prompt flow (model/commandPaletteFlow.ts) before handing back a
// `CommandPaletteResult`. A mouse-down outside it dismisses it.
//
// Differences from Swift:
// - The panel is an overlay in the window (`CommandPaletteHost`, rendered by ContentView) rather
//   than a child `NSPanel`; the vibrancy material is the theme's menu background.
// - The panel sizes to its content in CSS (header 57px, 44px rows, 8px bottom inset, at most
//   380px) instead of `resizePanelHeight`; the prompt field grows from 60px to 120px.
// - With an empty query the list never shows file rows, including right after the items load
//   (Swift's `setItems` shows them all until the first keystroke).
import { type KeyboardEvent, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import {
  type CommandPaletteItem,
  type CommandPaletteResult,
  displayName,
  itemId,
  promptPlaceholder,
} from '../../model/commandPalette.ts';
import {
  CommandPaletteFlow,
  type CommandPalettePhase,
  NAME_HINT,
} from '../../model/commandPaletteFlow.ts';
import { normalizeWorktreeName } from '../../model/worktreeName.ts';
import { Icon } from '../../theme/Icon.tsx';
import { CommandPaletteRowView, paletteRowId } from './CommandPaletteRowView.tsx';
import { filterPaletteItems, paletteIcon } from './paletteFilter.ts';
import { dismissPalette, usePaletteStore } from './paletteStore.ts';
import './palette.css';

export const PROMPT_HINT = 'Enter to submit · Shift+Enter for newline';
const PROMPT_MIN_HEIGHT = 60;
const PROMPT_MAX_HEIGHT = 120;

export interface CommandPalettePanelProps {
  items: readonly CommandPaletteItem[];
  usePointGuardNames: boolean;
  initialItem?: CommandPaletteItem | null;
  onSelect: (result: CommandPaletteResult) => void;
  onDismiss: () => void;
}

/** Renders the presented palette, if any (Swift `CommandPalettePanel.current`). */
export function CommandPaletteHost() {
  const presentation = usePaletteStore((state) => state.presentation);
  if (presentation === null) return null;
  return (
    <CommandPalettePanel
      key={presentation.id}
      items={presentation.items}
      usePointGuardNames={presentation.usePointGuardNames}
      initialItem={presentation.initialItem}
      onSelect={presentation.onSelect}
      onDismiss={dismissPalette}
    />
  );
}

export function CommandPalettePanel({
  items,
  usePointGuardNames,
  initialItem = null,
  onSelect,
  onDismiss,
}: CommandPalettePanelProps) {
  // A presentation's handlers and naming preference are fixed, so the first render's are kept.
  const [flow] = useState(() => createFlow(usePointGuardNames, onSelect, onDismiss));
  const [view, setView] = useState(() => flowView(flow));

  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(() =>
    filterPaletteItems(items, '').length > 0 ? 0 : -1,
  );
  const filtered = filterPaletteItems(items, query);
  const idPrefix = useId();

  const searchRef = useRef<HTMLInputElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const phase = view.phase;
  const phaseKey = phase.kind === 'selection' ? 'selection' : `${phase.kind}:${itemId(phase.item)}`;

  /** Runs a flow transition, then shows its result; back in selection the list starts over. */
  const run = (step: () => void) => {
    const before = flow.phase.kind;
    step();
    if (flow.phase.kind === 'selection' && before !== 'selection') {
      setQuery('');
      setSelectedIndex(filterPaletteItems(items, '').length > 0 ? 0 : -1);
    }
    setView(flowView(flow));
  };

  // `show(initialItem:)` selects the item right after the palette appears.
  const initialSelected = useRef(false);
  useEffect(() => {
    if (initialSelected.current || initialItem === null) return;
    initialSelected.current = true;
    run(() => {
      flow.selectItem(initialItem);
    });
    // Once, on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Focus follows the phase; on close it goes back where it was.
  const [previousFocus] = useState(() =>
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  useEffect(
    () => () => {
      if (previousFocus?.isConnected === true) previousFocus.focus();
    },
    [previousFocus],
  );
  useLayoutEffect(() => {
    if (phase.kind === 'selection') searchRef.current?.focus();
    else if (view.nameVisible) nameRef.current?.focus();
    else promptRef.current?.focus();
    // Only a phase change moves focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phaseKey]);

  // Keep the highlighted row on screen (Swift `scrollRowToVisible`).
  useLayoutEffect(() => {
    if (selectedIndex < 0) return;
    const row = listRef.current?.children[selectedIndex];
    if (row instanceof HTMLElement) row.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex]);

  const chooseRow = (index: number) => {
    const item = filtered[index];
    if (item === undefined) return;
    run(() => {
      flow.selectItem(item);
    });
  };

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    switch (event.key) {
      case 'ArrowUp':
        event.preventDefault();
        if (selectedIndex > 0) setSelectedIndex(selectedIndex - 1);
        return;
      case 'ArrowDown':
        event.preventDefault();
        if (selectedIndex < filtered.length - 1) setSelectedIndex(selectedIndex + 1);
        return;
      case 'Enter':
        event.preventDefault();
        if (selectedIndex >= 0) chooseRow(selectedIndex);
        return;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        run(() => {
          flow.handleEscape();
        });
        return;
    }
  };

  const onNameKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    switch (event.key) {
      case 'Enter':
        event.preventDefault();
        if (flow.phase.kind === 'prompt' && flow.isPromptFieldVisible) {
          promptRef.current?.focus();
        } else {
          run(() => {
            flow.enterName();
          });
        }
        return;
      case 'Tab':
        event.preventDefault();
        if (flow.isPromptFieldVisible) promptRef.current?.focus();
        return;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        run(() => {
          flow.handleEscape();
        });
        return;
    }
  };

  const onPromptKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      run(() => {
        flow.handleEscape();
      });
      return;
    }
    if (event.key !== 'Enter') return;
    if (event.shiftKey) return; // The textarea inserts the newline.
    event.preventDefault();
    if (event.altKey) {
      // Swift inserts a newline for Option+Enter too; a textarea does not on its own.
      const field = event.currentTarget;
      const { selectionStart, selectionEnd, value } = field;
      field.value = `${value.slice(0, selectionStart)}\n${value.slice(selectionEnd)}`;
      field.selectionStart = field.selectionEnd = selectionStart + 1;
      setFlowText(flow, 'prompt', field.value);
      fitPrompt(field);
      setView(flowView(flow));
      return;
    }
    run(() => {
      flow.submitPrompt();
    });
  };

  const header =
    phase.kind === 'selection' ? null : (
      <div className="palette-item-header" data-testid="palette-item-header">
        <Icon name={paletteIcon(phase.item)} size={18} />
        <span className="palette-item-title">
          {phase.kind === 'naming' ? `Name ${displayName(phase.item)}` : displayName(phase.item)}
        </span>
      </div>
    );

  const slug = normalizeWorktreeName(view.name);
  const nameNote = phase.kind === 'naming' ? view.nameHint : slug === '' ? ' ' : `pu/${slug}`;

  return (
    <div
      className="palette-backdrop"
      data-testid="palette-backdrop"
      onPointerDown={(event) => {
        if (event.target !== event.currentTarget) return;
        // Swift's mouse-down monitor swallows the click that dismisses the panel.
        event.preventDefault();
        onDismiss();
      }}
    >
      <div
        className="palette-panel"
        role="dialog"
        aria-label="Command Palette"
        data-testid="command-palette"
        data-phase={phase.kind}
      >
        {phase.kind === 'selection' ? (
          <>
            <input
              ref={searchRef}
              className="palette-search"
              type="text"
              role="combobox"
              aria-label="Filter"
              aria-expanded="true"
              aria-controls={`${idPrefix}-list`}
              aria-activedescendant={
                selectedIndex >= 0 ? paletteRowId(idPrefix, selectedIndex) : undefined
              }
              placeholder="Type to filter..."
              autoComplete="off"
              spellCheck={false}
              value={query}
              onChange={(event) => {
                const next = event.target.value;
                setQuery(next);
                setSelectedIndex(filterPaletteItems(items, next).length > 0 ? 0 : -1);
              }}
              onKeyDown={onSearchKeyDown}
            />
            <div className="palette-separator" />
            <div
              ref={listRef}
              id={`${idPrefix}-list`}
              className="palette-list"
              role="listbox"
              aria-label="Items"
            >
              {filtered.map((item, index) => (
                <CommandPaletteRowView
                  key={itemId(item)}
                  item={item}
                  index={index}
                  idPrefix={idPrefix}
                  isHighlighted={index === selectedIndex}
                  onHighlight={() => {
                    setSelectedIndex(index);
                  }}
                  onChoose={() => {
                    setSelectedIndex(index);
                    chooseRow(index);
                  }}
                />
              ))}
            </div>
          </>
        ) : (
          <div className="palette-item-phase">
            {header}
            {view.nameVisible && (
              <div className="palette-name">
                <label className="palette-name-label" htmlFor={`${idPrefix}-name`}>
                  Name
                </label>
                <input
                  ref={nameRef}
                  id={`${idPrefix}-name`}
                  className="palette-name-field"
                  type="text"
                  autoComplete="off"
                  spellCheck={false}
                  placeholder={
                    phase.kind === 'naming' ? 'e.g., Fix authentication' : 'e.g., fix-auth-bug'
                  }
                  value={view.name}
                  onChange={(event) => {
                    setFlowText(flow, 'name', event.target.value);
                    if (flow.phase.kind === 'naming') setFlowText(flow, 'nameHint', NAME_HINT);
                    setView(flowView(flow));
                  }}
                  onKeyDown={onNameKeyDown}
                />
                <span className="palette-name-note" data-testid="palette-name-note">
                  {nameNote}
                </span>
              </div>
            )}
            {view.promptVisible && (
              <>
                <textarea
                  ref={promptRef}
                  className="palette-prompt"
                  aria-label="Prompt"
                  placeholder={promptPlaceholder(phase.item)}
                  spellCheck={false}
                  value={view.prompt}
                  onChange={(event) => {
                    setFlowText(flow, 'prompt', event.target.value);
                    fitPrompt(event.target);
                    setView(flowView(flow));
                  }}
                  onKeyDown={onPromptKeyDown}
                />
                <span className="palette-prompt-hint">{PROMPT_HINT}</span>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** Swift `adjustPromptHeight`: the field grows with its text from 60px to 120px. */
function fitPrompt(field: HTMLTextAreaElement): void {
  field.style.height = `${String(PROMPT_MIN_HEIGHT)}px`;
  const height = Math.min(Math.max(field.scrollHeight + 4, PROMPT_MIN_HEIGHT), PROMPT_MAX_HEIGHT);
  field.style.height = `${String(height)}px`;
}

function createFlow(
  usePointGuardNames: boolean,
  onSelect: (result: CommandPaletteResult) => void,
  onDismiss: () => void,
): CommandPaletteFlow {
  const flow = new CommandPaletteFlow();
  flow.usePointGuardNames = usePointGuardNames;
  flow.onSelect = onSelect;
  flow.onDismiss = onDismiss;
  return flow;
}

/** What the view renders of the flow, copied after each change so render reads only state. */
interface FlowView {
  readonly phase: CommandPalettePhase;
  readonly name: string;
  readonly prompt: string;
  readonly nameHint: string;
  readonly nameVisible: boolean;
  readonly promptVisible: boolean;
}

function flowView(flow: CommandPaletteFlow): FlowView {
  return {
    phase: flow.phase,
    name: flow.name,
    prompt: flow.prompt,
    nameHint: flow.nameHint,
    nameVisible: flow.isNameFieldVisible,
    promptVisible: flow.isPromptFieldVisible,
  };
}

/** The field texts the view binds; the flow owns them, as the Mac's view controller does. */
function setFlowText(
  flow: CommandPaletteFlow,
  field: 'name' | 'prompt' | 'nameHint',
  value: string,
): void {
  flow[field] = value;
}
