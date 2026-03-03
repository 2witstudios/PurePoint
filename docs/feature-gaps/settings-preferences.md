# Settings & Preferences — Settings & Preferences Panel

## User Story

As a user, I want a settings panel where I can configure appearance, terminal behavior, and keyboard shortcuts so that I can customize PurePoint to match my workflow and visual preferences.

## Feature Description

A multi-section settings panel covering display preferences (theme, font, layout), terminal configuration (shell, font size, scrollback), and keyboard shortcut customization. Settings persist across sessions and apply immediately without restart.

## How ppg-cli Did It

Three-tab settings window (Display / Terminal / Shortcuts) with theme switching (Light/Dark/System), font size and family selection, shell configuration, and a keybinding editor with conflict detection. Settings stored in UserDefaults. Changes applied immediately to all open views.

What worked well: Immediate feedback on theme/font changes. Conflict detection in the keybinding editor prevented broken shortcuts. Three clear sections covered the major customization needs.

## PurePoint Opportunity

- **SwiftUI Settings scene**: Use SwiftUI's native `Settings` scene for macOS-standard Preferences window with proper keyboard shortcut (Cmd+,).
- **Daemon-backed config persistence**: Store settings in `.pu/config.toml` so CLI and GUI share the same configuration. Daemon watches for changes and notifies connected clients.
- **Terminal settings via IPC**: Font size, theme, and shell changes flow through the daemon to active terminal sessions via resize/theme IPC commands.
- **Layered config**: Project-level settings override user-level defaults, matching the `.pu/` convention.

## Priority

**P2** — Users expect a working settings panel. The non-functional gear icon is a visible gap that undermines perceived quality.
