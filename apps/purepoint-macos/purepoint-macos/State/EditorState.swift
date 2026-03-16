import Foundation
import Observation

@Observable
@MainActor
final class EditorState {
    var currentFile: EditorTab?
    var showChanges: Bool = true
    var externalChangeAlert: String?

    private var fileWatcher: FileTreeWatcher?
    private var watchedPath: String?

    func openFile(path: String, name: String) {
        if currentFile?.id == path {
            showChanges = false
            return
        }

        Task {
            do {
                let result = try await FileIOService.readFile(at: path)
                let modDate = FileIOService.fileModificationDate(at: path)
                let language = EditorLanguage.detect(from: name)

                currentFile = EditorTab(
                    id: path,
                    name: name,
                    content: result.content,
                    isDirty: false,
                    lastModified: modDate,
                    isBinary: result.isBinary,
                    language: language
                )
                showChanges = false
                watchFile(path: path)
            } catch {
                // Silently ignore — file may have been deleted
            }
        }
    }

    func updateContent(content: String) {
        currentFile?.content = content
        currentFile?.isDirty = true
    }

    func saveFile() async throws {
        guard let file = currentFile else { return }
        try await FileIOService.writeFile(content: file.content, to: file.id)
        currentFile?.isDirty = false
        currentFile?.lastModified = FileIOService.fileModificationDate(at: file.id)
    }

    func reloadFile() {
        guard let file = currentFile else { return }
        let path = file.id
        Task {
            do {
                let result = try await FileIOService.readFile(at: path)
                currentFile?.content = result.content
                currentFile?.isDirty = false
                currentFile?.lastModified = FileIOService.fileModificationDate(at: path)
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
        watchedPath = nil
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
        let dir = (path as NSString).deletingLastPathComponent
        if watchedPath != dir {
            if let old = watchedPath {
                fileWatcher?.unwatchDirectory(path: old)
            }
            ensureWatcher()
            watchedPath = dir
            fileWatcher?.watchDirectory(path: dir)
        }
    }

    private func checkForExternalChanges() {
        guard let file = currentFile,
            let lastMod = file.lastModified,
            let currentMod = FileIOService.fileModificationDate(at: file.id),
            currentMod > lastMod
        else { return }

        if file.isDirty {
            externalChangeAlert = file.id
        } else {
            let filePath = file.id
            Task {
                do {
                    let result = try await FileIOService.readFile(at: filePath)
                    currentFile?.content = result.content
                    currentFile?.lastModified = currentMod
                } catch {}
            }
        }
    }
}
