import SwiftUI

struct ToolCallCardView: View {
    let name: String?
    let input: String?
    let output: String?
    let isError: Bool
    let status: ToolUseStatus

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: toolIcon)
                        .font(.system(size: 12))
                        .foregroundStyle(iconColor)
                        .frame(width: 16)

                    if let name {
                        Text(name)
                            .font(.system(size: 12, weight: .medium))
                    } else {
                        Text("Result")
                            .font(.system(size: 12, weight: .medium))
                    }

                    Spacer()

                    statusIndicator
                }
                .padding(.vertical, 6)
            }
            .buttonStyle(.plain)

            if isExpanded {
                VStack(alignment: .leading, spacing: 6) {
                    if let input {
                        Text(input)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .lineLimit(10)
                            .textSelection(.enabled)
                    }
                    if let output {
                        Text(output)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(isError ? .red : .secondary)
                            .lineLimit(20)
                            .textSelection(.enabled)
                    }
                }
                .padding(.leading, 24)
            }
        }
    }

    private var toolIcon: String {
        guard let name else { return "arrow.turn.down.left" }
        switch name {
        case "Bash": return "terminal"
        case "Read": return "doc.text"
        case "Edit", "Write": return "pencil"
        case "Glob": return "magnifyingglass"
        case "Grep": return "text.magnifyingglass"
        case "Agent": return "person.2"
        default: return "wrench"
        }
    }

    private var iconColor: Color {
        switch status {
        case .running: .blue
        case .completed: .green
        case .failed: .red
        }
    }

    @ViewBuilder
    private var statusIndicator: some View {
        switch status {
        case .running:
            ProgressView()
                .controlSize(.mini)
        case .completed:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 12))
                .foregroundStyle(.green)
        case .failed:
            Image(systemName: "xmark.circle.fill")
                .font(.system(size: 12))
                .foregroundStyle(.red)
        }
    }
}
