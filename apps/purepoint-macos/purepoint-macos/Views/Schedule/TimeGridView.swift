import SwiftUI

struct TimeGridView: View {
    @Bindable var state: ScheduleState
    let columns: Int
    let dates: [Date]

    private let calendar = Calendar.current
    private let hourHeight = PurePointTheme.calendarHourHeight
    private let gutterWidth = PurePointTheme.calendarTimeGutterWidth
    private let totalHours = 24

    private static let hourFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "h a"
        return f
    }()

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                ZStack(alignment: .topLeading) {
                    // Hour grid lines + labels
                    gridLines

                    // Event blocks per column
                    ForEach(Array(dates.enumerated()), id: \.offset) { colIndex, date in
                        columnEvents(date: date, columnIndex: colIndex)
                    }

                    // Current time indicator
                    currentTimeIndicator
                }
                .frame(height: CGFloat(totalHours) * hourHeight)
                .id("timeGrid")
            }
            .onAppear {
                // Scroll to current hour area
                proxy.scrollTo("timeGrid", anchor: UnitPoint(x: 0, y: currentTimeAnchor))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - Grid Lines

    private var gridLines: some View {
        GeometryReader { geo in
            let columnWidth = (geo.size.width - gutterWidth) / CGFloat(max(columns, 1))

            ForEach(0..<totalHours, id: \.self) { hour in
                let y = CGFloat(hour) * hourHeight

                // Hour label
                Text(hourLabel(hour))
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(.tertiary)
                    .frame(width: gutterWidth - 8, alignment: .trailing)
                    .position(x: (gutterWidth - 8) / 2, y: y + 6)

                // Grid line
                Path { path in
                    path.move(to: CGPoint(x: gutterWidth, y: y))
                    path.addLine(to: CGPoint(x: geo.size.width, y: y))
                }
                .stroke(Color(nsColor: Theme.calendarGridLine), lineWidth: hour % 6 == 0 ? 0.8 : 0.3)
            }

            // Column dividers
            if columns > 1 {
                ForEach(1..<columns, id: \.self) { col in
                    let x = gutterWidth + CGFloat(col) * columnWidth
                    Path { path in
                        path.move(to: CGPoint(x: x, y: 0))
                        path.addLine(to: CGPoint(x: x, y: geo.size.height))
                    }
                    .stroke(Color(nsColor: Theme.calendarGridLine), lineWidth: 0.3)
                }
            }
        }
    }

    // MARK: - Column Events

    private func columnEvents(date: Date, columnIndex: Int) -> some View {
        GeometryReader { geo in
            let columnWidth = (geo.size.width - gutterWidth) / CGFloat(max(columns, 1))
            let dayStart = calendar.startOfDay(for: date)
            let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart
            let occurrences = state.expandedOccurrences(from: dayStart, to: dayEnd)

            ForEach(Array(occurrences.enumerated()), id: \.offset) { _, occurrence in
                let comps = calendar.dateComponents([.hour, .minute], from: occurrence.date)
                let hourFrac = CGFloat(comps.hour ?? 0) + CGFloat(comps.minute ?? 0) / 60.0
                let y = hourFrac * hourHeight
                let blockHeight = max(hourHeight * 0.75, 30)

                EventBlockView(event: occurrence.event, occurrence: occurrence.date, height: blockHeight)
                    .frame(width: columnWidth - 8)
                    .position(
                        x: gutterWidth + CGFloat(columnIndex) * columnWidth + columnWidth / 2,
                        y: y + blockHeight / 2
                    )
            }

            // Click-to-create overlay
            Color.clear
                .contentShape(Rectangle())
                .frame(width: columnWidth, height: geo.size.height)
                .position(
                    x: gutterWidth + CGFloat(columnIndex) * columnWidth + columnWidth / 2,
                    y: geo.size.height / 2
                )
                .onTapGesture { location in
                    let hourFloat = location.y / hourHeight
                    let hour = Int(hourFloat)
                    let minute = Int((hourFloat - CGFloat(hour)) * 60)
                    let roundedMinute = (minute / 15) * 15

                    var comps = calendar.dateComponents([.year, .month, .day], from: date)
                    comps.hour = min(hour, 23)
                    comps.minute = roundedMinute
                    if let clickDate = calendar.date(from: comps) {
                        state.handleTimeSlotClick(date: clickDate)
                    }
                }
                .allowsHitTesting(true)
        }
    }

    // MARK: - Current Time Indicator

    private var currentTimeIndicator: some View {
        TimelineView(.periodic(from: .now, by: 60)) { context in
            GeometryReader { geo in
                let now = context.date
                let comps = calendar.dateComponents([.hour, .minute], from: now)
                let hourFrac = CGFloat(comps.hour ?? 0) + CGFloat(comps.minute ?? 0) / 60.0
                let y = hourFrac * hourHeight

                // Only show if today is in the visible dates
                if dates.contains(where: { calendar.isDateInToday($0) }) {
                    HStack(spacing: 0) {
                        Circle()
                            .fill(.red)
                            .frame(width: 8, height: 8)
                        Rectangle()
                            .fill(.red)
                            .frame(height: 1)
                    }
                    .frame(width: geo.size.width - gutterWidth + 4)
                    .position(x: gutterWidth + (geo.size.width - gutterWidth) / 2, y: y)
                }
            }
        }
    }

    // MARK: - Helpers

    private func hourLabel(_ hour: Int) -> String {
        let period = hour < 12 ? "AM" : "PM"
        let displayHour = hour == 0 ? 12 : (hour > 12 ? hour - 12 : hour)
        return "\(displayHour) \(period)"
    }

    private var currentTimeAnchor: CGFloat {
        let comps = calendar.dateComponents([.hour], from: Date())
        let hour = CGFloat(comps.hour ?? 8)
        // Position so current time is near top third
        return max(0, min(1, (hour - 2) / CGFloat(totalHours)))
    }
}
