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

    init() {
        generateMockData()
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
            event.recurrence.matches(date: date, originalDate: event.date, calendar: calendar)
        }
    }

    /// Expand all events into concrete occurrences within a date range.
    func expandedOccurrences(from start: Date, to end: Date) -> [(date: Date, event: ScheduleEvent)] {
        var results: [(Date, ScheduleEvent)] = []

        for event in events {
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

    // MARK: - Mock Data

    private func generateMockData() {
        let today = Date()
        let cal = calendar

        // Doc review swarm — daily at 9am
        if let date = cal.date(bySettingHour: 9, minute: 0, second: 0, of: today) {
            events.append(ScheduleEvent(
                name: "Doc Review Swarm",
                type: .swarm,
                date: date,
                recurrence: .daily,
                projectName: "purepoint",
                target: "docs/"
            ))
        }

        // Feature generation — hourly
        if let date = cal.date(bySettingHour: 10, minute: 0, second: 0, of: today) {
            events.append(ScheduleEvent(
                name: "Feature Gen Agents",
                type: .agent,
                date: date,
                recurrence: .hourly,
                projectName: "acme-app",
                target: "src/features/"
            ))
        }

        // Pre-dinner batch — weekdays at 5:30pm
        if let date = cal.date(bySettingHour: 17, minute: 30, second: 0, of: today) {
            events.append(ScheduleEvent(
                name: "Pre-Dinner Batch",
                type: .swarm,
                date: date,
                recurrence: .weekdays,
                projectName: "purepoint",
                target: "crates/"
            ))
        }

        // Weekly review — weekly
        if let date = cal.date(bySettingHour: 14, minute: 0, second: 0, of: today) {
            events.append(ScheduleEvent(
                name: "Weekly Code Review",
                type: .swarm,
                date: date,
                recurrence: .weekly,
                projectName: "data-pipeline",
                target: "src/"
            ))
        }

        // Monthly report — monthly
        if let firstOfMonth = cal.date(from: cal.dateComponents([.year, .month], from: today)),
           let date = cal.date(bySettingHour: 10, minute: 0, second: 0, of: firstOfMonth) {
            events.append(ScheduleEvent(
                name: "Monthly Report Gen",
                type: .agent,
                date: date,
                recurrence: .monthly,
                projectName: "analytics",
                target: "reports/"
            ))
        }

        // Nightly lint — daily at 11pm
        if let date = cal.date(bySettingHour: 23, minute: 0, second: 0, of: today) {
            events.append(ScheduleEvent(
                name: "Nightly Lint",
                type: .agent,
                date: date,
                recurrence: .daily,
                projectName: "acme-app",
                target: "."
            ))
        }
    }
}
