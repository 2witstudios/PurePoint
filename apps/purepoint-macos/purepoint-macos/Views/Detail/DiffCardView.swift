import SwiftUI

/// Card displaying a single file's diff — header with metadata + AppKit diff content body.
struct DiffCardView: View {
    let fileDiff: FileDiff

    var body: some View {
        VStack(spacing: 0) {
            header
            if !fileDiff.hunks.isEmpty {
                DiffContentRepresentable(hunks: fileDiff.hunks)
                    .frame(minHeight: 20)
            }
        }
        .background(Color(nsColor: Theme.cardBackground))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 6) {
            Image(systemName: statusIcon)
                .font(.system(size: 12))
                .foregroundColor(statusColor)

            filenameText
                .lineLimit(1)
                .truncationMode(.middle)

            statusBadge
                .layoutPriority(-1)

            Spacer(minLength: 4)

            statsText
        }
        .padding(.horizontal, 10)
        .frame(height: 32)
        .background(Color(nsColor: Theme.cardHeaderBackground))
    }

    private var filenameText: Text {
        let components = fileDiff.filename.split(separator: "/")
        if components.count > 1 {
            let dir = components.dropLast().joined(separator: "/") + "/"
            let file = String(components.last!)
            return Text("\(Text(dir).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary))\(Text(file).font(.system(size: 12, weight: .semibold, design: .monospaced)))")
        } else {
            return Text(fileDiff.filename).font(.system(size: 12, weight: .semibold, design: .monospaced))
        }
    }

    private var statusBadge: some View {
        Text(statusLabel)
            .font(.system(size: 10, weight: .medium))
            .foregroundColor(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(statusColor.opacity(0.6))
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    private var statsText: some View {
        HStack(spacing: 4) {
            if fileDiff.added > 0 {
                Text("+\(fileDiff.added)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.additionText))
            }
            if fileDiff.removed > 0 {
                Text("-\(fileDiff.removed)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(nsColor: Theme.deletionText))
            }
        }
    }

    // MARK: - Status Helpers

    private var statusIcon: String {
        switch fileDiff.statusCode {
        case "A": "doc.badge.plus"
        case "D": "doc.badge.minus"
        case "M": "doc.text"
        default:  "doc.text"
        }
    }

    private var statusColor: Color {
        switch fileDiff.statusCode {
        case "A": .green
        case "D": .red
        case "M": .yellow
        default:  .gray
        }
    }

    private var statusLabel: String {
        switch fileDiff.statusCode {
        case "M":  "Modified"
        case "A":  "Added"
        case "D":  "Deleted"
        case "??": "Untracked"
        default:   fileDiff.statusCode
        }
    }
}
