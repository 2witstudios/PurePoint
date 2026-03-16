import SwiftUI

struct ScheduleView: View {
    @Environment(AppState.self) private var appState

    private var state: ScheduleState {
        appState.scheduleState
    }

    private var projectRoots: [String] {
        appState.projects.map(\.projectRoot)
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
            ScheduleEventSheet(state: state, editingEvent: nil)
        }
        .sheet(isPresented: Bindable(state).showingEditSheet) {
            ScheduleEventSheet(state: state, editingEvent: state.selectedEvent)
        }
        .task {
            await state.loadSchedules(projectRoots: projectRoots)
        }
        .onChange(of: appState.projects.count) { _, _ in
            Task {
                await state.loadSchedules(projectRoots: projectRoots)
            }
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
