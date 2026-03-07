import SwiftUI

// MARK: - View Mode

enum ScheduleViewMode: String, CaseIterable, Identifiable {
    case month, week, day, list

    var id: String { rawValue }

    var label: String {
        rawValue.capitalized
    }
}

// MARK: - Schedule State

@Observable
@MainActor
final class ScheduleState {
    var viewMode: ScheduleViewMode = .week
    var currentDate: Date = Date()
    var events: [ScheduleEvent] = []
    var showingCreationSheet = false
    var creationPrefillDate: Date?
    var selectedEvent: ScheduleEvent?

    private let calendar = Calendar.current
    @ObservationIgnored private let client: DaemonClient

    @ObservationIgnored private static let monthYearFormatter: DateFormatter = {
        let f = DateFormatter(); f.dateFormat = "MMMM yyyy"; return f
    }()
    @ObservationIgnored private static let shortDateFormatter: DateFormatter = {
        let f = DateFormatter(); f.dateFormat = "MMM d"; return f
    }()
    @ObservationIgnored private static let dayFormatter: DateFormatter = {
        let f = DateFormatter(); f.dateFormat = "EEEE, MMMM d"; return f
    }()

    init(client: DaemonClient = DaemonClient()) {
        self.client = client
    }

    // MARK: - Backend Integration

    func loadSchedules(projectRoot: String) async {
        do {
            let response = try await client.send(.listSchedules(projectRoot: projectRoot))
            if case .scheduleList(let schedules) = response {
                self.events = schedules.map { ScheduleEvent(from: $0) }
            }
        } catch {
            print("Failed to load schedules: \(error)")
        }
    }

    func saveSchedule(
        projectRoot: String,
        name: String,
        enabled: Bool,
        recurrence: RecurrenceRule,
        startAt: Date,
        trigger: ScheduleTriggerPayload,
        target: String,
        scope: String
    ) async {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        let startAtString = formatter.string(from: startAt)

        do {
            _ = try await client.send(
                .saveSchedule(
                    projectRoot: projectRoot,
                    name: name,
                    enabled: enabled,
                    recurrence: recurrence.backendString,
                    startAt: startAtString,
                    trigger: trigger,
                    target: target,
                    scope: scope
                ))
            await loadSchedules(projectRoot: projectRoot)
        } catch {
            print("Failed to save schedule: \(error)")
        }
    }

    func deleteSchedule(projectRoot: String, name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteSchedule(projectRoot: projectRoot, name: name, scope: scope))
            await loadSchedules(projectRoot: projectRoot)
        } catch {
            print("Failed to delete schedule: \(error)")
        }
    }

    func toggleSchedule(projectRoot: String, name: String, currentlyEnabled: Bool) async {
        do {
            if currentlyEnabled {
                _ = try await client.send(.disableSchedule(projectRoot: projectRoot, name: name))
            } else {
                _ = try await client.send(.enableSchedule(projectRoot: projectRoot, name: name))
            }
            await loadSchedules(projectRoot: projectRoot)
        } catch {
            print("Failed to toggle schedule: \(error)")
        }
    }

    // MARK: - Date Navigation

    var currentMonthYear: String {
        Self.monthYearFormatter.string(from: currentDate)
    }

    var currentWeekRange: String {
        let weekStart = calendar.dateInterval(of: .weekOfYear, for: currentDate)?.start ?? currentDate
        let weekEnd = calendar.date(byAdding: .day, value: 6, to: weekStart) ?? currentDate
        return "\(Self.shortDateFormatter.string(from: weekStart)) – \(Self.shortDateFormatter.string(from: weekEnd))"
    }

    var currentDayFormatted: String {
        Self.dayFormatter.string(from: currentDate)
    }

    var headerDateLabel: String {
        switch viewMode {
        case .month: currentMonthYear
        case .week: currentWeekRange
        case .day: currentDayFormatted
        case .list: currentMonthYear
        }
    }

    func goForward() { navigate(by: 1) }
    func goBackward() { navigate(by: -1) }

    private func navigate(by value: Int) {
        let component: Calendar.Component =
            switch viewMode {
            case .month, .list: .month
            case .week: .weekOfYear
            case .day: .day
            }
        currentDate = calendar.date(byAdding: component, value: value, to: currentDate) ?? currentDate
    }

    func goToToday() {
        currentDate = Date()
    }

    // MARK: - Event Queries

    func events(for date: Date) -> [ScheduleEvent] {
        events.filter { event in
            event.enabled && event.recurrence.matches(date: date, originalDate: event.date, calendar: calendar)
        }
    }

    /// Expand all events into concrete occurrences within a date range.
    func expandedOccurrences(from start: Date, to end: Date) -> [(date: Date, event: ScheduleEvent)] {
        var results: [(Date, ScheduleEvent)] = []

        for event in events where event.enabled {
            switch event.recurrence {
            case .none:
                if event.date >= start && event.date <= end {
                    results.append((event.date, event))
                }
            case .hourly, .daily:
                results += walkDays(from: start, to: end, event: event)
            case .weekdays:
                results += walkDays(from: start, to: end, event: event) { [calendar] day in
                    let weekday = calendar.component(.weekday, from: day)
                    return weekday >= 2 && weekday <= 6
                }
            case .weekly:
                let origWeekday = calendar.component(.weekday, from: event.date)
                results += walkDays(from: start, to: end, event: event) { [calendar] day in
                    calendar.component(.weekday, from: day) == origWeekday
                }
            case .monthly:
                let origDay = calendar.component(.day, from: event.date)
                results += walkDays(from: start, to: end, event: event) { [calendar] day in
                    calendar.component(.day, from: day) == origDay
                }
            }
        }

        return results.sorted { $0.0 < $1.0 }
    }

    // MARK: - Interaction

    func handleTimeSlotClick(date: Date) {
        creationPrefillDate = date
        showingCreationSheet = true
    }

    func addEvent(_ event: ScheduleEvent) {
        events.append(event)
    }

    // MARK: - Month Grid Helpers

    var monthDays: [Date] {
        guard let monthInterval = calendar.dateInterval(of: .month, for: currentDate) else { return [] }
        let firstOfMonth = monthInterval.start
        let firstWeekday = calendar.component(.weekday, from: firstOfMonth)
        let leadingBlanks = (firstWeekday - calendar.firstWeekday + 7) % 7

        guard let gridStart = calendar.date(byAdding: .day, value: -leadingBlanks, to: firstOfMonth) else { return [] }

        // 6 weeks = 42 cells
        return (0..<42).compactMap { calendar.date(byAdding: .day, value: $0, to: gridStart) }
    }

    func isCurrentMonth(_ date: Date) -> Bool {
        calendar.component(.month, from: date) == calendar.component(.month, from: currentDate)
    }

    func isToday(_ date: Date) -> Bool {
        calendar.isDateInToday(date)
    }

    // MARK: - Week Helpers

    var weekDays: [Date] {
        guard let weekInterval = calendar.dateInterval(of: .weekOfYear, for: currentDate) else { return [] }
        return (0..<7).compactMap { calendar.date(byAdding: .day, value: $0, to: weekInterval.start) }
    }

    // MARK: - Occurrence Helpers

    private func walkDays(
        from start: Date, to end: Date, event: ScheduleEvent,
        filter: ((Date) -> Bool)? = nil
    ) -> [(Date, ScheduleEvent)] {
        var results: [(Date, ScheduleEvent)] = []
        var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
        let endDay = calendar.startOfDay(for: end)
        let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
        while cursor <= endDay {
            if filter?(cursor) ?? true,
                let occurrence = calendar.date(
                    bySettingHour: origComps.hour ?? 0,
                    minute: origComps.minute ?? 0,
                    second: 0, of: cursor),
                occurrence >= start && occurrence <= end
            {
                results.append((occurrence, event))
            }
            cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
        }
        return results
    }
}
