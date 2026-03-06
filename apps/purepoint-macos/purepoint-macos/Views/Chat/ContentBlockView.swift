import SwiftUI

struct ContentBlockView: View {
    let block: ContentBlock

    var body: some View {
        switch block {
        case .text(_, let text):
            MarkdownTextView(text: text)
        case .codeBlock(_, let language, let code):
            CodeBlockView(language: language, code: code)
        case .toolUse(_, let name, let input, let status):
            ToolCallCardView(name: name, input: input, output: nil, isError: false, status: status)
        case .toolResult(_, _, let output, let isError):
            ToolCallCardView(name: nil, input: nil, output: output, isError: isError, status: isError ? .failed : .completed)
        case .pulse(_, let summary):
            pulseCard(summary)
        }
    }

    private func pulseCard(_ summary: PulseSummary) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: "waveform.path.ecg")
                    .font(.system(size: 12))
                    .foregroundStyle(.blue)
                Text("\(summary.activeAgents) active agent\(summary.activeAgents == 1 ? "" : "s")")
                    .font(.system(size: 12, weight: .medium))
            }

            ForEach(Array(summary.recentEvents.prefix(3).enumerated()), id: \.offset) { _, event in
                HStack(spacing: 6) {
                    Circle()
                        .fill(Color.green)
                        .frame(width: 6, height: 6)
                    Text("\(event.agent): \(event.event)")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(10)
        .background(Color.blue.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
    }
}
