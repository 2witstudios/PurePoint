import SwiftUI

struct ToolCallCardView: View {
    let name: String?
    let input: String?
    let output: String?
    let isError: Bool
    let status: ToolUseStatus

    @State private var isExpanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            VStack(alignment: .leading, spacing: 10) {
                if let input, !input.isEmpty { detail(input, label: "Input") }
                if let output, !output.isEmpty { detail(output, label: "Output") }
                if (input ?? "").isEmpty && (output ?? "").isEmpty {
                    Text("No output").font(.system(size: 12)).foregroundStyle(.secondary)
                }
            }.padding(.top, 8).padding(.bottom, 4)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(name ?? "Result").font(.system(size: 13, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(statusLabel).font(.system(size: 11)).foregroundStyle(.secondary)
            }.foregroundStyle(isError || status == .failed ? Color.red : Color.secondary)
        }
        .padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityHint("Expand to inspect tool input and output")
    }

    private var statusLabel: String {
        if isError { return "Failed" }
        switch status {
        case .running: return "Running"
        case .completed: return "Done"
        case .failed: return "Failed"
        }
    }

    private func detail(_ text: String, label: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(.system(size: 11)).foregroundStyle(.secondary)
            ScrollView(.horizontal) {
                Text(text).font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(label == "Output" && isError ? Color.red : Color.secondary)
                    .textSelection(.enabled).fixedSize(horizontal: true, vertical: false)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}
