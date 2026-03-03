# Theme System — Dynamic Theme System

## User Story

As a user, I want to switch between light, dark, and system-matched themes so that PurePoint matches my visual preferences and adapts to my environment.

## Feature Description

Theme switching with Light/Dark/System options, a comprehensive color palette covering chrome, content, status, separators, and accents. Terminal themes that match the app theme. Real-time switching without restart.

## How ppg-cli Did It

Light/Dark/System appearance switching with a full color palette defined as constants (chrome background, content background, status bar, separator, accent colors). Real-time switching via UserDefaults observation. Terminal colors matched the selected theme.

What worked well: System-following mode was the sensible default. Having a complete color palette meant consistent styling across all views. Real-time switching gave immediate feedback.

## PurePoint Opportunity

- **Native SwiftUI theming**: Use `.preferredColorScheme` and SwiftUI's built-in dark mode support for automatic light/dark adaptation.
- **Daemon-independent**: Theming is a pure UI concern — no daemon involvement needed, keeping the architecture clean.
- **Custom accent colors**: Let users pick accent colors beyond the default palette.
- **Terminal theme presets**: Offer popular terminal color schemes (Solarized, Dracula, One Dark, etc.) that apply to SwiftTerm.
- **Per-agent themes**: Optional per-agent terminal themes for visual differentiation in split views.

## Priority

**P2** — Visual polish and user comfort. The current system-only following works but lacks user control.
