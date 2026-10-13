import Foundation
import Observation

@Observable
@MainActor
final class EditorState {
    var currentFile: EditorTab?
    var showChanges: Bool = true
    var externalChangeAlert: String?

    private var fileWatcher: (any HostDirectoryWatcher)?
    private var watchedPath: String?
    /// Where opened files live. Set before opening; a remote host reads and saves through
    /// the machine's daemon and rejects saves that would overwrite a newer remote file.
    var host: any ProjectHost = LocalProjectHost.shared
    /// Modification stamp of the open file as last read or written, for conflict checks.
    private var loadedModifiedMs: Int64?

    func openFile(path: String, name: String) {
        if currentFile?.id == path {
            showChanges = false
            return
        }

        let host = self.host
        Task {
            do {
                let result = try await FileIOService.readTextFile(at: path, host: host)
                let modDate = await FileIOService.fileModificationDate(at: path, host: host)
                let language = EditorLanguage.detect(from: name)
                loadedModifiedMs = result.modifiedMs

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
        let modified = try await FileIOService.writeFile(
            content: file.content, to: file.id, host: host, expectedModifiedMs: host.isLocal ? nil : loadedModifiedMs)
        loadedModifiedMs = modified
        currentFile?.isDirty = false
        currentFile?.lastModified = await FileIOService.fileModificationDate(at: file.id, host: host)
    }

    func reloadFile() {
        guard let file = currentFile else { return }
        let path = file.id
        let host = self.host
        Task {
            do {
                let result = try await FileIOService.readTextFile(at: path, host: host)
                loadedModifiedMs = result.modifiedMs
                currentFile?.content = result.content
                currentFile?.isDirty = false
                currentFile?.lastModified = await FileIOService.fileModificationDate(at: path, host: host)
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
        fileWatcher = host.makeWatcher { [weak self] in
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
        guard host.isLocal else {
            checkForRemoteChanges()
            return
        }
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

    private func checkForRemoteChanges() {
        guard let file = currentFile, let lastMod = file.lastModified else { return }
        let filePath = file.id
        let host = self.host
        Task {
            guard let currentMod = await FileIOService.fileModificationDate(at: filePath, host: host),
                currentMod > lastMod, currentFile?.id == filePath
            else { return }
            if currentFile?.isDirty == true {
                externalChangeAlert = filePath
                return
            }
            do {
                let result = try await FileIOService.readTextFile(at: filePath, host: host)
                guard currentFile?.id == filePath else { return }
                loadedModifiedMs = result.modifiedMs
                currentFile?.content = result.content
                currentFile?.lastModified = currentMod
            } catch {}
        }
    }
}
