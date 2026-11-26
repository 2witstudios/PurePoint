// Swift `SettingsGroupBoxStyle` (Views/Settings/SettingsView.swift): a caption over a rounded card.
import type { ReactNode } from 'react';

export function SettingsGroup({
  label,
  children,
  testId,
}: {
  label?: ReactNode;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section className="settings-group" data-testid={testId}>
      {label !== undefined && <div className="settings-group-label">{label}</div>}
      <div className="settings-group-card">{children}</div>
    </section>
  );
}

/** A labelled switch row (SwiftUI `Toggle`). */
export function SettingsToggle({
  label,
  checked,
  disabled = false,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="settings-toggle">
      <span>{label}</span>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
    </label>
  );
}

export function SettingsCaption({ children }: { children: ReactNode }) {
  return <p className="settings-caption">{children}</p>;
}
