import SwiftUI

struct WeekCalendarView: View {
    @Bindable var state: ScheduleState

    private let calendar = Calendar.current

    private static let dayFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "EEE"
        return f
    }()

    var body: some View {
        VStack(spacing: 0) {
            dayHeaders
            Divider()
            TimeGridView(state: state, columns: 7, dates: state.weekDays)
        }
    }

    // MARK: - Day Column Headers

    private var dayHeaders: some View {
        HStack(spacing: 0) {
            // Gutter spacer
            Color.clear
                .frame(width: PurePointTheme.calendarTimeGutterWidth)

            ForEach(Array(state.weekDays.enumerated()), id: \.offset) { _, date in
                let isToday = calendar.isDateInToday(date)
                let dayNum = calendar.component(.day, from: date)
                let dayName = Self.dayFormatter.string(from: date)

                VStack(spacing: 2) {
                    Text(dayName.uppercased())
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(.secondary)

                    Text("\(dayNum)")
                        .font(.system(size: 14, weight: isToday ? .bold : .medium))
                        .foregroundStyle(isToday ? .white : .primary)
                        .frame(width: 26, height: 26)
                        .background(isToday ? Color.accentColor : .clear)
                        .clipShape(Circle())
                }
                .frame(maxWidth: .infinity)
                .frame(height: PurePointTheme.calendarHeaderHeight)
            }
        }
    }
}
