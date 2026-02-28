# Design Rules

Imperative instructions for UI/UX design in PurePoint.

## Design Principles

- Use the existing project design system and components
- Create intuitive, accessible, and visually appealing interfaces
- Subtle but delightful motion design — satisfying, never distracting
- Every interaction should feel immediate and responsive

## Desktop App Conventions (macOS/AppKit)

- Follow Apple Human Interface Guidelines for macOS
- Use native controls where possible (NSOutlineView, NSTabView, NSSplitView)
- SwiftTerm for terminal views — no custom terminal implementations
- Respect system appearance (light/dark mode via Theme.swift)
- Support keyboard shortcuts for all common actions
- Command palette for discoverability (CommandPalettePanel)

## Visual Design

- Clean, minimal chrome — content over decoration
- Consistent spacing and alignment
- Typography follows system fonts (SF Pro for macOS)
- Color used intentionally — status indicators, not decoration
- Agent status colors must be consistent across sidebar, dashboard, and grid

## Motion Design

- Transitions should be subtle and fast (150-250ms)
- Use easing curves that feel natural (ease-out for entrances, ease-in for exits)
- Never animate for animation's sake — every motion should communicate state change
- Loading states should be clear but not attention-grabbing

## Accessibility

- All interactive elements must be keyboard-navigable
- VoiceOver labels on all custom controls
- Sufficient color contrast (WCAG AA minimum)
- No information conveyed by color alone — use shapes/icons as supplements
