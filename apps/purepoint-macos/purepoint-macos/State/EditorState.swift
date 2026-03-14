import Foundation
import Observation

@Observable
@MainActor
final class EditorState {
    var openTabs: [EditorTab] = []
    var activeTabId: String?
    var showChangesTab: Bool = true
    var externalChangeAlert: String?

    private var fileWatcher: FileTreeWatcher?
    private var watchedFilePaths: Set<String> = []

    var activeTab: EditorTab? {
        guard let id = activeTabId else { return nil }
        return openTabs.first { $0.id == id }
    }

    func openFile(path: String, name: String) {
        // If already open, just activate
        if openTabs.contains(where: { $0.id == path }) {
            activeTabId = path
            showChangesTab = false
            return
        }

        Task {
            do {
                let result = try await FileIOService.readFile(at: path)
                let modDate = FileIOService.fileModificationDate(at: path)
                let language = EditorLanguage.detect(from: name)

                let tab = EditorTab(
                    id: path,
                    name: name,
                    content: result.content,
                    isDirty: false,
                    lastModified: modDate,
                    isBinary: result.isBinary,
                    language: language
                )
                openTabs.append(tab)
                activeTabId = path
                showChangesTab = false
                watchFile(path: path)
            } catch {
                // Silently ignore — file may have been deleted
            }
        }
    }

    func closeTab(id: String) {
        openTabs.removeAll { $0.id == id }
        unwatchFile(path: id)

        if activeTabId == id {
            activeTabId = openTabs.last?.id
            if activeTabId == nil {
                showChangesTab = true
            }
        }
    }

    func setActiveTab(id: String) {
        activeTabId = id
        showChangesTab = false
    }

    func activateChangesTab() {
        activeTabId = nil
        showChangesTab = true
    }

    func markDirty(id: String) {
        guard let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].isDirty = true
    }

    func updateContent(id: String, content: String) {
        guard let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        openTabs[idx].content = content
        openTabs[idx].isDirty = true
    }

    func saveFile(id: String) async throws {
        guard let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        try await FileIOService.writeFile(content: openTabs[idx].content, to: id)
        openTabs[idx].isDirty = false
        openTabs[idx].lastModified = FileIOService.fileModificationDate(at: id)
    }

    func reloadFile(id: String) {
        guard let idx = openTabs.firstIndex(where: { $0.id == id }) else { return }
        Task {
            do {
                let result = try await FileIOService.readFile(at: id)
                openTabs[idx].content = result.content
                openTabs[idx].isDirty = false
                openTabs[idx].lastModified = FileIOService.fileModificationDate(at: id)
            } catch {}
        }
        externalChangeAlert = nil
    }

    func dismissExternalChange() {
        externalChangeAlert = nil
    }

    func stopWatching() {
        fileWatcher?.stopAll()
        fileWatcher = nil
        watchedFilePaths.removeAll()
    }

    // MARK: - File Watching

    private func ensureWatcher() {
        guard fileWatcher == nil else { return }
        fileWatcher = FileTreeWatcher { [weak self] in
            let captured = self
            Task { @MainActor in
                captured?.checkForExternalChanges()
            }
        }
    }

    private func watchFile(path: String) {
        ensureWatcher()
        let dir = (path as NSString).deletingLastPathComponent
        if !watchedFilePaths.contains(dir) {
            watchedFilePaths.insert(dir)
            fileWatcher?.watchDirectory(path: dir)
        }
    }

    private func unwatchFile(path: String) {
        let dir = (path as NSString).deletingLastPathComponent
        let hasOtherFilesInDir = openTabs.contains { tab in
            tab.id != path && (tab.id as NSString).deletingLastPathComponent == dir
        }
        if !hasOtherFilesInDir {
            watchedFilePaths.remove(dir)
            fileWatcher?.unwatchDirectory(path: dir)
        }
    }

    private func checkForExternalChanges() {
        for (idx, tab) in openTabs.enumerated() {
            guard let lastMod = tab.lastModified,
                let currentMod = FileIOService.fileModificationDate(at: tab.id),
                currentMod > lastMod
            else { continue }

            if tab.isDirty {
                externalChangeAlert = tab.id
            } else {
                // Auto-reload clean tabs
                Task {
                    do {
                        let result = try await FileIOService.readFile(at: tab.id)
                        openTabs[idx].content = result.content
                        openTabs[idx].lastModified = currentMod
                    } catch {}
                }
            }
        }
    }
}
