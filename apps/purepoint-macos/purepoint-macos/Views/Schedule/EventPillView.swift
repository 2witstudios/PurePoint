import SwiftUI

struct EventPillView: View {
    let event: ScheduleEvent
    let occurrence: Date

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: event.type.icon)
                .font(.system(size: 7))

            Text(event.name)
                .font(.system(size: 9, weight: .medium))
                .lineLimit(1)

            if let badge = event.recurrence.badge {
                Text(badge)
                    .font(.system(size: 7, weight: .semibold))
                    .padding(.horizontal, 3)
                    .padding(.vertical, 1)
                    .background(event.color.opacity(0.3))
                    .clipShape(RoundedRectangle(cornerRadius: 2))
            }
        }
        .foregroundStyle(event.color)
        .padding(.horizontal, 4)
        .frame(height: PurePointTheme.calendarPillHeight)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(event.color.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 3))
    }
}
