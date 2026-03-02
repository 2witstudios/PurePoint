import SwiftUI

struct ProjectRow: View {
    let name: String
    var onAdd: () -> Void

    var body: some View {
        Label {
            HStack {
                Text(name)
                    .font(PurePointTheme.treeFont)
                    .fontWeight(.semibold)

                Spacer()

                Button { onAdd() } label: {
                    Image(systemName: "plus.circle")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        } icon: {
            Image(systemName: "folder.fill")
                .foregroundStyle(.secondary)
        }
    }
}
