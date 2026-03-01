import SwiftUI

struct SidebarFooter: View {
    var body: some View {
        VStack(spacing: 0) {
            Divider()

            HStack {
                Button {
                    // No-op
                } label: {
                    Image(systemName: "gear")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Text("Settings")
                    .font(PurePointTheme.smallFont)
                    .foregroundStyle(PurePointTheme.secondaryText)

                Spacer()

                Button {
                    // No-op
                } label: {
                    HStack(spacing: 3) {
                        Image(systemName: "plus")
                        Text("Add")
                            .font(PurePointTheme.smallFont)
                    }
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, PurePointTheme.padding + 4)
            .frame(height: PurePointTheme.footerHeight)
        }
    }
}

#Preview {
    SidebarFooter()
        .frame(width: 240)
        .preferredColorScheme(.dark)
}
