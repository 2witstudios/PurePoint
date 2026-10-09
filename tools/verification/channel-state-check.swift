import Foundation

@main struct ChannelStateCheck {
    @MainActor static func main() async throws {
        let fixture = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        guard case .channelHistory(let decoded) = try JSONDecoder().decode(DaemonResponse.self, from: fixture) else { fatalError("wire response") }
        precondition(decoded.oldestSequence == 7 && decoded.messages.count == 2 && decoded.selfAuthorId == "human:501")
        for request in [DaemonRequest.channelRead(projectRoot: "/project", before: 7, parentId: "parent"), .channelSend(projectRoot: "/project", text: "hello\nworld"), .channelEdit(projectRoot: "/project", messageId: "m", text: "edit"), .channelReact(projectRoot: "/project", messageId: "m", active: true)] {
            let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as! [String: Any]
            precondition(json["project_root"] as? String == "/project")
            precondition((json["type"] as? String)?.hasPrefix("channel_") == true)
        }
        let suite = "channel-check.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let author = ChannelAuthor(id: "agent:a", name: "Agent", kind: "agent", agentType: "codex", worktreeId: nil, branch: nil)
        func message(_ sequence: UInt64, parent: String? = nil, text: String = "hello") -> ChannelMessage {
            ChannelMessage(id: "m\(sequence)", sequence: sequence, parentId: parent, author: author, text: text, createdAt: "2026-10-09T12:00:00Z", editedAt: nil, references: [], reactions: [])
        }
        var revision: UInt64 = 1
        var edited = false
        var sends = 0
        var beforeCursors: [UInt64] = []
        let state = ChannelState(projectRoot: "/project", defaults: defaults) { request in
            switch request {
            case .channelRead(_, let after, let before, _, let query, let parent, let known):
                if parent != nil {
                    precondition(query == nil, "thread must not inherit search")
                    return .channelHistory(ChannelHistory(messages: [message(1), message(250, parent: "m1")], revision: revision, latestSequence: 250, hasMore: false, oldestSequence: 250, unchanged: false, selfAuthorId: "human:501", replyCounts: ["m1": 1]))
                }
                if let after {
                    let end = min(after + 100, 250)
                    let values = ((after+1)...end).map { message($0) }
                    return .channelHistory(ChannelHistory(messages: values, revision: revision, latestSequence: 250, hasMore: end < 250, oldestSequence: after+1, unchanged: false, selfAuthorId: "human:501", replyCounts: [:]))
                }
                if let before { beforeCursors.append(before) }
                let values = before == nil ? [message(1), message(200), message(250, parent: "m1")] : [message(100, text: edited ? "edited old message" : "original")]
                return .channelHistory(ChannelHistory(messages: known == revision ? [] : values, revision: revision, latestSequence: 250, hasMore: before == nil, oldestSequence: before == nil ? 200 : 100, unchanged: known == revision, selfAuthorId: "human:501", replyCounts: ["m1": 1]))
            case .channelSend:
                sends += 1
                throw NSError(domain: "unconfirmed", code: 1)
            default: fatalError("unexpected request")
            }
        }
        state.draft = "keep this draft"
        precondition(ChannelState(projectRoot: "/project", defaults: defaults).draft == "keep this draft")
        await state.refresh()
        precondition(state.unreadCount == 250, "must count history beyond latest page")
        state.markVisible(message(200))
        precondition(state.unreadCount == 249 && state.readSequence == 0, "viewing latest must not clear older messages or collapsed replies")
        await state.loadOlder()
        precondition(beforeCursors == [200], "parent context must not move pagination cursor")
        revision = 2; edited = true
        await state.refresh()
        precondition(state.messages.first(where: { $0.id == "m100" })?.text == "edited old message", "refresh loaded old edits")
        state.query = "search"
        await state.openThread("m1")
        precondition(state.threadMessages.count == 1 && state.threadMessages.first?.sequence == 250)
        await state.send()
        precondition(sends == 1 && state.draft == "keep this draft" && state.error != nil, "uncertain send must retain draft and never replay")
        await state.refresh(); precondition(state.mutationError != nil, "poll must not erase uncertain delivery warning")
        state.query = ""; state.markRead(); precondition(state.unreadCount == 0)
        precondition(ChannelState(projectRoot: "/project", defaults: defaults).readSequence == 250)
        var continuation: CheckedContinuation<DaemonResponse, Never>?
        var capturedParent: String?
        let switching = ChannelState(projectRoot: "/thread-switch", defaults: defaults) { request in
            switch request {
            case .channelRead:
                return .channelHistory(ChannelHistory(messages: [], revision: 1, latestSequence: 0, hasMore: false, oldestSequence: nil, unchanged: false, selfAuthorId: "human:501", replyCounts: [:]))
            case .channelSend(_, _, let parent, _):
                capturedParent = parent
                return await withCheckedContinuation { continuation = $0 }
            default: fatalError("unexpected mutation")
            }
        }
        await switching.openThread("A")
        switching.replyDraft = "same text"; switching.saveReplyDraft()
        let delivery = Task { await switching.send(reply: true) }
        while continuation == nil { await Task.yield() }
        await switching.openThread("B")
        switching.replyDraft = "same text"; switching.saveReplyDraft()
        continuation!.resume(returning: .channelMessage(ChannelMutation(message: message(1), revision: 2)))
        await delivery.value
        precondition(capturedParent == "A" && switching.replyDraft == "same text", "reply must preserve newly selected thread draft")
        precondition(defaults.string(forKey: "channel./thread-switch.reply.A") == nil)
        precondition(defaults.string(forKey: "channel./thread-switch.reply.B") == "same text")
        print("PASS channel wire, old-history refresh, parent cursors, thread search isolation, unread catch-up, durable draft/read and uncertain send")
    }
}
