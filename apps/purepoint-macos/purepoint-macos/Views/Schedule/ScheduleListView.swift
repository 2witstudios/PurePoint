import SwiftUI

struct ScheduleListView: View {
    @Bindable var state: ScheduleState

    private let calendar = Calendar.current

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = .autoupdatingCurrent
        f.dateFormat = "h:mm a"
        return f
    }()

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(groupedOccurrences, id: \.date) { group in
                    sectionHeader(group.label)

                    ForEach(Array(group.items.enumerated()), id: \.offset) { _, occurrence in
                        eventRow(occurrence)
                        Divider()
                            .padding(.leading, 80)
                    }
                }
            }
            .padding(.bottom, 16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - Section Header

    private func sectionHeader(_ label: String) -> some View {
        Text(label)
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 16)
            .padding(.top, 16)
            .padding(.bottom, 6)
    }

    // MARK: - Event Row

    private func eventRow(_ occurrence: (date: Date, event: ScheduleEvent)) -> some View {
        HStack(spacing: 10) {
            // Time
            Text(Self.timeFormatter.string(from: occurrence.date))
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 64, alignment: .trailing)

            // Color bar
            RoundedRectangle(cornerRadius: 1.5)
                .fill(occurrence.event.color)
                .frame(width: 3, height: 32)

            // Details
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Image(systemName: occurrence.event.type.icon)
                        .font(.system(size: 10))
                        .foregroundStyle(occurrence.event.color)

                    Text(occurrence.event.name)
                        .font(.system(size: 13, weight: .medium))

                    if let badge = occurrence.event.recurrence.badge {
                        Text(badge)
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(occurrence.event.color)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(occurrence.event.color.opacity(0.15))
                            .clipShape(RoundedRectangle(cornerRadius: 3))
                    }
                }

                Text(
                    occurrence.event.target.isEmpty
                        ? occurrence.event.projectName
                        : "\(occurrence.event.projectName) · \(occurrence.event.target)"
                )
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
            }

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }

    // MARK: - Grouped Data

    private struct DayGroup {
        let date: Date
        let label: String
        let items: [(date: Date, event: ScheduleEvent)]
    }

    private var groupedOccurrences: [DayGroup] {
        let start = calendar.startOfDay(for: state.currentDate)
        guard let end = calendar.date(byAdding: .day, value: 30, to: start) else { return [] }

        let all = state.expandedOccurrences(from: start, to: end)

        var grouped: [Date: [(date: Date, event: ScheduleEvent)]] = [:]
        for occ in all {
            let dayKey = calendar.startOfDay(for: occ.date)
            grouped[dayKey, default: []].append(occ)
        }

        return grouped.keys.sorted().map { dayKey in
            DayGroup(
                date: dayKey,
                label: dayLabel(dayKey),
                items: grouped[dayKey] ?? []
            )
        }
    }

    private static let dayLabelFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = .autoupdatingCurrent
        f.dateFormat = "EEEE, MMMM d"
        return f
    }()

    private func dayLabel(_ date: Date) -> String {
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInTomorrow(date) { return "Tomorrow" }
        return Self.dayLabelFormatter.string(from: date)
    }
}
