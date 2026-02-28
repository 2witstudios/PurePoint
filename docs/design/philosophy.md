# Design Philosophy

## Why Design Matters

PurePoint is a developer tool, but developer tools don't have to look like they were built in 1995. A beautiful, well-crafted interface reduces cognitive load, builds trust, and makes complex operations feel manageable.

## Principles

### Content Over Chrome
The terminal output, the agent status, the diff view — these are what matter. Everything else (sidebar, toolbar, frames) should disappear into the background.

### Native Feels Right
On macOS, PurePoint should feel like a first-party Apple app. Native controls, system fonts, respecting dark mode, following HIG conventions. Users shouldn't have to learn a new interaction model.

### Information Density Without Clutter
Developers want to see a lot of information at once (multiple terminals, status indicators, project tree). The challenge is presenting this density without it feeling cluttered. The pane grid system is the key affordance.

### Motion Communicates State
Animation is not decoration — it's communication. A terminal pane sliding into view tells the user where it came from. A status indicator pulsing tells them something is active. Every motion should answer "what just changed?"
