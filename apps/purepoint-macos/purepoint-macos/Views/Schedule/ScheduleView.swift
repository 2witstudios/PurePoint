import SwiftUI

struct ScheduleView: View {
    @Environment(AppState.self) private var appState

    private var state: ScheduleState {
        appState.scheduleState
    }

    private var projectRoot: String {
        appState.activeProjectRoot ?? appState.projects.first?.projectRoot ?? ""
    }

    var body: some View {
        VStack(spacing: 0) {
            ScheduleHeaderView(state: state)
            Divider()
            calendarContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .sheet(isPresented: Bindable(state).showingCreationSheet) {
            ScheduleCreationSheet(state: state, projectRoot: projectRoot)
        }
        .task {
            await state.loadSchedules(projectRoot: projectRoot)
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
