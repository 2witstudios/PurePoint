import SwiftUI

struct ScheduleView: View {
    @Environment(AppState.self) private var appState

    private var state: ScheduleState {
        appState.scheduleState
    }

    private var projectRoots: [String] {
        appState.libraryProjectRoots
    }

    var body: some View {
        VStack(spacing: 0) {
            #if os(iOS)
            LibraryMachineBar()
            Divider()
            #endif
            ScheduleHeaderView(state: state)
            Divider()
            calendarContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .sheet(isPresented: Bindable(state).showingCreationSheet) {
            ScheduleEventSheet(state: state, editingEvent: nil)
                .presentedControlDefaults()
        }
        .sheet(isPresented: Bindable(state).showingEditSheet) {
            ScheduleEventSheet(state: state, editingEvent: state.selectedEvent)
                .presentedControlDefaults()
        }
        .task {
            await state.loadSchedules(projectRoots: projectRoots)
        }
        .onChange(of: appState.projects.count) { _, _ in
            Task {
                await state.loadSchedules(projectRoots: projectRoots)
            }
        }
        #if os(iOS)
        // The library follows the selected machine (IPAD-012); reload when it or its projects change.
        .onChange(of: appState.libraryProjectRoots) { _, _ in
            Task { await state.loadSchedules(projectRoots: projectRoots) }
        }
        #endif
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
