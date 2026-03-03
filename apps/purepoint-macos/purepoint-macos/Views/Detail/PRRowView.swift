import SwiftUI

/// Row displaying a pull request's metadata in the PR list.
struct PRRowView: View {
    let pr: PullRequestInfo
    var isSelected: Bool = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            // Title line: #number title
            HStack(spacing: 6) {
                Text("#\(pr.number)")
                    .font(.system(size: 12, weight: .semibold, design: .monospaced))
                    .foregroundStyle(.secondary)

                Text(pr.title)
                    .font(.system(size: 13, weight: .medium))
                    .lineLimit(1)

                Spacer(minLength: 4)

                if pr.isDraft {
                    draftBadge
                }

                reviewIndicator
            }

            // Metadata line: author + labels + stats
            HStack(spacing: 8) {
                // Author
                Label(pr.author.login, systemImage: "person")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)

                // Branch
                Label(pr.headRefName, systemImage: "arrow.triangle.branch")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)

                Spacer(minLength: 4)

                // Stats
                statsView
            }

            // Labels
            if !pr.labels.isEmpty {
                labelsView
            }
        }
        .padding(.vertical, 6)
        .padding(.horizontal, 10)
        .background(isSelected ? Color.accentColor.opacity(0.1) : Color.clear)
        .contentShape(Rectangle())
    }

    private var draftBadge: some View {
        Text("Draft")
            .font(.system(size: 10, weight: .medium))
            .foregroundColor(.secondary)
            .padding(.horizontal, 5)
            .padding(.vertical, 1)
            .background(Color.secondary.opacity(0.15))
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    private var reviewIndicator: some View {
        Group {
            switch pr.reviewDecision {
            case "APPROVED":
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.green)
                    .font(.system(size: 12))
            case "CHANGES_REQUESTED":
                Image(systemName: "xmark.circle.fill")
                    .foregroundColor(.orange)
                    .font(.system(size: 12))
            case "REVIEW_REQUIRED":
                Image(systemName: "circle")
                    .foregroundColor(.secondary)
                    .font(.system(size: 12))
            default:
                EmptyView()
            }
        }
    }

    private var statsView: some View {
        HStack(spacing: 4) {
            if pr.additions > 0 {
                Text("+\(pr.additions)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.additionText))
            }
            if pr.deletions > 0 {
                Text("-\(pr.deletions)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.deletionText))
            }
            Text("\(pr.changedFiles) files")
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
        }
    }

    private var labelsView: some View {
        HStack(spacing: 4) {
            ForEach(pr.labels) { label in
                Text(label.name)
                    .font(.system(size: 10))
                    .foregroundColor(.white)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(labelColor(label.color))
                    .clipShape(RoundedRectangle(cornerRadius: 3))
            }
        }
    }

    private func labelColor(_ hex: String) -> Color {
        let cleanHex = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        guard cleanHex.count == 6, let value = UInt64(cleanHex, radix: 16) else {
            return .gray
        }
        let r = Double((value >> 16) & 0xFF) / 255.0
        let g = Double((value >> 8) & 0xFF) / 255.0
        let b = Double(value & 0xFF) / 255.0
        return Color(red: r, green: g, blue: b)
    }
}
