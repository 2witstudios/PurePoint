// Building blocks the Agents hub and its sheets share (WIN-8.7): the Mac's private helpers in
// Views/Agents/AgentsHubView.swift (`selectableRow`, `CommandHintBar`, `masterDetail`,
// `listPanel`, `codeSection`, `usageRow`), its `MockBadge` / `MockSurfaceCard`, and the SwiftUI
// sheet, segmented picker and stepper controls they use.
import { useEffect, useId, useRef, type ReactNode } from 'react';
import { Icon } from '../../theme/Icon.tsx';
import { SF_SYMBOLS, type SfSymbolName } from '../../theme/icons.ts';

export type BadgeTint = 'blue' | 'green' | 'orange' | 'purple' | 'gray';

/** Swift `MockBadge`. */
export function Badge({ text, tint }: { text: string; tint: BadgeTint }) {
  return <span className={`hub-badge hub-badge-${tint}`}>{text}</span>;
}

/** An SF Symbol a definition names, or `fallback` when this app has no icon for it. */
export function symbolOr(name: string | null, fallback: SfSymbolName): SfSymbolName {
  return name !== null && Object.hasOwn(SF_SYMBOLS, name) ? (name as SfSymbolName) : fallback;
}

/** Swift `CommandHintBar`. */
export function CommandHintBar({ icon, text }: { icon: SfSymbolName; text: string }) {
  return (
    <div className="hub-hint">
      <Icon name={icon} size={14} />
      <span>{text}</span>
    </div>
  );
}

/** Swift `MockSurfaceCard`. */
export function SurfaceCard({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  return (
    <section className="hub-card" aria-label={title}>
      <header className="hub-card-header">
        <h3 className="hub-card-title">{title}</h3>
        <p className="hub-card-subtitle">{subtitle}</p>
      </header>
      <div className="hub-card-body">{children}</div>
    </section>
  );
}

export function SectionLabel({ children }: { children: ReactNode }) {
  return <div className="hub-section-label">{children}</div>;
}

/** Swift `codeSection`: nothing when the content is empty. */
export function CodeSection({ label, content }: { label: string; content: string | null }) {
  if (content === null || content === '') return null;
  return (
    <div className="hub-code-section">
      <SectionLabel>{label}</SectionLabel>
      <pre className="hub-code">{content}</pre>
    </div>
  );
}

/** Swift `usageRow`. */
export function UsageRow({
  title,
  value,
  icon,
}: {
  title: string;
  value: string;
  icon: SfSymbolName;
}) {
  return (
    <div className="hub-usage-row">
      <Icon name={icon} size={14} className="hub-secondary" />
      <span>{title}</span>
      <span className="hub-usage-value">{value}</span>
    </div>
  );
}

/** Swift `masterDetail` + `listPanel` + `emptyListState` / `emptyDetailState`. */
export function MasterDetail<Item extends { id: string }>({
  title,
  items,
  selectedId,
  onSelect,
  onCreate,
  createLabel,
  emptyListMessage,
  emptyDetailMessage,
  row,
  detail,
}: {
  title: string;
  items: readonly Item[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  createLabel: string;
  emptyListMessage: string;
  emptyDetailMessage: string;
  row: (item: Item, selected: boolean) => ReactNode;
  detail: ReactNode | null;
}) {
  return (
    <div className="hub-master-detail">
      <div className="hub-list" aria-label={title}>
        <div className="hub-list-header">
          <span className="hub-list-title">{title}</span>
          <button
            type="button"
            className="hub-icon-button"
            aria-label={createLabel}
            title={createLabel}
            onClick={onCreate}
          >
            <Icon name="plus" size={14} />
          </button>
        </div>
        {items.length === 0 ? (
          <div className="hub-empty">
            <span>{emptyListMessage}</span>
            <button type="button" className="hub-button" onClick={onCreate}>
              Create
            </button>
          </div>
        ) : (
          <ul className="hub-list-items" role="listbox" aria-label={title}>
            {items.map((item) => {
              const selected = item.id === selectedId;
              return (
                <li key={item.id} role="presentation">
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    className={selected ? 'hub-row hub-row-selected' : 'hub-row'}
                    data-testid="hub-row"
                    data-id={item.id}
                    onClick={() => {
                      onSelect(item.id);
                    }}
                  >
                    {row(item, selected)}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      <div className="hub-detail">
        {detail ?? <div className="hub-empty hub-empty-detail">{emptyDetailMessage}</div>}
      </div>
    </div>
  );
}

/** A SwiftUI segmented `Picker`. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  title,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly T[];
  title: (option: T) => string;
  onChange: (value: T) => void;
}) {
  return (
    <div className="hub-segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={option === value}
          className={option === value ? 'hub-segment hub-segment-on' : 'hub-segment'}
          onClick={() => {
            onChange(option);
          }}
        >
          {title(option)}
        </button>
      ))}
    </div>
  );
}

/** A labelled form row, as a grouped SwiftUI `Form` lays out its controls. */
export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="hub-field">
      <span className="hub-field-label">{label}</span>
      <div className="hub-field-control">{children}</div>
    </div>
  );
}

/** A labelled text field (SwiftUI `TextField(_:text:)`, whose title is its placeholder). */
export function TextInput({
  label,
  value,
  onChange,
  placeholder,
  className,
  list,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  className?: string;
  list?: string;
}) {
  return (
    <input
      type="text"
      className={className === undefined ? 'hub-input' : `hub-input ${className}`}
      aria-label={label}
      placeholder={placeholder ?? label}
      value={value}
      spellCheck={false}
      {...(list === undefined ? {} : { list })}
      onChange={(event) => {
        onChange(event.target.value);
      }}
    />
  );
}

/** SwiftUI `Stepper(value:in:)`. */
export function Stepper({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  const clamp = (next: number) => Math.min(max, Math.max(min, next));
  return (
    <span className="hub-stepper-group">
      <input
        type="number"
        className="hub-input hub-stepper"
        aria-label={label}
        min={min}
        max={max}
        value={value}
        onChange={(event) => {
          const next = Number.parseInt(event.target.value, 10);
          if (Number.isFinite(next)) onChange(clamp(next));
        }}
      />
      <button
        type="button"
        className="hub-icon-button"
        aria-label={`Decrease ${label}`}
        disabled={value <= min}
        onClick={() => {
          onChange(clamp(value - 1));
        }}
      >
        −
      </button>
      <button
        type="button"
        className="hub-icon-button"
        aria-label={`Increase ${label}`}
        disabled={value >= max}
        onClick={() => {
          onChange(clamp(value + 1));
        }}
      >
        +
      </button>
    </span>
  );
}

/** A SwiftUI switch `Toggle`. */
export function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="hub-toggle">
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
      <span>{label}</span>
    </label>
  );
}

/** A SwiftUI `Picker` in menu style. */
export function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly { value: string; title: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <select
      className="hub-input hub-select"
      aria-label={label}
      value={value}
      onChange={(event) => {
        onChange(event.target.value);
      }}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.title}
        </option>
      ))}
    </select>
  );
}

/**
 * A `.sheet`: a modal panel with a title, a form and a Cancel / Create footer. Escape cancels;
 * Enter in a text field submits while `canSubmit` (the default action's keyboard shortcut).
 */
export function Sheet({
  title,
  canSubmit,
  submitTitle = 'Create',
  onCancel,
  onSubmit,
  footerNote,
  children,
}: {
  title: string;
  canSubmit: boolean;
  submitTitle?: string;
  onCancel: () => void;
  onSubmit: () => void;
  footerNote?: ReactNode;
  children: ReactNode;
}) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    panel.current?.querySelector<HTMLElement>('input, textarea, select')?.focus();
  }, []);
  return (
    <div className="hub-sheet-backdrop">
      <div
        className="hub-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        ref={panel}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onCancel();
          } else if (
            event.key === 'Enter' &&
            event.target instanceof HTMLInputElement &&
            event.target.type !== 'checkbox' &&
            canSubmit
          ) {
            event.preventDefault();
            onSubmit();
          }
        }}
      >
        <h2 className="hub-sheet-title" id={titleId}>
          {title}
        </h2>
        <div className="hub-sheet-form">{children}</div>
        {footerNote}
        <div className="hub-sheet-footer">
          <button type="button" className="hub-button" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className="hub-button hub-button-prominent"
            disabled={!canSubmit}
            onClick={onSubmit}
          >
            {submitTitle}
          </button>
        </div>
      </div>
    </div>
  );
}

/** A project `Picker`: the app's projects by name. */
export function ProjectPicker({
  projects,
  value,
  onChange,
}: {
  projects: readonly { projectRoot: string; projectName: string }[];
  value: string;
  onChange: (root: string) => void;
}) {
  return (
    <Field label="Project">
      <Select
        label="Project"
        value={value}
        options={projects.map((p) => ({ value: p.projectRoot, title: p.projectName }))}
        onChange={onChange}
      />
    </Field>
  );
}
