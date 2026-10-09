import SwiftUI

/// Actions surface on pointer hover and keyboard focus without crowding the timeline.
struct ChannelHoverRow<Content: View>: View {
    let canReply: Bool
    let canEdit: Bool
    let reply: () -> Void
    let react: () -> Void
    let copy: () -> Void
    let unread: () -> Void
    let edit: () -> Void
    @ViewBuilder let content: () -> Content
    @State private var hovering = false
    @FocusState private var focus: Target?
    private enum Target: Hashable { case row, reply, reaction, menu }
    var body: some View {
        content()
            .background(hovering ? Color.secondary.opacity(0.04) : Color.clear)
            .focusable().focused($focus, equals: .row)
            .overlay(alignment: .topTrailing) {
                HStack(spacing: 9) {
                    Button(action: react) { Image(systemName: "hand.thumbsup") }.help("React 👍").focused($focus, equals: .reaction)
                    if canReply { Button(action: reply) { Image(systemName: "bubble.left") }.help("Reply in thread").focused($focus, equals: .reply) }
                    Menu {
                        Button("Copy text", action: copy)
                        Button("Mark unread", action: unread)
                        if canEdit { Button("Edit message", action: edit) }
                    } label: { Image(systemName: "ellipsis") }.menuStyle(.borderlessButton).frame(width: 17).focused($focus, equals: .menu)
                }.buttonStyle(.plain).font(.system(size: 12)).foregroundStyle(.secondary)
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.2)))
                    .padding(.trailing, 16).padding(.top, 2)
                    .opacity(hovering || focus != nil ? 1 : 0)
            }
            .onHover { hovering = $0 }
            .accessibilityAction(named: "React", react)
            .accessibilityAction(named: "Mark unread", unread)
    }
}
