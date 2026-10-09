import Foundation
import Observation

@Observable @MainActor
final class ChannelState {
    let projectRoot: String
    var messages: [ChannelMessage] = []
    var replyCounts: [String: UInt64] = [:]
    var selfAuthorId = ""
    var readError: String?
    var mutationError: String?
    var error: String? { mutationError ?? readError }
    var isLoading = false
    var isSending = false
    var hasMore = false
    var latestSequence: UInt64 = 0
    var readSequence: UInt64
    var draft: String { didSet { defaults.set(draft, forKey: key("draft")) } }
    var query = ""
    var threadId: String?
    var threadMessages: [ChannelMessage] = []
    var threadHasMore = false
    var replyDraft = ""
    @ObservationIgnored private let sendRequest: (DaemonRequest) async throws -> DaemonResponse
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var refreshing = false
    @ObservationIgnored private var refreshAgain = false
    @ObservationIgnored private var revision: UInt64?
    @ObservationIgnored private var oldest: UInt64?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var viewers = 0
    @ObservationIgnored private var backgroundEnabled = false
    @ObservationIgnored private var loadedBefore: [UInt64] = []
    @ObservationIgnored private var threadOldest: UInt64?
    private var unreadAuthors: [UInt64: String] = [:]
    private var unreadMessages: [UInt64: ChannelMessage] = [:]
    private var seenSequences: Set<UInt64> = []
    @ObservationIgnored private var unreadScannedThrough: UInt64 = 0
    var unreadCount: Int { unreadAuthors.filter { $0.key > readSequence && $0.value != selfAuthorId && !seenSequences.contains($0.key) }.count }
    var topLevelMessages: [ChannelMessage] { messages.filter { $0.parentId == nil } }

    init(projectRoot: String, defaults: UserDefaults = .standard, sendRequest: @escaping (DaemonRequest) async throws -> DaemonResponse = { try await DaemonClient().send($0) }) {
        self.sendRequest = sendRequest
        self.projectRoot = projectRoot
        self.defaults = defaults
        let prefix = "channel.\(projectRoot)."
        draft = defaults.string(forKey: prefix + "draft") ?? ""
        readSequence = UInt64(defaults.string(forKey: prefix + "read") ?? "0") ?? 0
        seenSequences = Set((defaults.stringArray(forKey: prefix + "seen") ?? []).compactMap(UInt64.init))
    }
    private func key(_ suffix: String) -> String { "channel.\(projectRoot).\(suffix)" }
    func start() {
        viewers += 1
        ensurePolling()
    }
    func startBackground() { backgroundEnabled = true; ensurePolling() }
    private func ensurePolling() {
        guard polling == nil else { return }
        polling = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                do { try await Task.sleep(for: .seconds(self?.viewers == 0 ? 15 : 3)) } catch { return }
            }
        }
    }
    func stop() {
        viewers = max(0, viewers - 1)
        if viewers == 0 && !backgroundEnabled { polling?.cancel(); polling = nil }
    }
    func shutdown() { polling?.cancel(); polling = nil; viewers = 0; backgroundEnabled = false; generation += 1 }
    private func history(after: UInt64? = nil, before: UInt64? = nil, parent: String? = nil, known: UInt64? = nil) async throws -> ChannelHistory {
        let response = try await sendRequest(.channelRead(projectRoot: projectRoot, after: after, before: before, query: after != nil || parent != nil || query.isEmpty ? nil : query, parentId: parent, knownRevision: known))
        switch response {
        case .channelHistory(let value): return value
        case .error(_, let message): throw ChannelFailure(message: message)
        default: throw ChannelFailure(message: "Unexpected channel response")
        }
    }
    func search() async {
        generation += 1; loadedBefore = []; oldest = nil; revision = nil
        await refresh()
    }
    func refresh() async {
        guard !refreshing else { refreshAgain = true; return }
        refreshing = true
        defer {
            refreshing = false
            if refreshAgain { refreshAgain = false; Task { await refresh() } }
        }
        let token = generation
        isLoading = messages.isEmpty
        defer { isLoading = false }
        do {
            let latest = try await history(known: query.isEmpty ? revision : nil)
            guard token == generation, !Task.isCancelled else { return }
            selfAuthorId = latest.selfAuthorId; latestSequence = latest.latestSequence
            if !latest.unchanged {
                var all = latest.messages
                var counts = latest.replyCounts
                var oldestCursor = latest.oldestSequence
                var more = latest.hasMore
                let desiredOldest = loadedBefore.isEmpty ? nil : oldest
                var refreshedCursors: [UInt64] = []
                while more, let cursor = oldestCursor, let desiredOldest, cursor > desiredOldest {
                    let page = try await history(before: cursor)
                    guard token == generation, !Task.isCancelled else { return }
                    refreshedCursors.append(cursor)
                    all += page.messages; counts.merge(page.replyCounts) { _, new in new }
                    oldestCursor = page.oldestSequence; more = page.hasMore
                }
                loadedBefore = refreshedCursors
                messages = deduplicated(all); replyCounts = counts; oldest = oldestCursor; hasMore = more
                revision = latest.revision
                if let threadId { try await refreshThread(threadId, token: token) }
            }
            try await scanUnread(token: token)
            readError = nil
        } catch is CancellationError {} catch { if token == generation { self.readError = error.localizedDescription } }
    }
    private func scanUnread(token: Int) async throws {
        var cursor = max(readSequence, unreadScannedThrough)
        while cursor < latestSequence {
            let page = try await history(after: cursor)
            guard token == generation, !Task.isCancelled else { return }
            let created = page.messages.filter { $0.sequence > cursor }
            guard let last = created.map(\.sequence).max() else { break }
            for message in created { unreadAuthors[message.sequence] = message.author.id; unreadMessages[message.sequence] = message }
            cursor = last
            unreadScannedThrough = cursor
            if !page.hasMore { break }
        }
        advanceReadCursor()
    }
    func loadOlder() async {
        guard let cursor = oldest, hasMore else { return }
        let token = generation
        do {
            let page = try await history(before: cursor)
            guard token == generation else { return }
            loadedBefore.append(cursor); messages = deduplicated(messages + page.messages)
            replyCounts.merge(page.replyCounts) { _, new in new }; oldest = page.oldestSequence; hasMore = page.hasMore
        } catch { self.readError = error.localizedDescription }
    }
    func openThread(_ id: String) async {
        saveReplyDraft()
        threadId = id; threadMessages = []; threadOldest = nil; threadHasMore = false
        replyDraft = defaults.string(forKey: key("reply.\(id)")) ?? ""
        do { try await refreshThread(id, token: generation) } catch { self.readError = error.localizedDescription }
    }
    private func refreshThread(_ id: String, token: Int) async throws {
        let page = try await history(parent: id)
        guard token == generation, threadId == id else { return }
        var all = page.messages.filter { $0.parentId == id }
        var cursor = page.oldestSequence
        var more = page.hasMore
        // Refresh all previously loaded replies, including edits outside the newest page.
        let previousOldest = threadOldest
        while more, let before = cursor, let previousOldest, before > previousOldest {
            let older = try await history(before: before, parent: id)
            guard token == generation, threadId == id else { return }
            all += older.messages.filter { $0.parentId == id }; cursor = older.oldestSequence; more = older.hasMore
        }
        threadMessages = deduplicated(all); threadOldest = cursor; threadHasMore = more
    }
    func loadOlderReplies() async {
        guard let id = threadId, let cursor = threadOldest, threadHasMore else { return }
        do {
            let page = try await history(before: cursor, parent: id)
            guard threadId == id else { return }
            threadMessages = deduplicated(threadMessages + page.messages.filter { $0.parentId == id })
            threadOldest = page.oldestSequence; threadHasMore = page.hasMore
        } catch { self.readError = error.localizedDescription }
    }
    func saveReplyDraft() { if let threadId { defaults.set(replyDraft, forKey: key("reply.\(threadId)")) } }
    func markVisible(_ message: ChannelMessage) {
        guard query.isEmpty else { return }
        seenSequences.insert(message.sequence)
        advanceReadCursor()
    }
    private func advanceReadCursor() {
        while readSequence < unreadScannedThrough {
            let next = readSequence + 1
            guard let author = unreadAuthors[next], author == selfAuthorId || seenSequences.contains(next) else { break }
            readSequence = next
            seenSequences.remove(next); unreadAuthors.removeValue(forKey: next); unreadMessages.removeValue(forKey: next)
        }
        defaults.set(String(readSequence), forKey: key("read"))
        defaults.set(seenSequences.map(String.init), forKey: key("seen"))
    }
    func firstUnread() -> ChannelMessage? {
        unreadMessages.values.filter { $0.sequence > readSequence && $0.author.id != selfAuthorId && !seenSequences.contains($0.sequence) }.min { $0.sequence < $1.sequence }
    }
    func revealFirstUnread() async -> String? {
        guard let first = firstUnread() else { return nil }
        while !messages.contains(where: { $0.id == first.id }), hasMore {
            let previous = oldest
            await loadOlder()
            if oldest == previous { break }
        }
        if let parent = first.parentId { await openThread(parent); return parent }
        return first.id
    }
    func markUnread(_ message: ChannelMessage) {
        readSequence = min(readSequence, message.sequence - 1)
        seenSequences = seenSequences.filter { $0 < message.sequence }
        unreadScannedThrough = min(unreadScannedThrough, readSequence)
        defaults.set(String(readSequence), forKey: key("read"))
        defaults.set(seenSequences.map(String.init), forKey: key("seen"))
        Task { await refresh() }
    }
    func markRead() {
        guard query.isEmpty else { return }
        readSequence = latestSequence; unreadAuthors = [:]; unreadMessages = [:]; seenSequences = []
        defaults.set(String(readSequence), forKey: key("read")); defaults.removeObject(forKey: key("seen"))
    }
    func send(reply: Bool = false) async {
        let targetParent = reply ? threadId : nil
        let text = reply ? replyDraft : draft
        guard !isSending, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        isSending = true; defer { isSending = false }
        do {
            try await mutate(.channelSend(projectRoot: projectRoot, text: text, parentId: targetParent))
            if reply, let targetParent {
                let savedKey = key("reply.\(targetParent)")
                if defaults.string(forKey: savedKey) == text { defaults.removeObject(forKey: savedKey) }
                if threadId == targetParent && replyDraft == text { replyDraft = ""; saveReplyDraft() }
            }
            else if draft == text { draft = "" }
            mutationError = nil; revision = nil; await refresh()
        } catch { self.mutationError = "Send was not confirmed. Check the channel before trying again. \(error.localizedDescription)" }
    }
    func edit(_ message: ChannelMessage, text: String) async {
        do { try await mutate(.channelEdit(projectRoot: projectRoot, messageId: message.id, text: text)); mutationError = nil; revision = nil; await refresh() }
        catch { self.mutationError = error.localizedDescription }
    }
    func react(_ message: ChannelMessage) async {
        let active = !message.reactions.contains { $0.emoji == "👍" && $0.authorIds.contains(selfAuthorId) }
        do { try await mutate(.channelReact(projectRoot: projectRoot, messageId: message.id, active: active)); mutationError = nil; revision = nil; await refresh() }
        catch { self.mutationError = error.localizedDescription }
    }
    private func mutate(_ request: DaemonRequest) async throws {
        let response = try await sendRequest(request)
        switch response {
        case .channelMessage: return
        case .error(_, let message): throw ChannelFailure(message: message)
        default: throw ChannelFailure(message: "Unexpected channel response")
        }
    }
    private func deduplicated(_ values: [ChannelMessage]) -> [ChannelMessage] {
        var byId: [String: ChannelMessage] = [:]
        for value in values { byId[value.id] = value }
        return byId.values.sorted { $0.sequence < $1.sequence }
    }
}
nonisolated private struct ChannelFailure: LocalizedError { let message: String; var errorDescription: String? { message } }
