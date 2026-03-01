import SwiftUI

struct SidebarView: View {
    // Two selection bindings: `selection` (parent-owned, includes nav items) and
    // `treeSelection` (List-owned, tree items only). Synchronized via onChange handlers.
    @Binding var selection: SidebarSelection?
    @State private var treeSelection: SidebarSelection?

    var body: some View {
        VStack(spacing: 0) {
            NavButtonsSection(selection: $selection)

            Divider()
                .padding(.horizontal, PurePointTheme.padding)

            ProjectTreeSection(selection: $treeSelection)

            SidebarFooter()
        }
        .onChange(of: treeSelection) { _, newValue in
            if newValue != nil {
                selection = newValue
            }
        }
        .onChange(of: selection) { _, newValue in
            if case .nav = newValue {
                treeSelection = nil
            }
        }
    }
}
