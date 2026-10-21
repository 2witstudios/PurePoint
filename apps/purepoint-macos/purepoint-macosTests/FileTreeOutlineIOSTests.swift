#if os(iOS)
import Foundation
import SwiftUI
import Testing
import UIKit

@testable import PurePoint

// MARK: - Keyboard

struct FileTreeKeyNavigationTests {
    typealias Nav = FileTreeKeyNavigation

    // src/ (open) > main.swift, README.md
    private let rows: [Nav.Row] = [
        Nav.Row(parent: nil, isExpandable: true, isExpanded: true),
        Nav.Row(parent: 0),
        Nav.Row(parent: nil),
    ]

    @Test func downAndUpStepOneVisibleRow() {
        #expect(Nav.command(for: .down, rows: rows, selected: nil) == .select(0))
        #expect(Nav.command(for: .down, rows: rows, selected: 0) == .select(1))
        #expect(Nav.command(for: .down, rows: rows, selected: 2) == Nav.Command.none)
        #expect(Nav.command(for: .up, rows: rows, selected: nil) == .select(2))
        #expect(Nav.command(for: .up, rows: rows, selected: 1) == .select(0))
        #expect(Nav.command(for: .up, rows: rows, selected: 0) == Nav.Command.none)
    }

    @Test func leftClosesAnOpenFolderThenMovesToTheParent() {
        #expect(Nav.command(for: .left, rows: rows, selected: 0) == .collapse(0))
        #expect(Nav.command(for: .left, rows: rows, selected: 1) == .select(0))
        #expect(Nav.command(for: .left, rows: rows, selected: 2) == Nav.Command.none)
    }

    @Test func rightOpensOnlyAClosedFolder() {
        let closed = [Nav.Row(parent: nil, isExpandable: true, isExpanded: false)]
        #expect(Nav.command(for: .right, rows: closed, selected: 0) == .expand(0))
        #expect(Nav.command(for: .right, rows: rows, selected: 0) == Nav.Command.none)
        #expect(Nav.command(for: .right, rows: rows, selected: 2) == Nav.Command.none)
        #expect(Nav.command(for: .right, rows: rows, selected: nil) == Nav.Command.none)
    }
}

struct FileTreeTypeSelectTests {
    private let names = ["docs", "Package.swift", "README.md", "readme-old.txt", "src"]

    @Test func typingSelectsTheFirstRowWithThePrefixIgnoringCase() {
        var select = FileTreeTypeSelect()
        let now = Date()
        #expect(select.row(typing: "p", names: names, selected: nil, now: now) == 1)
        var other = FileTreeTypeSelect()
        #expect(other.row(typing: "R", names: names, selected: nil, now: now) == 2)
    }

    @Test func aQuickSecondKeyExtendsThePrefixFromTheSelectedRow() {
        var select = FileTreeTypeSelect()
        let now = Date()
        #expect(select.row(typing: "r", names: names, selected: nil, now: now) == 2)
        #expect(select.row(typing: "e", names: names, selected: 2, now: now.addingTimeInterval(0.2)) == 2)
        #expect(select.row(typing: "adme-", names: names, selected: 2, now: now.addingTimeInterval(0.4)) == 3)
        #expect(select.prefix == "readme-")
    }

    @Test func aPauseStartsANewSearchAfterTheSelectedRowAndWraps() {
        var select = FileTreeTypeSelect()
        let now = Date()
        #expect(select.row(typing: "r", names: names, selected: nil, now: now) == 2)
        #expect(select.row(typing: "r", names: names, selected: 2, now: now.addingTimeInterval(2)) == 3)
        #expect(select.row(typing: "r", names: names, selected: 3, now: now.addingTimeInterval(4)) == 2)
        #expect(select.row(typing: "z", names: names, selected: 3, now: now.addingTimeInterval(6)) == nil)
    }

    @Test func onlyPrintableTextTypeSelects() {
        #expect(FileTreeKeyOutlineView.isTypeSelectInput("a"))
        #expect(FileTreeKeyOutlineView.isTypeSelectInput("."))
        #expect(!FileTreeKeyOutlineView.isTypeSelectInput(""))
        #expect(!FileTreeKeyOutlineView.isTypeSelectInput(" "))
        #expect(!FileTreeKeyOutlineView.isTypeSelectInput("\t"))
        #expect(!FileTreeKeyOutlineView.isTypeSelectInput("\r"))
        #expect(!FileTreeKeyOutlineView.isTypeSelectInput("\u{F700}"))
    }
}

// MARK: - Controller

@MainActor
struct FileTreeOutlineViewControllerIOSTests {
    private func node(_ relative: String, directory: Bool = false, children: [FileTreeNode] = []) -> FileTreeNode {
        FileTreeNode(
            name: (relative as NSString).lastPathComponent, absolutePath: "/srv/app/" + relative,
            relativePath: relative, isDirectory: directory, children: children)
    }

    private func makeController(_ nodes: [FileTreeNode]) -> FileTreeOutlineViewController {
        let vc = FileTreeOutlineViewController()
        vc.loadViewIfNeeded()
        vc.view.frame = CGRect(x: 0, y: 0, width: 280, height: 600)
        vc.isLocal = false
        vc.updateNodes(nodes)
        vc.view.layoutIfNeeded()
        return vc
    }

    @Test func rendersRootRowsInTheStatesOrderWith24ptRows() throws {
        let vc = makeController([node("src", directory: true), node("README.md")])
        #expect(vc.visibleNodes.map(\.name) == ["src", "README.md"])
        let attributes = try #require(vc.outlineView.layoutAttributesForItem(at: IndexPath(item: 1, section: 0)))
        #expect(attributes.frame.height == 24)
        #expect(attributes.frame.minY == 24)
    }

    @Test func expandingAnUnloadedFolderAsksTheStateThenShowsItsChildrenIndented() throws {
        let src = node("src", directory: true)
        let vc = makeController([src, node("README.md")])
        var expanded: [String] = []
        vc.onNodeExpanded = { expanded.append($0.relativePath) }

        vc.setExpanded(src, true)
        #expect(expanded == ["src"])
        #expect(vc.isExpanded(src))

        // The state answers with a new tree holding the folder's children.
        let loaded = node("src", directory: true, children: [node("src/main.swift")])
        vc.updateNodes([loaded, node("README.md")])
        vc.view.layoutIfNeeded()
        #expect(vc.visibleNodes.map(\.relativePath) == ["src", "src/main.swift", "README.md"])
        #expect(vc.isExpanded(loaded))
        let cell = try #require(
            vc.outlineView.cellForItem(at: IndexPath(item: 1, section: 0)) as? FileTreeRowCell)
        #expect(cell.nameLabel.text == "main.swift")
        #expect(cell.disclosureButton.isHidden)
    }

    @Test func expandingALoadedFolderDoesNotReloadIt() {
        let src = node("src", directory: true, children: [node("src/main.swift")])
        let vc = makeController([src])
        var expanded = 0
        vc.onNodeExpanded = { _ in expanded += 1 }
        vc.setExpanded(src, true)
        #expect(expanded == 0)
        #expect(vc.visibleNodes.map(\.relativePath) == ["src", "src/main.swift"])
    }

    @Test func collapsingTellsTheStateAndHidesTheChildren() {
        let src = node("src", directory: true, children: [node("src/main.swift")])
        let vc = makeController([src])
        vc.setExpanded(src, true)
        var collapsed: [String] = []
        vc.onNodeCollapsed = { collapsed.append($0.relativePath) }
        vc.setExpanded(src, false)
        #expect(collapsed == ["src"])
        #expect(vc.visibleNodes.map(\.relativePath) == ["src"])
    }

    @Test func expansionSurvivesAReloadWithNewNodes() {
        let vc = makeController([node("src", directory: true, children: [node("src/a.swift")])])
        vc.setExpanded(vc.rootNodes[0], true)
        vc.updateNodes([node("src", directory: true, children: [node("src/a.swift"), node("src/b.swift")])])
        #expect(vc.visibleNodes.map(\.relativePath) == ["src", "src/a.swift", "src/b.swift"])
    }

    @Test func selectingAFileOpensItAndAFolderOnlySelects() {
        let vc = makeController([node("src", directory: true), node("README.md")])
        var opened: [String] = []
        vc.onFileSelected = { path, name in opened.append("\(path)|\(name)") }

        vc.collectionView(vc.outlineView, didSelectItemAt: IndexPath(item: 0, section: 0))
        #expect(opened.isEmpty)
        vc.collectionView(vc.outlineView, didSelectItemAt: IndexPath(item: 1, section: 0))
        #expect(opened == ["/srv/app/README.md|README.md"])
        // Choosing the selected row again changes nothing.
        vc.collectionView(vc.outlineView, didSelectItemAt: IndexPath(item: 1, section: 0))
        #expect(opened.count == 1)
    }

    @Test func arrowKeysMoveTheSelectionAndOpenFiles() {
        let vc = makeController([node("src", directory: true), node("README.md")])
        var opened: [String] = []
        vc.onFileSelected = { _, name in opened.append(name) }
        #expect(vc.handleArrowKey(.down))
        #expect(vc.handleArrowKey(.down))
        #expect(opened == ["README.md"])
        #expect(vc.outlineView.indexPathsForSelectedItems == [IndexPath(item: 1, section: 0)])
        #expect(!vc.handleArrowKey(.down))
    }

    @Test func typeSelectMovesToTheMatchingRow() {
        let vc = makeController([node("docs", directory: true), node("README.md")])
        var opened: [String] = []
        vc.onFileSelected = { _, name in opened.append(name) }
        #expect(vc.handleTypeSelect("r"))
        #expect(opened == ["README.md"])
    }

    @Test func contextMenuIsTheMacSetWithoutRevealInFinder() {
        typealias VC = FileTreeOutlineViewController
        let file = node("README.md")
        let folder = node("src", directory: true)
        #expect(VC.menuItems(for: file, isLocal: false) == [.openLocalCopy, .copyPath, .copyRelativePath])
        #expect(VC.menuItems(for: folder, isLocal: false) == [.copyPath, .copyRelativePath])
        #expect(VC.menuItems(for: file, isLocal: true) == [.copyPath, .copyRelativePath])
        #expect(VC.menuItems(for: file, isLocal: false).map(\.title) == ["Open Local Copy", "Copy Path", "Copy Relative Path"])
    }

    @Test func contextActionsCopyPathsAndAskForALocalCopy() {
        let vc = makeController([node("src/main.swift")])
        var localCopies: [String] = []
        vc.onOpenLocalCopy = { localCopies.append($0) }
        let file = vc.rootNodes[0]
        vc.perform(.copyPath, on: file)
        #expect(UIPasteboard.general.string == "/srv/app/src/main.swift")
        vc.perform(.copyRelativePath, on: file)
        #expect(UIPasteboard.general.string == "src/main.swift")
        vc.perform(.openLocalCopy, on: file)
        #expect(localCopies == ["/srv/app/src/main.swift"])
    }

    @Test func rowsUseTheMacIcons() {
        #expect(FileTreeRowCell.iconName(for: node("src", directory: true)) == "folder.fill")
        #expect(FileTreeRowCell.iconName(for: node("main.swift")) == EditorLanguage.detect(from: "main.swift").icon)
    }
}

// MARK: - Ignore resolution

@MainActor
struct FileTreeStateIOSIgnoreTests {
    @Test func remoteTreeDropsWhatTheDaemonsGitIgnoresButKeepsDotFiles() async throws {
        let host = ListingHost(entries: [
            "/srv/app": [
                .init(name: "build", path: "/srv/app/build", isDirectory: true, ignored: true),
                .init(name: ".env", path: "/srv/app/.env", isDirectory: false, ignored: true),
                .init(name: ".git", path: "/srv/app/.git", isDirectory: true, ignored: false),
                .init(name: "Sources", path: "/srv/app/Sources", isDirectory: true, ignored: false),
                .init(name: "README.md", path: "/srv/app/README.md", isDirectory: false, ignored: false),
            ]
        ])
        let state = FileTreeState()
        state.load(worktreePath: "/srv/app", host: host)
        for _ in 0..<200 where state.rootNodes.isEmpty {
            try await Task.sleep(for: .milliseconds(10))
        }
        state.stopWatching()
        #expect(state.rootNodes.map(\.name) == ["Sources", ".env", "README.md"])
        #expect(state.rootNodes.map(\.relativePath) == ["Sources", ".env", "README.md"])
        // Ignore verdicts arrive with the listing: no local git and no extra command runs.
        #expect(host.commandCount == 0)
    }
}

private final class ListingHost: ProjectHost, @unchecked Sendable {
    struct Entry {
        let name: String
        let path: String
        let isDirectory: Bool
        let ignored: Bool
    }

    private let lock = NSLock()
    private let entries: [String: [Entry]]
    private var commands = 0

    init(entries: [String: [Entry]]) { self.entries = entries }

    var commandCount: Int { lock.withLock { commands } }
    var isLocal: Bool { false }
    var identity: String { "machine:test" }

    func list(directory: String) throws -> [HostFileEntry] {
        (entries[directory] ?? []).map {
            HostFileEntry(
                name: $0.name, path: $0.path, isDirectory: $0.isDirectory, isSymlink: false, size: 0, modifiedMs: 0,
                ignored: $0.ignored)
        }
    }
    func listRecursive(root: String, limit: Int) throws -> (entries: [HostFileEntry], truncated: Bool) { ([], false) }
    func stat(path: String) throws -> HostFileStat { .missing }
    func read(path: String, maxBytes: Int) throws -> HostFileData { throw HostError.notFound(path) }
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        throw HostError.notFound(path)
    }
    func delete(path: String) throws {}
    func run(_ command: HostCommand) -> CommandResult {
        lock.withLock { commands += 1 }
        return CommandResult(stdout: "", stderr: "", exitCode: 1)
    }
    func makeWatcher(onChange: @escaping @Sendable () -> Void) -> any HostDirectoryWatcher { InertDirectoryWatcher() }
}

// MARK: - The sidebar split

struct WorkspaceFilesSplitTests {
    typealias Split = WorkspaceFilesSplit<EmptyView, EmptyView>

    @Test func sidebarWidthStaysInsideTheMacFrameLimits() {
        #expect(Split.clampedSidebarWidth(360, total: 1200) == 360)
        #expect(Split.clampedSidebarWidth(100, total: 1200) == 240)
        #expect(Split.clampedSidebarWidth(900, total: 1200) == 640)
    }

    @Test func contentKeepsItsMinimumInANarrowWindow() {
        // 160pt content plus the 1pt divider.
        #expect(Split.clampedSidebarWidth(360, total: 500) == 339)
        #expect(Split.clampedSidebarWidth(360, total: 100) == 0)
    }
}
#endif
