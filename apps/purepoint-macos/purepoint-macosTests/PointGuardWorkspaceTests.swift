import Foundation
import Testing

@testable import PurePoint

@MainActor @Suite struct PointGuardWorkspaceTests {
    private func withRegistry(_ body: (WorkspaceRegistry, String) throws -> Void) rethrows {
        let root = NSTemporaryDirectory() + "pp-point-guard-workspace-" + UUID().uuidString
        let registry = WorkspaceRegistry()
        defer {
            registry.forgetProject(root)
            try? FileManager.default.removeItem(atPath: root)
        }
        try body(registry, root)
    }

    @Test func chatCanSplitAndShareTabsWithRootAgents() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.split(workspaceId: id, leafId: 0, axis: .vertical)
            let target = try #require(registry.workspace(id: id)?.focusedSurface?.id)
            registry.reserveSurface(projectRoot: root, workspaceId: id, surfaceId: target)
            registry.fulfillReservation(projectRoot: root, workspaceId: id, surfaceId: target, agentId: "ag-terminal")
            registry.reconcile(
                projectRoot: root,
                rootAgents: [
                    AgentModel(
                        id: "ag-terminal", name: "Terminal", agentType: "terminal", status: .running, prompt: "",
                        startedAt: "")
                ], worktrees: [])
            let workspace = try #require(registry.workspace(id: id))
            #expect(workspace.container == .projectRoot)
            #expect(workspace.paneCount == 2)
            #expect(workspace.surfaces.map(\.surface.content) == [.pointGuard, .agent("ag-terminal")])
            #expect(registry.workspaceId(forAgent: "ag-terminal") == id)
            #expect(registry.workspacesByProject[root]?.count == 1)
        }
    }

    @Test func visitingPointGuardKeepsTheFocusedChatAndExistingLayout() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            let terminal = try #require(registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-terminal")))
            registry.selectTab(workspaceId: id, surfaceId: 0)
            registry.activate(workspaceId: id)
            #expect(registry.activeWorkspace?.focusedSurface?.content == .pointGuard)
            #expect(try registry.ensurePointGuardWorkspace(projectRoot: root) == id)
            #expect(registry.workspace(id: id)?.tabCount == 2)
            registry.selectTab(workspaceId: id, surfaceId: terminal)
            #expect(registry.activeWorkspace?.focusedSurface?.content == .agent("ag-terminal"))
        }
    }

    @Test func closingTheLastTabReturnsToChatWithoutReusingSurfaceIds() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.activeWorkspaceId = id
            registry.closeTab(workspaceId: id, surfaceId: 0)
            let workspace = try #require(registry.workspace(id: id))
            #expect(workspace.focusedSurface?.content == .pointGuard)
            #expect(workspace.focusedSurface?.id != 0)
            #expect(registry.activeWorkspaceId == id)
        }
    }

    @Test func deadTerminalDoesNotRemoveChatOrAdoptItAsAnAgent() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-dead"))
            registry.reconcile(projectRoot: root, rootAgents: [], worktrees: [])
            #expect(registry.workspace(id: id)?.surfaces.map(\.surface.content) == [.pointGuard])
        }
    }

    @Test func closingChatDoesNotKillItsSiblingTerminalAndReopeningRestoresChat() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-terminal"))
            var killed: [String] = []
            registry.onCloseAgent = { _, agentId in killed.append(agentId) }
            registry.closeTab(workspaceId: id, surfaceId: 0)
            #expect(killed.isEmpty)
            #expect(registry.workspace(id: id)?.agentIds == ["ag-terminal"])
            try registry.ensurePointGuardWorkspace(projectRoot: root)
            #expect(registry.workspace(id: id)?.tabCount == 2)
            #expect(registry.workspace(id: id)?.focusedSurface?.content == .pointGuard)
        }
    }

    @Test func chatAndFileTabsPersistAcrossRestart() throws {
        try withRegistry { registry, root in
            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.split(workspaceId: id, leafId: 0, axis: .horizontal)
            let file = try #require(registry.workspace(id: id)?.focusedSurface?.id)
            registry.openFile(workspaceId: id, surfaceId: file, path: root + "/notes.md")
            registry.activeWorkspaceId = id
            registry.saveAll()
            let data = try Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root)))
            let document = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
            // Version 3 builds must recognize this as newer and leave it untouched.
            #expect(document.version == 4)
            let restored = WorkspacePersistence.load(projectRoot: root)
            #expect(restored == [registry.workspace(id: id)!])
            #expect(restored.first?.isPointGuard == true)
            #expect(WorkspaceReconciler.reconcile(stored: restored, live: []) == restored)
        }
    }

    @Test func versionThreeTabLayoutsStillLoadAndUpgradeWithoutLosingIds() throws {
        try withRegistry { _, root in
            var original = Workspace.adopting(agentId: "ag-existing", container: .projectRoot)
            original.newTab(leafId: 0, content: .file(path: root + "/notes.md"))
            original.split(leafId: 0, axis: .vertical, content: .agent("ag-sibling"))
            let url = URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root))
            WorkspacePersistence.save([original], projectRoot: root, activeWorkspaceId: original.id)
            var document = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
            document["version"] = 3
            try JSONSerialization.data(withJSONObject: document).write(to: url)

            let restored = WorkspacePersistence.load(projectRoot: root)
            #expect(restored == [original])
            WorkspacePersistence.save(restored, projectRoot: root, activeWorkspaceId: original.id)
            let upgraded = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: Data(contentsOf: url))
            #expect(upgraded.version == 4)
            #expect(upgraded.activeWorkspaceId == original.id)
            #expect(WorkspacePersistence.load(projectRoot: root) == [original])
        }
    }

    @Test func projectRootsHaveDistinctPointGuardLayouts() throws {
        try withRegistry { registry, root in
            let first = try registry.ensurePointGuardWorkspace(projectRoot: root)
            let secondRoot = root + "-second"
            defer { registry.forgetProject(secondRoot); try? FileManager.default.removeItem(atPath: secondRoot) }
            let second = try registry.ensurePointGuardWorkspace(projectRoot: secondRoot)
            #expect(first != second)
            #expect(registry.projectRoot(forWorkspace: first) == root)
            #expect(registry.projectRoot(forWorkspace: second) == secondRoot)
        }
    }

    @Test func copiedProjectRekeysPointGuardBeforeGlobalLookupsAndMutations() throws {
        try withRegistry { registry, root in
            let copiedRoot = root + "-copy"
            defer { registry.forgetProject(copiedRoot); try? FileManager.default.removeItem(atPath: copiedRoot) }
            var original = Workspace.pointGuard(projectRoot: root)
            original.split(leafId: 0, axis: .vertical, content: .file(path: nil))
            original.newTab(leafId: original.focusedLeafId, content: .file(path: root + "/notes.md"))
            WorkspacePersistence.save([original], projectRoot: root, activeWorkspaceId: original.id)
            let copiedURL = URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: copiedRoot))
            try FileManager.default.createDirectory(
                at: copiedURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try FileManager.default.copyItem(
                atPath: WorkspacePersistence.filePath(projectRoot: root), toPath: copiedURL.path)

            let originalId = try registry.ensurePointGuardWorkspace(projectRoot: root)
            // Reconcile is also a first-load path, before the chat view has appeared.
            registry.reconcile(projectRoot: copiedRoot, rootAgents: [], worktrees: [])
            let copiedId = try registry.ensurePointGuardWorkspace(projectRoot: copiedRoot)
            let migrated = try JSONDecoder().decode(
                PersistedWorkspaceDocument.self, from: Data(contentsOf: copiedURL))
            #expect(migrated.activeWorkspaceId == copiedId)
            #expect(migrated.workspaces.map(\.id) == [copiedId])
            #expect(copiedId == Workspace.pointGuard(projectRoot: copiedRoot).id)
            #expect(copiedId != originalId)
            #expect(registry.projectRoot(forWorkspace: originalId) == root)
            #expect(registry.projectRoot(forWorkspace: copiedId) == copiedRoot)
            let restored = try #require(registry.workspace(id: copiedId))
            #expect(restored.root == original.root)
            #expect(restored.panes == original.panes)
            #expect(restored.focusedLeafId == original.focusedLeafId)
            #expect(restored.nextLeafId == original.nextLeafId)
            #expect(restored.nextSurfaceId == original.nextSurfaceId)

            registry.newTab(
                workspaceId: copiedId, leafId: restored.focusedLeafId, content: .file(path: copiedRoot + "/copy.md"))
            #expect(registry.workspace(id: originalId) == original)
            #expect(registry.workspace(id: copiedId)?.tabCount == original.tabCount + 1)
            registry.saveAll()
            #expect(WorkspacePersistence.load(projectRoot: copiedRoot) == [registry.workspace(id: copiedId)!])
        }
    }

    @Test func renamedProjectLoadsExistingLayoutWithCurrentProjectId() throws {
        try withRegistry { registry, root in
            let renamedRoot = root + "-renamed"
            defer { registry.forgetProject(renamedRoot); try? FileManager.default.removeItem(atPath: renamedRoot) }
            var original = Workspace.pointGuard(projectRoot: root)
            original.newTab(leafId: 0, content: .agent("ag-terminal"))
            WorkspacePersistence.save([original], projectRoot: root)
            try FileManager.default.moveItem(atPath: root, toPath: renamedRoot)

            let renamedId = try registry.ensurePointGuardWorkspace(projectRoot: renamedRoot)
            let restored = try #require(registry.workspace(id: renamedId))
            #expect(renamedId == Workspace.pointGuard(projectRoot: renamedRoot).id)
            #expect(registry.workspace(id: original.id) == nil)
            #expect(restored.panes == original.panes)
            #expect(restored.root == original.root)
        }
    }

    @Test func newerCopiedDocumentRekeysIdsWithoutDowngradingOrDroppingUnknownData() throws {
        try withRegistry { registry, root in
            let original = Workspace.pointGuard(projectRoot: root + "-original")
            WorkspacePersistence.save([original], projectRoot: root, activeWorkspaceId: original.id)
            let url = URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root))
            var future = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
            future["version"] = PersistedWorkspaceDocument.currentVersion + 1
            future["futureDocument"] = ["value": "preserve", "number": 42]
            var workspaces = try #require(future["workspaces"] as? [[String: Any]])
            workspaces[0]["futureWorkspace"] = ["enabled": true]
            var tree = try #require(workspaces[0]["tree"] as? [String: Any])
            tree["futureTree"] = ["layout": "new"]
            var tabs = try #require(tree["tabs"] as? [[String: Any]])
            tabs[0]["futureSurface"] = ["content": "opaque"]
            tabs.append(["id": 10, "kind": "future_browser", "url": "https://example.com"])
            tree["tabs"] = tabs
            workspaces[0]["tree"] = tree
            future["workspaces"] = workspaces
            try JSONSerialization.data(withJSONObject: future).write(to: url)

            let id = try registry.ensurePointGuardWorkspace(projectRoot: root)
            #expect(id != original.id)
            workspaces[0]["id"] = id
            future["workspaces"] = workspaces
            future["activeWorkspaceId"] = id
            let migratedData = try Data(contentsOf: url)
            let migrated = try #require(JSONSerialization.jsonObject(with: migratedData) as? NSDictionary)
            #expect(migrated == future as NSDictionary)
            #expect(WorkspacePersistence.isNewerThanSupported(projectRoot: root))

            registry.newTab(workspaceId: id, leafId: 0)
            registry.saveAll()
            // Only identity migration is allowed; regular saves must retain the future schema.
            #expect(try Data(contentsOf: url) == migratedData)
        }
    }

    @Test func failedIdentityWriteBlocksPublicationAndCanRetryWithoutTouchingOtherProject() throws {
        for code in [CocoaError.Code.fileWriteNoPermission, .fileWriteOutOfSpace] {
            try withRegistry { _, root in
                let copiedRoot = root + "-copy"
                var canWrite = false
                let registry = WorkspaceRegistry { projectRoot in
                    try WorkspacePersistence.loadForRegistry(projectRoot: projectRoot) { data, url in
                        if !canWrite { throw CocoaError(code) }
                        try data.write(to: url, options: .atomic)
                    }
                }
                defer {
                    registry.forgetProject(root)
                    registry.forgetProject(copiedRoot)
                    try? FileManager.default.removeItem(atPath: copiedRoot)
                }
                let original = Workspace.pointGuard(projectRoot: root)
                WorkspacePersistence.save([original], projectRoot: root)
                WorkspacePersistence.save([original], projectRoot: copiedRoot, activeWorkspaceId: original.id)
                let url = URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: copiedRoot))
                let before = try Data(contentsOf: url)
                let originalId = try registry.ensurePointGuardWorkspace(projectRoot: root)
                registry.activate(workspaceId: originalId)

                registry.reconcile(projectRoot: copiedRoot, rootAgents: [], worktrees: [])
                #expect(throws: CocoaError.self) { try registry.ensurePointGuardWorkspace(projectRoot: copiedRoot) }
                #expect(registry.workspacesByProject[copiedRoot] == nil)
                #expect(registry.workspace(id: Workspace.pointGuardId(projectRoot: copiedRoot)) == nil)
                #expect(registry.layoutErrorsByProject[copiedRoot] != nil)
                #expect(registry.workspace(id: originalId) == original)
                registry.saveAll()
                #expect(try Data(contentsOf: url) == before)

                canWrite = true
                let copiedId = try registry.ensurePointGuardWorkspace(projectRoot: copiedRoot)
                #expect(copiedId != originalId)
                #expect(registry.projectRoot(forWorkspace: copiedId) == copiedRoot)
                #expect(registry.layoutErrorsByProject[copiedRoot] == nil)
                #expect(registry.workspace(id: originalId) == original)
                let document = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: Data(contentsOf: url))
                #expect(document.activeWorkspaceId == copiedId)
            }
        }
    }

    @Test func worktreeAgentsCannotBeClaimedByPointGuard() throws {
        var workspace = Workspace.pointGuard(projectRoot: "/tmp/project")
        workspace.newTab(leafId: 0, content: .agent("ag-worktree"))
        let restored = WorkspaceReconciler.reconcile(
            stored: [workspace], live: [LiveAgent(id: "ag-worktree", container: .worktree("wt-one"))])
        #expect(restored.first?.container == .projectRoot)
        #expect(restored.first?.surfaces.map(\.surface.content) == [.pointGuard])
        #expect(restored.last?.container == .worktree("wt-one"))
    }
}
