import SwiftUI

struct ProjectRow: View {
    let name: String

    var body: some View {
        Label {
            Text(name)
                .font(PurePointTheme.treeFont)
                .fontWeight(.semibold)
        } icon: {
            Image(systemName: "folder.fill")
                .foregroundStyle(.secondary)
        }
    }
}
