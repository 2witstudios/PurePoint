#if os(iOS)
import SwiftUI

/// iOS twin of SwiftUI's macOS-only `HSplitView`: the same children side by side with a
/// divider between each, so the shared detail layouts keep their call sites. The children keep
/// their own frame limits; dragging the divider is not offered.
struct HSplitView<Content: View>: View {
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        Group(subviews: content) { subviews in
            HStack(spacing: 0) {
                ForEach(Array(subviews.enumerated()), id: \.element.id) { index, subview in
                    if index > 0 { Divider() }
                    subview
                }
            }
        }
    }
}
#endif
