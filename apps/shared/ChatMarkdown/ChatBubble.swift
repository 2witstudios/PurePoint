import SwiftUI

/// Text-message styling shared by the phone and desktop Point Guard transcripts.
enum ChatBubbleSender { case user, assistant }

enum ChatBubbleStyle {
    static let assistantFill = Color.primary.opacity(0.07)
    static let userFill = Color.accentColor
    static let radius: CGFloat = 20
    static let tailRadius: CGFloat = 6

    /// The corner nearest the sender stays tight, like a message tail.
    static func shape(_ sender: ChatBubbleSender, radius: CGFloat = radius) -> UnevenRoundedRectangle {
        UnevenRoundedRectangle(
            topLeadingRadius: radius,
            bottomLeadingRadius: sender == .assistant ? tailRadius : radius,
            bottomTrailingRadius: sender == .user ? tailRadius : radius,
            topTrailingRadius: radius,
            style: .continuous)
    }
}

/// A sender-aligned bubble that hugs short messages and wraps long ones.
struct ChatBubble<Content: View>: View {
    let sender: ChatBubbleSender
    var compact = false
    @ViewBuilder let content: Content

    var body: some View {
        HStack(spacing: 0) {
            if sender == .user { Spacer(minLength: compact ? 28 : 48) }
            ChatBubbleHuggingLayout {
                content
                    .padding(.horizontal, compact ? 13 : 15).padding(.vertical, compact ? 8 : 10)
                    .foregroundStyle(sender == .user ? Color.white : Color.primary)
                    .tint(sender == .user ? Color.white : Color.accentColor)
                    .background(
                        sender == .user ? ChatBubbleStyle.userFill : ChatBubbleStyle.assistantFill,
                        in: ChatBubbleStyle.shape(sender))
            }
            if sender == .assistant { Spacer(minLength: compact ? 28 : 48) }
        }
    }
}

/// Proposes the content's ideal width, capped by the space offered, so a one-line
/// reply gets a snug bubble even though Markdown content fills any width it is given.
struct ChatBubbleHuggingLayout: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let content = subviews.first else { return .zero }
        let ideal = content.sizeThatFits(.unspecified)
        let width = min(ideal.width, proposal.width ?? ideal.width)
        return content.sizeThatFits(ProposedViewSize(width: width, height: nil))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: nil))
    }
}

/// Three dots rising in turn. Static when Reduce Motion is on.
struct TypingDots: View {
    var diameter: CGFloat = 7
    var spacing: CGFloat = 5
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let time = context.date.timeIntervalSinceReferenceDate
            HStack(spacing: spacing) {
                ForEach(0..<3, id: \.self) { index in
                    let lift = reduceMotion ? 0.3 : Self.lift(at: time, delay: Double(index) * 0.15)
                    Circle()
                        .fill(Color.secondary)
                        .frame(width: diameter, height: diameter)
                        .opacity(0.35 + 0.65 * lift)
                        .offset(y: -diameter * 0.45 * lift)
                }
            }
        }
        .accessibilityHidden(true)
    }

    /// One 1.2s cycle per dot: rise and fall over the first half, then rest.
    static func lift(at time: TimeInterval, delay: TimeInterval) -> Double {
        let period = 1.2
        let phase = (time - delay).truncatingRemainder(dividingBy: period)
        let progress = (phase < 0 ? phase + period : phase) / period
        return progress < 0.6 ? sin(progress / 0.6 * .pi) : 0
    }
}

/// Shown at the end of the transcript while Point Guard is working.
struct TypingIndicatorBubble: View {
    var compact = false

    var body: some View {
        HStack {
            TypingDots(diameter: compact ? 6 : 7, spacing: compact ? 4 : 5)
                .padding(.horizontal, compact ? 13 : 16).padding(.vertical, compact ? 11 : 14)
                .background(ChatBubbleStyle.assistantFill, in: ChatBubbleStyle.shape(.assistant, radius: 18))
            Spacer(minLength: 0)
        }
        .accessibilityElement()
        .accessibilityLabel("Point Guard is typing")
        .accessibilityAddTraits(.updatesFrequently)
    }
}
