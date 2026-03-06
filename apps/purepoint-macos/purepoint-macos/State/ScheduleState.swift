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
            _ = try await client.send(.saveSchedule(
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
        let formatter = DateFormatter()
        formatter.dateFormat = "MMMM yyyy"
        return formatter.string(from: currentDate)
    }

    var currentWeekRange: String {
        let weekStart = calendar.dateInterval(of: .weekOfYear, for: currentDate)?.start ?? currentDate
        let weekEnd = calendar.date(byAdding: .day, value: 6, to: weekStart) ?? currentDate
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        return "\(formatter.string(from: weekStart)) – \(formatter.string(from: weekEnd))"
    }

    var currentDayFormatted: String {
        let formatter = DateFormatter()
        formatter.dateFormat = "EEEE, MMMM d"
        return formatter.string(from: currentDate)
    }

    var headerDateLabel: String {
        switch viewMode {
        case .month: currentMonthYear
        case .week: currentWeekRange
        case .day: currentDayFormatted
        case .list: currentMonthYear
        }
    }

    func goForward() {
        switch viewMode {
        case .month:
            currentDate = calendar.date(byAdding: .month, value: 1, to: currentDate) ?? currentDate
        case .week:
            currentDate = calendar.date(byAdding: .weekOfYear, value: 1, to: currentDate) ?? currentDate
        case .day:
            currentDate = calendar.date(byAdding: .day, value: 1, to: currentDate) ?? currentDate
        case .list:
            currentDate = calendar.date(byAdding: .month, value: 1, to: currentDate) ?? currentDate
        }
    }

    func goBackward() {
        switch viewMode {
        case .month:
            currentDate = calendar.date(byAdding: .month, value: -1, to: currentDate) ?? currentDate
        case .week:
            currentDate = calendar.date(byAdding: .weekOfYear, value: -1, to: currentDate) ?? currentDate
        case .day:
            currentDate = calendar.date(byAdding: .day, value: -1, to: currentDate) ?? currentDate
        case .list:
            currentDate = calendar.date(byAdding: .month, value: -1, to: currentDate) ?? currentDate
        }
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
            case .hourly:
                // Show one per day within range at the original time
                var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
                let endDay = calendar.startOfDay(for: end)
                let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
                while cursor <= endDay {
                    if let occurrence = calendar.date(bySettingHour: origComps.hour ?? 0,
                                                     minute: origComps.minute ?? 0,
                                                     second: 0, of: cursor),
                       occurrence >= start && occurrence <= end {
                        results.append((occurrence, event))
                    }
                    cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
                }
            case .daily:
                var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
                let endDay = calendar.startOfDay(for: end)
                let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
                while cursor <= endDay {
                    if let occurrence = calendar.date(bySettingHour: origComps.hour ?? 0,
                                                     minute: origComps.minute ?? 0,
                                                     second: 0, of: cursor),
                       occurrence >= start && occurrence <= end {
                        results.append((occurrence, event))
                    }
                    cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
                }
            case .weekdays:
                var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
                let endDay = calendar.startOfDay(for: end)
                let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
                while cursor <= endDay {
                    let weekday = calendar.component(.weekday, from: cursor)
                    if weekday >= 2 && weekday <= 6 {
                        if let occurrence = calendar.date(bySettingHour: origComps.hour ?? 0,
                                                         minute: origComps.minute ?? 0,
                                                         second: 0, of: cursor),
                           occurrence >= start && occurrence <= end {
                            results.append((occurrence, event))
                        }
                    }
                    cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
                }
            case .weekly:
                let origWeekday = calendar.component(.weekday, from: event.date)
                var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
                let endDay = calendar.startOfDay(for: end)
                let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
                while cursor <= endDay {
                    if calendar.component(.weekday, from: cursor) == origWeekday {
                        if let occurrence = calendar.date(bySettingHour: origComps.hour ?? 0,
                                                         minute: origComps.minute ?? 0,
                                                         second: 0, of: cursor),
                           occurrence >= start && occurrence <= end {
                            results.append((occurrence, event))
                        }
                    }
                    cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
                }
            case .monthly:
                let origDay = calendar.component(.day, from: event.date)
                var cursor = max(calendar.startOfDay(for: start), calendar.startOfDay(for: event.date))
                let endDay = calendar.startOfDay(for: end)
                let origComps = calendar.dateComponents([.hour, .minute], from: event.date)
                while cursor <= endDay {
                    if calendar.component(.day, from: cursor) == origDay {
                        if let occurrence = calendar.date(bySettingHour: origComps.hour ?? 0,
                                                         minute: origComps.minute ?? 0,
                                                         second: 0, of: cursor),
                           occurrence >= start && occurrence <= end {
                            results.append((occurrence, event))
                        }
                    }
                    cursor = calendar.date(byAdding: .day, value: 1, to: cursor) ?? end
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
}
