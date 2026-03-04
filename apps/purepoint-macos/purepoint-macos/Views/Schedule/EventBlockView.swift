import SwiftUI

struct EventBlockView: View {
    let event: ScheduleEvent
    let occurrence: Date
    let height: CGFloat

    @State private var isHovered = false

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "h:mm a"
        return f
    }()

    var body: some View {
        HStack(spacing: 0) {
            // Left accent bar
            RoundedRectangle(cornerRadius: 1.5)
                .fill(event.color)
                .frame(width: 3)

            VStack(alignment: .leading, spacing: 2) {
                Text(event.name)
                    .font(.system(size: 11, weight: .semibold))
                    .lineLimit(1)

                if height > 36 {
                    HStack(spacing: 4) {
                        Image(systemName: event.type.icon)
                            .font(.system(size: 9))
                        Text(Self.timeFormatter.string(from: occurrence))
                            .font(.system(size: 10, design: .monospaced))
                    }
                    .foregroundStyle(.secondary)
                }

                if height > 52 {
                    Text(event.target.isEmpty ? event.projectName : "\(event.projectName) · \(event.target)")
                        .font(.system(size: 9))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
            .padding(.leading, 4)
            .padding(.vertical, 2)

            Spacer(minLength: 0)

            if let badge = event.recurrence.badge, height > 36 {
                Text(badge)
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(event.color)
                    .padding(.horizontal, 4)
                    .padding(.vertical, 2)
                    .background(event.color.opacity(0.2))
                    .clipShape(RoundedRectangle(cornerRadius: 3))
                    .padding(.trailing, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .frame(height: height)
        .background(event.color.opacity(isHovered ? 0.2 : 0.12))
        .clipShape(RoundedRectangle(cornerRadius: 4))
        .onHover { isHovered = $0 }
    }
}
