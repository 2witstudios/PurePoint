import SwiftUI

struct ProjectRow: View {
    let project: MockProject

    var body: some View {
        Label {
            Text(project.name)
                .font(PurePointTheme.treeFont)
                .fontWeight(.semibold)
        } icon: {
            Image(systemName: "folder.fill")
                .foregroundStyle(.secondary)
        }
    }
}

#Preview {
    ProjectRow(project: MockData.project)
        .padding()
        .preferredColorScheme(.dark)
}
