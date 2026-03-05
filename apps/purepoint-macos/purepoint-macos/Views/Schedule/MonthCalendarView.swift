import SwiftUI

struct MonthCalendarView: View {
    @Bindable var state: ScheduleState

    private let columns = Array(repeating: GridItem(.flexible(), spacing: 1), count: 7)
    private let weekdaySymbols = Calendar.current.shortWeekdaySymbols
    private let calendar = Calendar.current

    var body: some View {
        VStack(spacing: 0) {
            weekdayHeader
            Divider()
            monthGrid
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    // MARK: - Weekday Header

    private var weekdayHeader: some View {
        LazyVGrid(columns: columns, spacing: 0) {
            ForEach(weekdaySymbols, id: \.self) { symbol in
                Text(symbol)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(.secondary)
                    .frame(height: 24)
            }
        }
        .padding(.horizontal, 4)
    }

    // MARK: - Month Grid

    private var monthGrid: some View {
        LazyVGrid(columns: columns, spacing: 1) {
            ForEach(state.monthDays, id: \.self) { date in
                monthCell(for: date)
            }
        }
        .padding(.horizontal, 4)
        .padding(.bottom, 4)
    }

    // MARK: - Day Cell

    private func monthCell(for date: Date) -> some View {
        let isCurrentMonth = state.isCurrentMonth(date)
        let isToday = state.isToday(date)
        let dayEvents = eventsForDay(date)
        let maxVisible = 3

        return VStack(alignment: .leading, spacing: 2) {
            // Day number
            HStack {
                Text("\(calendar.component(.day, from: date))")
                    .font(.system(size: 11, weight: isToday ? .bold : .regular))
                    .foregroundColor(isToday ? .white : isCurrentMonth ? .primary : .secondary.opacity(0.5))
                    .frame(width: 20, height: 20)
                    .background(isToday ? Color.accentColor : .clear)
                    .clipShape(Circle())
                Spacer()
            }

            // Event pills
            ForEach(Array(dayEvents.prefix(maxVisible).enumerated()), id: \.offset) { _, occurrence in
                EventPillView(event: occurrence.event, occurrence: occurrence.date)
            }

            if dayEvents.count > maxVisible {
                Text("+\(dayEvents.count - maxVisible) more")
                    .font(.system(size: 8, weight: .medium))
                    .foregroundStyle(.secondary)
                    .padding(.leading, 4)
            }

            Spacer(minLength: 0)
        }
        .padding(3)
        .frame(minHeight: 80)
        .background(
            isToday
                ? Color(nsColor: Theme.calendarTodayBackground)
                : Color.clear
        )
        .contentShape(Rectangle())
        .onTapGesture {
            let targetDate = calendar.date(bySettingHour: 9, minute: 0, second: 0, of: date) ?? date
            state.handleTimeSlotClick(date: targetDate)
        }
    }

    private func eventsForDay(_ date: Date) -> [(date: Date, event: ScheduleEvent)] {
        let dayStart = calendar.startOfDay(for: date)
        guard let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) else { return [] }
        return state.expandedOccurrences(from: dayStart, to: dayEnd)
    }
}
