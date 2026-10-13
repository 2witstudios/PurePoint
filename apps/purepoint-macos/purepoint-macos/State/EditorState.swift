import Foundation
import Observation

@Observable
@MainActor
final class EditorState {
    var currentFile: EditorTab?
    var showChanges: Bool = true
    var externalChangeAlert: String?
    /// Why the last file asked for could not be opened (too large, unreadable, …).
    var openError: String?
    /// A remote save was refused because the file changed on the machine; the user can
    /// reload or overwrite.
    var saveConflict = false

    private var fileWatcher: (any HostDirectoryWatcher)?
    private var watchedPath: String?
    /// Where opened files live. Set before opening; a remote host reads and saves through
    /// the machine's daemon and rejects saves that would overwrite a newer remote file.
    var host: any ProjectHost = LocalProjectHost.shared
    /// Modification stamp of the open file as last read or written, for conflict checks.
    private var loadedModifiedMs: Int64?
    /// The file most recently asked for: a slower earlier read must not replace it.
    private var requestedPath: String?

    init() {
        OpenEditors.register(self)
    }

    /// Whether the open file holds edits not yet saved.
    var hasUnsavedChanges: Bool { currentFile?.isDirty == true }

    /// Why the open file cannot be edited, if it cannot.
    var readOnlyReason: String? {
        currentFile?.isReadOnly == true
            ? "Read-only: files over 750 KB cannot be saved on a remote machine." : nil
    }

    func openFile(path: String, name: String) {
        // Set first: re-selecting the open file must also win over a slower read
        // of another file still in flight.
        requestedPath = path
        if currentFile?.id == path {
            showChanges = false
            return
        }

        let host = self.host
        Task {
            do {
                let result = try await FileIOService.readTextFile(at: path, host: host)
                let modDate =
                    host.isLocal
                    ? FileIOService.fileModificationDate(at: path)
                    : Date(timeIntervalSince1970: Double(result.modifiedMs) / 1000)
                guard requestedPath == path else { return }
                // The stamp and the content come from the same read, so a save is checked
                // against exactly the version shown.
                loadedModifiedMs = result.modifiedMs
                openError = nil
                saveConflict = false
                currentFile = EditorTab(
                    id: path,
                    name: name,
                    content: result.content,
                    isDirty: false,
                    lastModified: modDate,
                    isBinary: result.isBinary,
                    language: EditorLanguage.detect(from: name),
                    isReadOnly: Self.isTooLargeToSave(result.content, host: host)
                )
                showChanges = false
                watchFile(path: path)
            } catch {
                guard requestedPath == path else { return }
                openError = "Could not open \(name): \(error.localizedDescription)"
            }
        }
    }

    /// A remote save travels in one request line, so larger files open read-only rather than
    /// accepting edits that could never be saved.
    nonisolated static func isTooLargeToSave(_ content: String, host: any ProjectHost) -> Bool {
        !host.isLocal && content.utf8.count > HostError.maxRemoteSaveBytes
    }

    func updateContent(content: String) {
        guard currentFile?.isReadOnly != true else { return }
        currentFile?.content = content
        currentFile?.isDirty = true
    }

    func saveFile() async throws {
        try await save(expectedModifiedMs: host.isLocal ? nil : loadedModifiedMs)
    }

    /// Save over a version changed on the machine since it was opened (the user chose to).
    func overwriteRemoteChanges() async throws {
        guard let file = currentFile, !host.isLocal else { return }
        let host = self.host
        let current = await HostIO.value { (try? host.stat(path: file.id))?.modifiedMs }
        try await save(expectedModifiedMs: current)
    }

    private func save(expectedModifiedMs: Int64?) async throws {
        guard let file = currentFile, !file.isReadOnly else { return }
        do {
            let modified = try await FileIOService.writeFile(
                content: file.content, to: file.id, host: host, expectedModifiedMs: expectedModifiedMs)
            saveConflict = false
            externalChangeAlert = nil
            // Another file opened while this save was in flight keeps its own stamp.
            guard currentFile?.id == file.id else { return }
            loadedModifiedMs = modified
            currentFile?.isDirty = false
            currentFile?.lastModified =
                host.isLocal
                ? FileIOService.fileModificationDate(at: file.id)
                : modified.map { Date(timeIntervalSince1970: Double($0) / 1000) }
        } catch let error as HostError {
            if case .conflict = error { saveConflict = true }
            throw error
        }
    }

    func reloadFile() {
        guard let file = currentFile else { return }
        let path = file.id
        let host = self.host
        Task {
            do {
                let result = try await FileIOService.readTextFile(at: path, host: host)
                guard currentFile?.id == path else { return }
                loadedModifiedMs = result.modifiedMs
                saveConflict = false
                currentFile?.content = result.content
                currentFile?.isDirty = false
                currentFile?.isReadOnly = Self.isTooLargeToSave(result.content, host: host)
                currentFile?.lastModified =
                    host.isLocal
                    ? FileIOService.fileModificationDate(at: path)
                    : Date(timeIntervalSince1970: Double(result.modifiedMs) / 1000)
            } catch {
                openError = "Could not reload \(file.name): \(error.localizedDescription)"
            }
        }
        externalChangeAlert = nil
    }

    func dismissOpenError() {
        openError = nil
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

/// Every live editor, so closing a machine's projects can tell which hold unsaved edits —
/// including the editors of worktree and checkout detail views, not only file-pane tabs.
@MainActor
enum OpenEditors {
    private final class Entry {
        weak var editor: EditorState?
        init(_ editor: EditorState) { self.editor = editor }
    }

    private static var entries: [Entry] = []

    static func register(_ editor: EditorState) {
        entries.removeAll { $0.editor == nil }
        entries.append(Entry(editor))
    }

    /// Names of files with unsaved edits open through `hostIdentity` below `root`.
    static func unsavedFileNames(hostIdentity: String, root: String) -> [String] {
        entries.removeAll { $0.editor == nil }
        let prefix = root.hasSuffix("/") ? root : root + "/"
        return entries.compactMap { entry in
            guard let editor = entry.editor, editor.host.identity == hostIdentity, editor.hasUnsavedChanges,
                let file = editor.currentFile, file.id == root || file.id.hasPrefix(prefix)
            else { return nil }
            return file.name
        }
    }
}
