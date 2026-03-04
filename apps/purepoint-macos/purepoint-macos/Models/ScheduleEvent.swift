import SwiftUI

// MARK: - Schedule Type

enum ScheduleType: String, CaseIterable, Identifiable {
    case swarm
    case agent

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .swarm: "person.3.fill"
        case .agent: "cpu"
        }
    }

    var label: String {
        rawValue.capitalized
    }
}

// MARK: - Recurrence Rule

enum RecurrenceRule: String, CaseIterable, Identifiable {
    case none = "Does not repeat"
    case hourly = "Every hour"
    case daily = "Every day"
    case weekdays = "Every weekday"
    case weekly = "Every week"
    case monthly = "Every month"

    var id: String { rawValue }

    var badge: String? {
        switch self {
        case .none: nil
        case .hourly: "Hourly"
        case .daily: "Daily"
        case .weekdays: "Weekdays"
        case .weekly: "Weekly"
        case .monthly: "Monthly"
        }
    }

    func matches(date: Date, originalDate: Date, calendar: Calendar) -> Bool {
        switch self {
        case .none:
            return calendar.isDate(date, inSameDayAs: originalDate)
        case .hourly:
            return date >= originalDate
        case .daily:
            let origTime = calendar.dateComponents([.hour, .minute], from: originalDate)
            let checkTime = calendar.dateComponents([.hour, .minute], from: date)
            return date >= originalDate
                && origTime.hour == checkTime.hour
                && origTime.minute == checkTime.minute
        case .weekdays:
            let weekday = calendar.component(.weekday, from: date)
            let isWeekday = weekday >= 2 && weekday <= 6
            let origTime = calendar.dateComponents([.hour, .minute], from: originalDate)
            let checkTime = calendar.dateComponents([.hour, .minute], from: date)
            return date >= originalDate && isWeekday
                && origTime.hour == checkTime.hour
                && origTime.minute == checkTime.minute
        case .weekly:
            let origWeekday = calendar.component(.weekday, from: originalDate)
            let checkWeekday = calendar.component(.weekday, from: date)
            let origTime = calendar.dateComponents([.hour, .minute], from: originalDate)
            let checkTime = calendar.dateComponents([.hour, .minute], from: date)
            return date >= originalDate && origWeekday == checkWeekday
                && origTime.hour == checkTime.hour
                && origTime.minute == checkTime.minute
        case .monthly:
            let origDay = calendar.component(.day, from: originalDate)
            let checkDay = calendar.component(.day, from: date)
            let origTime = calendar.dateComponents([.hour, .minute], from: originalDate)
            let checkTime = calendar.dateComponents([.hour, .minute], from: date)
            return date >= originalDate && origDay == checkDay
                && origTime.hour == checkTime.hour
                && origTime.minute == checkTime.minute
        }
    }
}

// MARK: - Event Color

enum EventColor {
    private static let palette: [Color] = [
        .blue, .purple, .teal, .indigo,
        .orange, .pink, .green, .yellow
    ]

    /// Stable djb2 hash — deterministic across app launches.
    private static func stableHash(_ string: String) -> UInt {
        var hash: UInt = 5381
        for byte in string.utf8 {
            hash = ((hash &<< 5) &+ hash) &+ UInt(byte)
        }
        return hash
    }

    static func forProject(_ name: String) -> Color {
        let hash = stableHash(name)
        return palette[Int(hash % UInt(palette.count))]
    }
}

// MARK: - Schedule Event

struct ScheduleEvent: Identifiable {
    let id: UUID
    var name: String
    var type: ScheduleType
    var date: Date
    var recurrence: RecurrenceRule
    var projectName: String
    var target: String
    var color: Color

    init(
        id: UUID = UUID(),
        name: String,
        type: ScheduleType,
        date: Date,
        recurrence: RecurrenceRule = .none,
        projectName: String,
        target: String = ""
    ) {
        self.id = id
        self.name = name
        self.type = type
        self.date = date
        self.recurrence = recurrence
        self.projectName = projectName
        self.target = target
        self.color = EventColor.forProject(projectName)
    }
}
