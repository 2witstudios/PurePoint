// Port of apps/purepoint-macos/purepoint-macos/Views/DaemonErrorBanner.swift (WIN-7.1):
// DaemonErrorBanner, InlineErrorBanner, GHUnavailableView and OverlayButton, plus the shell's
// banner stack — the daemon error from AppState and, new on Windows, the first-launch installer's
// failure (the Mac installs its CLI silently; WIN-10.1 reports failures as `installer-state`).
import { Icon } from '../../theme/Icon.tsx';
import type { SfSymbolName } from '../../theme/icons.ts';

export interface DaemonErrorBannerProps {
  message: string;
  onDismiss: () => void;
  /** An extra action (the installer's Retry). */
  action?: { label: string; onClick: () => void } | undefined;
}

export function DaemonErrorBanner({ message, onDismiss, action }: DaemonErrorBannerProps) {
  return (
    <div className="daemon-error-banner" role="alert">
      <Icon name="exclamationmark.triangle.fill" className="daemon-error-banner-icon" />
      <span className="daemon-error-banner-message">{message}</span>
      {action !== undefined && (
        <button type="button" className="banner-text-button" onClick={action.onClick}>
          {action.label}
        </button>
      )}
      <button
        type="button"
        className="banner-icon-button"
        aria-label="Dismiss"
        title="Dismiss"
        onClick={onDismiss}
      >
        <Icon name="xmark" />
      </button>
    </div>
  );
}

export function InlineErrorBanner({
  message,
  onDismiss,
}: {
  message: string;
  onDismiss: () => void;
}) {
  return (
    <div className="inline-error-banner" role="alert">
      <Icon name="exclamationmark.triangle.fill" className="inline-error-banner-icon" />
      <span className="inline-error-banner-message">{message}</span>
      <button type="button" className="banner-text-button" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}

/** Shown where a PR diff would be when `gh` is missing; the Windows install is winget. */
export function GHUnavailableView() {
  return (
    <div className="gh-unavailable">
      <Icon name="terminal" size={28} />
      <span className="gh-unavailable-title">Install GitHub CLI for PR diffs</span>
      <code className="gh-unavailable-command">
        Run `winget install GitHub.cli` then `gh auth login`
      </code>
    </div>
  );
}

export function OverlayButton({
  icon,
  tooltip,
  onClick,
}: {
  icon: SfSymbolName;
  tooltip: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="overlay-button"
      title={tooltip}
      aria-label={tooltip}
      onClick={onClick}
    >
      <Icon name={icon} size={11} />
    </button>
  );
}
