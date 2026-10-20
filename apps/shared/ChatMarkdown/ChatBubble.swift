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
    var assistantFill = ChatBubbleStyle.assistantFill
    /// Text on the user fill; must contrast with `ChatBubbleStyle.userFill` in both appearances.
    var userForeground = Color.white
    /// Minimum gap left beside an assistant bubble; `nil` uses the default inset.
    var assistantTrailingInset: CGFloat? = nil
    @ViewBuilder let content: Content

    var body: some View {
        HStack(spacing: 0) {
            if sender == .user { Spacer(minLength: compact ? 28 : 48) }
            ChatBubbleHuggingLayout {
                content
                    .padding(.horizontal, compact ? 13 : 15).padding(.vertical, compact ? 8 : 10)
                    .foregroundStyle(sender == .user ? userForeground : Color.primary)
                    .tint(sender == .user ? userForeground : Color.accentColor)
                    .background(
                        sender == .user ? ChatBubbleStyle.userFill : assistantFill,
                        in: ChatBubbleStyle.shape(sender))
            }
            if sender == .assistant { Spacer(minLength: assistantTrailingInset ?? (compact ? 28 : 48)) }
        }
    }
}

/// Proposes the content's ideal width, capped by the space offered, so a one-line
/// reply gets a snug bubble even though Markdown content fills any width it is given.
struct ChatBubbleHuggingLayout: Layout {
    /// The unconstrained measurement is the expensive one; SwiftUI rebuilds this cache
    /// whenever the content changes, so each layout pass measures the content only once.
    func makeCache(subviews: Subviews) -> CGFloat? {
        subviews.first?.sizeThatFits(.unspecified).width
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout CGFloat?) -> CGSize {
        guard let content = subviews.first, let ideal = cache else { return .zero }
        let width = min(ideal, proposal.width ?? ideal)
        return content.sizeThatFits(ProposedViewSize(width: width, height: nil))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout CGFloat?) {
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
    var fill = ChatBubbleStyle.assistantFill

    var body: some View {
        HStack {
            TypingDots(diameter: compact ? 6 : 7, spacing: compact ? 4 : 5)
                .padding(.horizontal, compact ? 13 : 16).padding(.vertical, compact ? 11 : 14)
                .background(fill, in: ChatBubbleStyle.shape(.assistant, radius: 18))
            Spacer(minLength: 0)
        }
        .accessibilityElement()
        .accessibilityLabel("Point Guard is typing")
        .accessibilityAddTraits(.updatesFrequently)
    }
}
