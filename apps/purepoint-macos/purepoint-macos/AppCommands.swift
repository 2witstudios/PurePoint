#if os(macOS)
import AppKit
import Sparkle
#endif
import SwiftUI

/// The menu bar commands, shared by the Mac and iPad Apps. On iPadOS they appear in the menu
/// bar and as hardware-keyboard shortcuts. The Mac-only items (the `NSOpenPanel` Open Project
/// and Check for Updates) are guarded; on iOS, Open Project and the palette go through
/// `AppLifecycleHooks`.
struct AppCommands: Commands {
    let appState: AppState
    let settingsState: SettingsState
    let registry: WorkspaceRegistry
    let keyBindingState: KeyBindingState
    #if os(macOS)
    @ObservedObject var updaterViewModel: CheckForUpdatesViewModel
    #endif

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Agent\u{2026}") {
                showNewAgentPalette()
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .newAgent),
                modifiers: keyBindingState.eventModifiers(for: .newAgent)
            )

            Button("Open Project\u{2026}") {
                #if os(macOS)
                openProjectWithPicker()
                #else
                AppLifecycleHooks.openProject?()
                #endif
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .openProject),
                modifiers: keyBindingState.eventModifiers(for: .openProject)
            )
        }

        // Both sidebars, in the View menu. Each also has a toggle button in the window's top strip.
        CommandGroup(after: .sidebar) {
            Button("Toggle Sidebar") {
                SidebarVisibility.toggle(for: .toggleSidebar)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .toggleSidebar),
                modifiers: keyBindingState.eventModifiers(for: .toggleSidebar)
            )

            Button("Toggle Workspace Sidebar") {
                SidebarVisibility.toggle(for: .toggleWorkspaceSidebar)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .toggleWorkspaceSidebar),
                modifiers: keyBindingState.eventModifiers(for: .toggleWorkspaceSidebar)
            )
        }

        CommandGroup(replacing: .appSettings) {
            Button("Settings\u{2026}") {
                appState.showSettings = true
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .settings),
                modifiers: keyBindingState.eventModifiers(for: .settings)
            )

            #if os(macOS)
            Divider()

            Button("Check for Updates\u{2026}") {
                updaterViewModel.checkForUpdates()
            }
            .disabled(!updaterViewModel.canCheckForUpdates)
            #endif
        }

        CommandMenu("Navigation") {
            Button("Focus Sidebar") {
                postHotkeyAction(.focusSidebar)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusSidebar),
                modifiers: keyBindingState.eventModifiers(for: .focusSidebar)
            )

            Button("Focus Content") {
                postHotkeyAction(.focusContent)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusContent),
                modifiers: keyBindingState.eventModifiers(for: .focusContent)
            )

            Divider()

            Button("Dashboard") {
                postHotkeyAction(.navDashboard)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .navDashboard),
                modifiers: keyBindingState.eventModifiers(for: .navDashboard)
            )

            Button("Agents") {
                postHotkeyAction(.navAgents)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .navAgents),
                modifiers: keyBindingState.eventModifiers(for: .navAgents)
            )

            Button("Schedule") {
                postHotkeyAction(.navSchedule)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .navSchedule),
                modifiers: keyBindingState.eventModifiers(for: .navSchedule)
            )

            Divider()

            Button("Close Tab") {
                postHotkeyAction(.closeAgent)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .closeAgent),
                modifiers: keyBindingState.eventModifiers(for: .closeAgent)
            )
            .disabled(registry.activeWorkspace == nil)
        }

        CommandMenu("Panes") {
            Button("Split Below") {
                splitFocusedPane(axis: .horizontal)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .splitBelow),
                modifiers: keyBindingState.eventModifiers(for: .splitBelow)
            )
            .disabled(!canSplitBelow)

            Button("Split Right") {
                splitFocusedPane(axis: .vertical)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .splitRight),
                modifiers: keyBindingState.eventModifiers(for: .splitRight)
            )
            .disabled(!canSplitRight)

            Button("Close Pane") {
                closeFocusedPane()
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .closePane),
                modifiers: keyBindingState.eventModifiers(for: .closePane)
            )
            .disabled((registry.activeWorkspace?.paneCount ?? 0) <= 1)

            Divider()

            Button("Focus Up") {
                moveFocus(.up)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusUp),
                modifiers: keyBindingState.eventModifiers(for: .focusUp)
            )
            .disabled((registry.activeWorkspace?.paneCount ?? 0) <= 1)

            Button("Focus Down") {
                moveFocus(.down)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusDown),
                modifiers: keyBindingState.eventModifiers(for: .focusDown)
            )
            .disabled((registry.activeWorkspace?.paneCount ?? 0) <= 1)

            Button("Focus Left") {
                moveFocus(.left)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusLeft),
                modifiers: keyBindingState.eventModifiers(for: .focusLeft)
            )
            .disabled((registry.activeWorkspace?.paneCount ?? 0) <= 1)

            Button("Focus Right") {
                moveFocus(.right)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .focusRight),
                modifiers: keyBindingState.eventModifiers(for: .focusRight)
            )
            .disabled((registry.activeWorkspace?.paneCount ?? 0) <= 1)
        }

        CommandMenu("Tabs") {
            Button("New Tab") {
                newTabInFocusedPane()
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .newTab),
                modifiers: keyBindingState.eventModifiers(for: .newTab)
            )
            .disabled(registry.activeWorkspace == nil)

            Divider()

            Button("Next Tab") {
                cycleFocusedPaneTab(by: 1)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .nextTab),
                modifiers: keyBindingState.eventModifiers(for: .nextTab)
            )
            .disabled(!focusedPaneHasTabs)

            Button("Previous Tab") {
                cycleFocusedPaneTab(by: -1)
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .previousTab),
                modifiers: keyBindingState.eventModifiers(for: .previousTab)
            )
            .disabled(!focusedPaneHasTabs)

            ForEach(HotkeyAction.selectTabActions, id: \.self) { action in
                Button(action.displayName) {
                    selectFocusedPaneTab(index: action.tabIndex ?? 0)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: action),
                    modifiers: keyBindingState.eventModifiers(for: action)
                )
                .disabled(registry.activeWorkspace == nil)
            }

            Button("Go to Last Tab") {
                selectFocusedPaneLastTab()
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .selectLastTab),
                modifiers: keyBindingState.eventModifiers(for: .selectLastTab)
            )
            .disabled(registry.activeWorkspace == nil)

            Divider()

            Button("Move Tab to New Pane") {
                breakFocusedTab()
            }
            .keyboardShortcut(
                keyBindingState.keyEquivalent(for: .breakTab),
                modifiers: keyBindingState.eventModifiers(for: .breakTab)
            )
            .disabled(!canBreakFocusedTab)
        }
    }

    // MARK: - Command Palette

    private func showNewAgentPalette() {
        if registry.activeWorkspace?.isPointGuard == true || appState.activeSidebarSelection == .nav(.dashboard) {
            appState.newPointGuardTab()
            return
        }
        let project: ProjectState?
        if let projectId = appState.activeProjectRoot {
            project = appState.projectState(forId: projectId)
        } else {
            project = appState.projects.first
        }
        guard let project else { return }

        let hub = appState.agentsHubState
        let sel = appState.activeSidebarSelection
        #if os(macOS)
        let window = NSApp.keyWindow
        CommandPalettePanel.requestOpen {
            await hub.loadAll(projectRoots: appState.libraryProjectRoots)
            let items = CommandPaletteItem.buildItems(
                builtInVariants: AgentVariant.variantsWithWorktree,
                agents: hub.agents,
                swarms: hub.swarms,
                preferredOrder: settingsState.commandPaletteOrder
            )

            CommandPalettePanel.show(
                relativeTo: window, items: items, usePointGuardNames: settingsState.usePointGuardNames
            ) { result in
                project.handlePaletteResult(result, selection: sel, hub: hub)
            }
        }
        #else
        // No key window to anchor to: the palette presenter decides where the palette appears.
        AppLifecycleHooks.paletteOpenCoordinator.request {
            await hub.loadAll(projectRoots: appState.libraryProjectRoots)
            let items = CommandPaletteItem.buildItems(
                builtInVariants: AgentVariant.variantsWithWorktree,
                agents: hub.agents,
                swarms: hub.swarms,
                preferredOrder: settingsState.commandPaletteOrder
            )

            AppLifecycleHooks.presentPalette?(
                CommandPalettePresentation(items: items, usePointGuardNames: settingsState.usePointGuardNames) { result in
                    project.handlePaletteResult(result, selection: sel, hub: hub)
                }
            )
        }
        #endif
    }

    // MARK: - Split Helpers

    /// Splitting no longer has an "enter grid mode" step — a workspace is always a grid,
    /// so the only question is whether the on-screen one has room for another pane.
    private var canSplitBelow: Bool { registry.activeWorkspace?.root.canSplit(axis: .horizontal) ?? false }

    private var canSplitRight: Bool { registry.activeWorkspace?.root.canSplit(axis: .vertical) ?? false }

    private func splitFocusedPane(axis: PaneSplitNode.Axis) {
        guard let workspace = registry.activeWorkspace else { return }
        registry.split(workspaceId: workspace.id, leafId: workspace.focusedLeafId, axis: axis)
        registry.pendingPaletteSurfaceId = registry.workspace(id: workspace.id)?.focusedSurface?.id
    }

    private func closeFocusedPane() {
        guard let workspace = registry.activeWorkspace, let pane = workspace.focusedPane else { return }
        let workspaceId = workspace.id
        let leafId = workspace.focusedLeafId
        TabCloseConfirmation.close(pane.tabs, in: workspaceId, appState: appState, registry: registry) {
            [registry] in
            registry.closePane(workspaceId: workspaceId, leafId: leafId)
        }
    }

    // MARK: - Tab Helpers

    private var focusedPaneHasTabs: Bool { (registry.activeWorkspace?.focusedPane?.tabs.count ?? 0) > 1 }

    private var canBreakFocusedTab: Bool {
        guard let workspace = registry.activeWorkspace else { return false }
        return focusedPaneHasTabs && workspace.root.canSplit(axis: .vertical)
    }

    private func newTabInFocusedPane() {
        guard let workspace = registry.activeWorkspace else { return }
        registry.pendingPaletteSurfaceId = registry.newTab(workspaceId: workspace.id, leafId: workspace.focusedLeafId)
    }

    private func cycleFocusedPaneTab(by offset: Int) {
        guard let workspace = registry.activeWorkspace else { return }
        registry.cycleTab(workspaceId: workspace.id, leafId: workspace.focusedLeafId, by: offset)
    }

    private func selectFocusedPaneTab(index: Int) {
        guard let workspace = registry.activeWorkspace else { return }
        registry.selectTab(workspaceId: workspace.id, leafId: workspace.focusedLeafId, index: index)
    }

    private func selectFocusedPaneLastTab() {
        guard let workspace = registry.activeWorkspace else { return }
        registry.selectLastTab(workspaceId: workspace.id, leafId: workspace.focusedLeafId)
    }

    private func breakFocusedTab() {
        guard let workspace = registry.activeWorkspace, let surface = workspace.focusedSurface else { return }
        registry.breakTab(workspaceId: workspace.id, surfaceId: surface.id, axis: .vertical)
    }

    private func moveFocus(_ direction: FocusDirection) {
        guard let workspace = registry.activeWorkspace else { return }
        registry.moveFocus(workspaceId: workspace.id, direction: direction)
    }

    private func postHotkeyAction(_ action: HotkeyAction) {
        NotificationCenter.default.post(
            name: .hotkeyAction,
            object: nil,
            userInfo: ["action": action]
        )
    }

    #if os(macOS)
    private func openProjectWithPicker() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a project directory"

        if panel.runModal() == .OK, let url = panel.url {
            appState.openProject(url.path)
        }
    }
    #endif
}
