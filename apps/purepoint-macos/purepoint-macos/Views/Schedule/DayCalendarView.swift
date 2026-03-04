import SwiftUI

struct DayCalendarView: View {
    @Bindable var state: ScheduleState

    var body: some View {
        VStack(spacing: 0) {
            dayHeader
            Divider()
            TimeGridView(state: state, columns: 1, dates: [state.currentDate])
        }
    }

    // MARK: - Day Header

    private var dayHeader: some View {
        HStack {
            Text(state.currentDayFormatted)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Calendar.current.isDateInToday(state.currentDate) ? Color.accentColor : .primary)
            Spacer()
        }
        .padding(.horizontal, 16)
        .frame(height: PurePointTheme.calendarHeaderHeight)
    }
}
