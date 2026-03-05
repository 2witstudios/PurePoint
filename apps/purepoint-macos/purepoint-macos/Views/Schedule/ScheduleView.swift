import SwiftUI

struct ScheduleView: View {
    @State private var state = ScheduleState()

    var body: some View {
        VStack(spacing: 0) {
            ScheduleHeaderView(state: state)
            Divider()
            calendarContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .sheet(isPresented: $state.showingCreationSheet) {
            ScheduleCreationSheet(state: state)
        }
    }

    @ViewBuilder
    private var calendarContent: some View {
        switch state.viewMode {
        case .month:
            MonthCalendarView(state: state)
        case .week:
            WeekCalendarView(state: state)
        case .day:
            DayCalendarView(state: state)
        case .list:
            ScheduleListView(state: state)
        }
    }
}
