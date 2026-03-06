import SwiftUI

// MARK: - Schedule Type

enum ScheduleType: String, CaseIterable, Identifiable {
    case swarm
    case agent
    case inlinePrompt

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .swarm: "person.3.fill"
        case .agent: "cpu"
        case .inlinePrompt: "text.bubble"
        }
    }

    var label: String {
        switch self {
        case .swarm: "Swarm"
        case .agent: "Agent"
        case .inlinePrompt: "Inline Prompt"
        }
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

    /// Backend string matching Rust recurrence field.
    var backendString: String {
        switch self {
        case .none: "none"
        case .hourly: "hourly"
        case .daily: "daily"
        case .weekdays: "weekdays"
        case .weekly: "weekly"
        case .monthly: "monthly"
        }
    }

    init(backendString: String) {
        switch backendString {
        case "hourly": self = .hourly
        case "daily": self = .daily
        case "weekdays": self = .weekdays
        case "weekly": self = .weekly
        case "monthly": self = .monthly
        default: self = .none
        }
    }

    func matches(date: Date, originalDate: Date, calendar: Calendar) -> Bool {
        guard date >= originalDate else { return false }

        switch self {
        case .none:
            return calendar.isDate(date, inSameDayAs: originalDate)
        case .hourly:
            return true
        case .daily:
            return timesMatch(date, originalDate, calendar)
        case .weekdays:
            let weekday = calendar.component(.weekday, from: date)
            return weekday >= 2 && weekday <= 6 && timesMatch(date, originalDate, calendar)
        case .weekly:
            return calendar.component(.weekday, from: date) == calendar.component(.weekday, from: originalDate)
                && timesMatch(date, originalDate, calendar)
        case .monthly:
            return calendar.component(.day, from: date) == calendar.component(.day, from: originalDate)
                && timesMatch(date, originalDate, calendar)
        }
    }

    private func timesMatch(_ a: Date, _ b: Date, _ calendar: Calendar) -> Bool {
        let t1 = calendar.dateComponents([.hour, .minute], from: a)
        let t2 = calendar.dateComponents([.hour, .minute], from: b)
        return t1.hour == t2.hour && t1.minute == t2.minute
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
    var enabled: Bool
    var scope: String
    var trigger: ScheduleTriggerPayload?

    init(
        id: UUID = UUID(),
        name: String,
        type: ScheduleType,
        date: Date,
        recurrence: RecurrenceRule = .none,
        projectName: String,
        target: String = "",
        enabled: Bool = true,
        scope: String = "local",
        trigger: ScheduleTriggerPayload? = nil
    ) {
        self.id = id
        self.name = name
        self.type = type
        self.date = date
        self.recurrence = recurrence
        self.projectName = projectName
        self.target = target
        self.color = EventColor.forProject(projectName)
        self.enabled = enabled
        self.scope = scope
        self.trigger = trigger
    }

    /// Initialize from a daemon ScheduleInfoPayload.
    init(from payload: ScheduleInfoPayload) {
        self.id = UUID()
        self.name = payload.name
        self.enabled = payload.enabled
        self.recurrence = RecurrenceRule(backendString: payload.recurrence)
        self.projectName = payload.projectRoot
        self.target = payload.target
        self.scope = payload.scope
        self.color = EventColor.forProject(payload.projectRoot)
        self.trigger = payload.trigger

        // Parse start_at ISO 8601 date (with or without fractional seconds)
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let parsed = formatter.date(from: payload.startAt) {
            self.date = parsed
        } else {
            formatter.formatOptions = [.withInternetDateTime]
            self.date = formatter.date(from: payload.startAt) ?? Date()
        }

        // Determine type from trigger
        switch payload.trigger {
        case .agentDef:
            self.type = .agent
        case .swarmDef:
            self.type = .swarm
        case .inlinePrompt:
            self.type = .inlinePrompt
        }
    }
}
