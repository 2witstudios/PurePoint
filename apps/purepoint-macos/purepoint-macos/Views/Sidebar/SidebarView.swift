import SwiftUI

struct SidebarView: View {
    @Binding var selection: SidebarSelection?
    @State private var treeSelection: SidebarSelection?

    var body: some View {
        VStack(spacing: 0) {
            NavButtonsSection(selection: $selection)

            Divider()
                .padding(.horizontal, PurePointTheme.padding)

            ProjectTreeSection(
                projects: MockData.projects,
                selection: $treeSelection
            )

            SidebarFooter()
        }
        .onChange(of: treeSelection) { _, newValue in
            if newValue != nil {
                selection = newValue
            }
        }
        .onChange(of: selection) { _, newValue in
            // When a nav button is tapped, clear the tree's selection highlight
            if case .nav = newValue {
                treeSelection = nil
            }
        }
    }
}

#Preview {
    SidebarView(selection: .constant(.nav(.dashboard)))
        .frame(width: 240, height: 500)
        .preferredColorScheme(.dark)
}
