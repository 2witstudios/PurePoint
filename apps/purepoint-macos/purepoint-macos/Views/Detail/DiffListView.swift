import SwiftUI

/// Scrollable list of diff cards with loading, empty, and error states.
struct DiffListView: View {
    let diff: DiffData?
    let isLoading: Bool
    let emptyMessage: String
    let error: String?
    var onRetry: (() -> Void)?

    var body: some View {
        Group {
            if isLoading && diff == nil {
                loadingState
            } else if let error {
                errorState(error)
            } else if let diff, !diff.files.isEmpty {
                diffCards(diff)
            } else {
                emptyState
            }
        }
    }

    private var loadingState: some View {
        VStack(spacing: 12) {
            ProgressView()
                .scaleEffect(0.8)
            Text("Loading…")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func errorState(_ message: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 28))
                .foregroundStyle(.secondary)
            Text(message)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            if let onRetry {
                Button("Retry") { onRetry() }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func diffCards(_ diff: DiffData) -> some View {
        ScrollView {
            VStack(spacing: 12) {
                ForEach(diff.files) { file in
                    DiffCardView(fileDiff: file)
                }
            }
            .padding(16)
        }
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "checkmark.circle")
                .font(.system(size: 28))
                .foregroundStyle(.green.opacity(0.7))
            Text(emptyMessage)
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
