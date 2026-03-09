import SwiftUI

struct ContentBlockView: View {
    let block: ContentBlock
    var allBlocks: [ContentBlock] = []

    var body: some View {
        switch block {
        case .text(_, let text):
            MarkdownTextView(text: text)
        case .codeBlock(_, let language, let code):
            CodeBlockView(language: language, code: code)
        case .toolUse(let id, let name, let input, let status):
            let result = allBlocks.lazy.compactMap { b -> (String, Bool)? in
                if case .toolResult(_, let tid, let o, let e) = b, tid == id { return (o, e) }
                return nil
            }.first
            ToolCallCardView(
                name: name, input: input,
                output: result?.0,
                isError: result?.1 ?? false,
                status: status
            )
        case .toolResult(_, let toolUseId, let output, let isError):
            if allBlocks.contains(where: { b in
                if case .toolUse(let id, _, _, _) = b, id == toolUseId { return true }
                return false
            }) {
                EmptyView()
            } else {
                ToolCallCardView(
                    name: nil, input: nil, output: output, isError: isError,
                    status: isError ? .failed : .completed
                )
            }
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
