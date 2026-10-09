import SwiftUI

struct ProjectChannelView: View {
    let project: ProjectState
    var compact = false
    @State private var atBottom = true
    @State private var searchVisible = false
    @State private var searchText = ""
    @State private var editing: ChannelMessage?
    @State private var editText = ""
    @State private var unreadBoundary: UInt64?
    @State private var jumpRequest = 0
    @State private var unreadRequest = 0
    private var channel: ChannelState { project.channel }

    var body: some View {
        @Bindable var state = channel
        VStack(spacing: 0) {
            HStack(spacing: 9) {
                Text("#").font(.system(size: 22, weight: .medium)).foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 2) {
                    Text(project.projectName).font(.system(size: 14, weight: .semibold))
                    if !compact { Text("A shared place to keep each other up to date").font(.system(size: 11)).foregroundStyle(.secondary) }
                }
                Spacer()
                Button { searchVisible.toggle() } label: { Image(systemName: "magnifyingglass") }
                    .buttonStyle(.borderless).help("Search messages")
                Menu {
                    Button("Mark as read") { channel.markRead(); unreadBoundary = nil }
                    Button("Refresh") { Task { await channel.refresh() } }
                } label: { Image(systemName: "ellipsis") }.menuStyle(.borderlessButton).frame(width: 22)
            }.padding(.horizontal, 20).padding(.vertical, compact ? 12 : 16)
            Divider()
            if channel.unreadCount > 0 && channel.query.isEmpty {
                HStack {
                    Button("\(channel.unreadCount) unread messages ↓") { unreadRequest += 1 }.buttonStyle(.plain)
                    Spacer()
                    Button { channel.markRead(); unreadBoundary = nil } label: { Image(systemName: "xmark") }.buttonStyle(.plain).help("Mark as read")
                }.font(.system(size: 11, weight: .medium)).foregroundStyle(Color.accentColor).padding(.horizontal, 16).padding(.vertical, 8).background(Color.accentColor.opacity(0.08))
            }
            if searchVisible {
                TextField("Search messages, people, branches…", text: $searchText)
                    .textFieldStyle(.roundedBorder).padding(12)
                    .onSubmit { channel.query = searchText; Task { await channel.search() } }
                if !channel.query.isEmpty {
                    Button("Clear search") { channel.query = ""; searchText = ""; Task { await channel.search() } }.buttonStyle(.plain).padding(.bottom, 8)
                }
            }
            if let error = channel.error {
                HStack {
                    Image(systemName: "exclamationmark.circle")
                    Text(error).font(.system(size: 12)).textSelection(.enabled)
                    Spacer()
                    Button("Refresh") { Task { await channel.refresh() } }
                }.foregroundStyle(.secondary).padding(12).background(Color.orange.opacity(0.08))
            }
            if channel.isLoading && channel.messages.isEmpty {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                timeline
            }
            if let id = channel.threadId { thread(id) }
            Divider()
            composer(reply: false)
        }
        .background(Color(nsColor: .textBackgroundColor))
        .onAppear { unreadBoundary = channel.readSequence; searchText = channel.query; channel.start() }
        .onDisappear { channel.stop() }
        .onChange(of: channel.latestSequence) { _, _ in if atBottom && channel.query.isEmpty { channel.markRead() } }
        .sheet(item: $editing) { message in
            VStack(alignment: .leading, spacing: 14) {
                Text("Edit message").font(.headline)
                TextEditor(text: $editText).frame(width: 420, height: 140)
                HStack { Spacer(); Button("Cancel") { editing = nil }; Button("Save") { Task { await channel.edit(message, text: editText); if channel.error == nil { editing = nil } } }.disabled(editText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
                if let error = channel.error { Text(error).font(.caption).foregroundStyle(.red) }
            }.padding(20)
        }
    }
    private var timeline: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if channel.hasMore { Button("Load older messages") { Task { await channel.loadOlder() } }.buttonStyle(.plain).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(16) }
                    if channel.topLevelMessages.isEmpty {
                        VStack(spacing: 10) {
                            Image(systemName: "bubble.left.and.bubble.right").font(.system(size: 30)).foregroundStyle(.tertiary)
                            Text(channel.query.isEmpty ? "The conversation starts here" : "No messages found").font(.system(size: 15, weight: .semibold))
                            if channel.query.isEmpty { Text("Share progress, ask a question, or leave a note for the team.").font(.system(size: 12)).foregroundStyle(.secondary) }
                        }.frame(maxWidth: .infinity).padding(.vertical, 60)
                    }
                    ForEach(Array(channel.topLevelMessages.enumerated()), id: \.element.id) { index, message in
                        let previous = index > 0 ? channel.topLevelMessages[index - 1] : nil
                        if let boundary = unreadBoundary, message.sequence > boundary, previous == nil || previous!.sequence <= boundary {
                            HStack { Rectangle().frame(height: 1); Text("New messages").font(.system(size: 10, weight: .semibold)); Rectangle().frame(height: 1) }.foregroundStyle(Color.accentColor).padding(.horizontal, 20).padding(.vertical, 12)
                        }
                        if previous == nil || day(previous!.createdAt) != day(message.createdAt) {
                            HStack { Rectangle().fill(Color.secondary.opacity(0.15)).frame(height: 1); Text(day(message.createdAt)).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary); Rectangle().fill(Color.secondary.opacity(0.15)).frame(height: 1) }.padding(.horizontal, 20).padding(.vertical, 16)
                        }
                        messageRow(message, grouped: grouped(message, previous)).id(message.id)
                    }
                    Color.clear.frame(height: 1).id("channel-bottom")
                        .onAppear { atBottom = true; channel.markRead() }
                        .onDisappear { atBottom = false }
                }.padding(.bottom, 12)
            }
            .overlay(alignment: .bottom) {
                if !atBottom && channel.query.isEmpty {
                    Button { jumpRequest += 1 } label: { Label(channel.unreadCount > 0 ? "\(channel.unreadCount) new · Jump to latest" : "Jump to latest", systemImage: "arrow.down") }.buttonStyle(.borderedProminent).controlSize(.small).padding(12)
                }
            }
            .onChange(of: unreadRequest) { _, _ in
                if let first = channel.messages.first(where: { $0.sequence > channel.readSequence && $0.author.id != channel.selfAuthorId }) {
                    withAnimation { proxy.scrollTo(first.parentId ?? first.id, anchor: .top) }
                    if let parent = first.parentId { Task { await channel.openThread(parent) } }
                }
            }
            .onChange(of: jumpRequest) { _, _ in withAnimation { proxy.scrollTo("channel-bottom", anchor: .bottom) } }
            .onChange(of: channel.messages.last?.id) { _, _ in if atBottom && channel.query.isEmpty { proxy.scrollTo("channel-bottom", anchor: .bottom) } }
        }
    }
    private func messageRow(_ message: ChannelMessage, grouped: Bool = false) -> some View {
        HStack(alignment: .top, spacing: 11) {
            if grouped { Text(time(message.createdAt)).font(.system(size: 9)).foregroundStyle(.tertiary).frame(width: 34).padding(.top, 3) }
            else { avatar(message.author) }
            VStack(alignment: .leading, spacing: 5) {
                if !grouped {
                    HStack(spacing: 7) {
                        Text(message.author.name).font(.system(size: 13, weight: .semibold))
                        if message.author.kind == "agent" { Text(message.author.agentType ?? "agent").font(.system(size: 9, weight: .medium)).foregroundStyle(.secondary).padding(.horizontal, 4).padding(.vertical, 2).background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 3)) }
                        Text(time(message.createdAt)).font(.system(size: 10)).foregroundStyle(.tertiary)
                        if let branch = message.author.branch { Text(branch).font(.system(size: 10, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1) }
                    }
                }
                Text((try? AttributedString(markdown: message.text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(message.text)).font(.system(size: 13)).lineSpacing(4).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                if message.editedAt != nil { Text("edited").font(.system(size: 10)).foregroundStyle(.tertiary) }
                if !message.references.isEmpty {
                    HStack { ForEach(Array(message.references.enumerated()), id: \.offset) { _, ref in
                        Label(ref.label ?? (ref.kind == "pr" ? "PR #\(ref.value)" : String(ref.value.prefix(8))), systemImage: ref.kind == "pr" ? "arrow.triangle.pull" : "point.3.connected.trianglepath.dotted").font(.system(size: 11, design: .monospaced)).padding(.horizontal, 7).padding(.vertical, 4).background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 4)).textSelection(.enabled)
                    } }
                }
                HStack(spacing: 10) {
                    ForEach(message.reactions, id: \.emoji) { reaction in
                        Button { Task { await channel.react(message) } } label: { Text("\(reaction.emoji) \(reaction.authorIds.count)").font(.system(size: 11)).padding(.horizontal, 6).padding(.vertical, 3).background(reaction.authorIds.contains(channel.selfAuthorId) ? Color.accentColor.opacity(0.12) : Color.secondary.opacity(0.06), in: Capsule()) }.buttonStyle(.plain)
                    }
                    if let count = channel.replyCounts[message.id], count > 0 {
                        Button("\(count) \(count == 1 ? "reply" : "replies")") { Task { await channel.openThread(message.id) } }.buttonStyle(.plain).font(.system(size: 11, weight: .medium)).foregroundStyle(Color.accentColor)
                    }
                }
            }
            Menu {
                if message.parentId == nil { Button("Reply in thread") { Task { await channel.openThread(message.id) } } }
                Button("React 👍") { Task { await channel.react(message) } }
                Button("Copy text") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(message.text, forType: .string) }
                if message.author.id == channel.selfAuthorId { Button("Edit message") { editText = message.text; editing = message } }
            } label: { Image(systemName: "ellipsis").foregroundStyle(.secondary) }.menuStyle(.borderlessButton).frame(width: 18).help("Message actions")
        }.padding(.horizontal, 20).padding(.top, grouped ? 3 : 12).padding(.bottom, grouped ? 3 : 6)
    }
    private func thread(_ id: String) -> some View {
        VStack(spacing: 0) {
            Divider()
            HStack { Text("Thread").font(.system(size: 12, weight: .semibold)); Spacer(); Button { channel.saveReplyDraft(); channel.threadId = nil } label: { Image(systemName: "xmark") }.buttonStyle(.plain).help("Close thread") }.padding(12)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let parent = channel.messages.first(where: { $0.id == id }) { messageRow(parent) }
                    if channel.threadHasMore { Button("Load older replies") { Task { await channel.loadOlderReplies() } }.padding(12) }
                    ForEach(channel.threadMessages) { messageRow($0) }
                }
            }.frame(maxHeight: 220)
            composer(reply: true)
        }.background(Color.secondary.opacity(0.035))
    }
    private func composer(reply: Bool) -> some View {
        @Bindable var state = channel
        let binding = reply ? $state.replyDraft : $state.draft
        return VStack(alignment: .leading, spacing: 7) {
            ZStack(alignment: .topLeading) {
                if binding.wrappedValue.isEmpty { Text(reply ? "Reply to thread…" : "Message #\(project.projectName)").font(.system(size: 13)).foregroundStyle(.tertiary).padding(.leading, 12).padding(.top, 10).allowsHitTesting(false) }
                ChannelComposer(text: binding, placeholder: reply ? "Reply" : "Message channel") { Task { await channel.send(reply: reply) } }.frame(height: 60)
            }
            HStack(spacing: 10) {
                Menu {
                    ForEach(project.allAgents) { agent in Button("@\(agent.displayName)") { binding.wrappedValue += "@\(agent.displayName) " } }
                } label: { Text("@").font(.system(size: 15, weight: .medium)) }.menuStyle(.borderlessButton).frame(width: 25).help("Mention someone")
                Text("Enter to send · Shift Enter for a new line").font(.system(size: 10)).foregroundStyle(.tertiary)
                Spacer()
                Button { Task { await channel.send(reply: reply) } } label: { if channel.isSending { ProgressView().controlSize(.small) } else { Image(systemName: "arrow.up").font(.system(size: 12, weight: .bold)) } }.buttonStyle(.borderedProminent).controlSize(.small).disabled(channel.isSending || binding.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty).help("Send message")
            }.padding(.horizontal, 10).padding(.bottom, 8)
        }.background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8)).overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.secondary.opacity(0.2))).padding(.horizontal, 16).padding(.vertical, 12)
        .onChange(of: channel.replyDraft) { _, _ in if reply { channel.saveReplyDraft() } }
    }
    private func avatar(_ author: ChannelAuthor) -> some View {
        Text(author.name.split(separator: " ").prefix(2).compactMap { $0.first }.map(String.init).joined()).font(.system(size: 11, weight: .semibold)).foregroundStyle(author.kind == "agent" ? Color.accentColor : Color.primary).frame(width: 34, height: 34).background(author.kind == "agent" ? Color.accentColor.opacity(0.13) : Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
    }
    private func date(_ value: String) -> Date? { ISO8601DateFormatter().date(from: value) ?? { let f = ISO8601DateFormatter(); f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]; return f.date(from: value) }() }
    private func time(_ value: String) -> String { date(value)?.formatted(date: .omitted, time: .shortened) ?? value }
    private func day(_ value: String) -> String { date(value)?.formatted(date: .abbreviated, time: .omitted) ?? String(value.prefix(10)) }
    private func grouped(_ value: ChannelMessage, _ previous: ChannelMessage?) -> Bool {
        guard let previous, previous.author.id == value.author.id, let a = date(value.createdAt), let b = date(previous.createdAt) else { return false }
        return a.timeIntervalSince(b) < 300 && day(value.createdAt) == day(previous.createdAt)
    }
}
