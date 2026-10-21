#if os(iOS)
import SwiftUI
import UIKit

// The Mac's control looks under the style names shared views already use, so a toggle, button
// or text field draws as it does on the Mac rather than as its iOS namesake (a 51pt green
// switch for a checkbox, blue text for a push button, a 34pt field). Two mechanisms:
//
// - Styles a Mac view gets without asking — a checkbox for a plain `Toggle`, a push button for
//   a plain `Button`, the Mac's group box — are set once as the app root's defaults (`macControlDefaults()`), which
//   any explicit style below still overrides, as on the Mac.
// - Explicit styles (`.switch`, `.bordered`, `.borderedProminent`, `.borderless`,
//   `.roundedBorder`) are routed by the concrete-type overloads at the bottom, which Swift
//   prefers over SwiftUI's generic modifiers, so call sites don't change.

/// The Mac's control metrics per `ControlSize`: height, font size and corner radius.
private struct MacControlMetrics {
    let height: CGFloat
    let fontSize: CGFloat
    let cornerRadius: CGFloat
    let horizontalPadding: CGFloat

    init(_ size: ControlSize) {
        switch size {
        case .mini: (height, fontSize, cornerRadius, horizontalPadding) = (16, 9, 4, 6)
        case .small: (height, fontSize, cornerRadius, horizontalPadding) = (20, 11, 5, 8)
        case .large, .extraLarge: (height, fontSize, cornerRadius, horizontalPadding) = (30, 13, 8, 14)
        default: (height, fontSize, cornerRadius, horizontalPadding) = (24, 13, 6, 11)
        }
    }
}

// MARK: - Toggles

/// The Mac's default toggle: a checkbox in front of its label.
struct MacCheckboxToggleStyle: ToggleStyle {
    @Environment(\.controlSize) private var controlSize
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.colorScheme) private var colorScheme

    func makeBody(configuration: Configuration) -> some View {
        let side: CGFloat = controlSize == .mini ? 10 : controlSize == .small ? 12 : 14
        Button {
            configuration.isOn.toggle()
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                ZStack {
                    RoundedRectangle(cornerRadius: side * 0.25, style: .continuous)
                        .fill(configuration.isOn ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(boxFill))
                    RoundedRectangle(cornerRadius: side * 0.25, style: .continuous)
                        .strokeBorder(Color.primary.opacity(configuration.isOn ? 0 : 0.25), lineWidth: 0.5)
                    if configuration.isOn {
                        Image(systemName: "checkmark")
                            .font(.system(size: side * 0.65, weight: .bold))
                            .foregroundStyle(.white)
                    }
                }
                .frame(width: side, height: side)
                .alignmentGuide(.firstTextBaseline) { $0[.bottom] - side * 0.2 }
                configuration.label
                    .font(.system(size: MacControlMetrics(controlSize).fontSize))
                    .foregroundStyle(.primary)
            }
            .contentShape(Rectangle())
            .opacity(isEnabled ? 1 : 0.45)
        }
        .buttonStyle(.plain)
        .accessibilityValue(configuration.isOn ? "On" : "Off")
    }

    private var boxFill: Color {
        colorScheme == .dark ? Color.white.opacity(0.12) : Color.white
    }
}

/// The Mac's switch: the label, then a small accent-colored switch at the trailing edge.
struct MacSwitchToggleStyle: ToggleStyle {
    @Environment(\.controlSize) private var controlSize
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let size: CGSize =
            switch controlSize {
            case .mini: CGSize(width: 26, height: 15)
            case .small: CGSize(width: 32, height: 18)
            default: CGSize(width: 38, height: 22)
            }
        HStack(spacing: 8) {
            configuration.label
            Spacer(minLength: 0)
            Capsule()
                .fill(configuration.isOn ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(Color.primary.opacity(0.16)))
                .overlay(alignment: configuration.isOn ? .trailing : .leading) {
                    Circle()
                        .fill(.white)
                        .shadow(color: .black.opacity(0.25), radius: 1, y: 0.5)
                        .padding(1.5)
                }
                .frame(width: size.width, height: size.height)
                .animation(.easeInOut(duration: 0.15), value: configuration.isOn)
                .onTapGesture { configuration.isOn.toggle() }
                .opacity(isEnabled ? 1 : 0.45)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityValue(configuration.isOn ? "On" : "Off")
        .accessibilityAction { configuration.isOn.toggle() }
    }
}

// MARK: - Buttons

/// The Mac's push button (`.automatic` and `.bordered`): a rounded rect at the control
/// height, label-colored text.
struct MacPushButtonStyle: ButtonStyle {
    @Environment(\.controlSize) private var controlSize
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.colorScheme) private var colorScheme

    func makeBody(configuration: Configuration) -> some View {
        let metrics = MacControlMetrics(controlSize)
        configuration.label
            .font(.system(size: metrics.fontSize))
            .lineLimit(1)
            .foregroundStyle(.primary)
            .padding(.horizontal, metrics.horizontalPadding)
            .frame(minHeight: metrics.height)
            .background {
                RoundedRectangle(cornerRadius: metrics.cornerRadius, style: .continuous)
                    .fill(fill(pressed: configuration.isPressed))
                    .shadow(color: .black.opacity(colorScheme == .dark ? 0 : 0.12), radius: 0.5, y: 0.5)
            }
            .overlay {
                RoundedRectangle(cornerRadius: metrics.cornerRadius, style: .continuous)
                    .strokeBorder(Color.primary.opacity(colorScheme == .dark ? 0.08 : 0.12), lineWidth: 0.5)
            }
            .contentShape(Rectangle())
            .opacity(isEnabled ? 1 : 0.45)
    }

    private func fill(pressed: Bool) -> Color {
        if colorScheme == .dark { return Color.white.opacity(pressed ? 0.32 : 0.2) }
        return pressed ? Color(white: 0.88) : Color.white
    }
}

/// The Mac's default (prominent) button: accent fill, white text.
struct MacProminentButtonStyle: ButtonStyle {
    @Environment(\.controlSize) private var controlSize
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let metrics = MacControlMetrics(controlSize)
        configuration.label
            .font(.system(size: metrics.fontSize))
            .lineLimit(1)
            .foregroundStyle(isEnabled ? .white : .secondary)
            .padding(.horizontal, metrics.horizontalPadding)
            .frame(minHeight: metrics.height)
            .background {
                RoundedRectangle(cornerRadius: metrics.cornerRadius, style: .continuous)
                    .fill(isEnabled ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(Color.primary.opacity(0.1)))
                    .brightness(configuration.isPressed ? -0.1 : 0)
            }
            .contentShape(Rectangle())
    }
}

/// The Mac's borderless button: the label as is, in the label color rather than iOS's tint,
/// dimmed while pressed.
struct MacBorderlessButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .contentShape(Rectangle())
            .opacity(isEnabled ? (configuration.isPressed ? 0.5 : 1) : 0.45)
    }
}

// MARK: - Text fields

/// The Mac's rounded-border field: a 22pt field with a thin border and the text background.
struct MacRoundedBorderTextFieldStyle: TextFieldStyle {
    @Environment(\.controlSize) private var controlSize
    @Environment(\.colorScheme) private var colorScheme

    // swiftlint:disable:next identifier_name
    func _body(configuration: TextField<Self._Label>) -> some View {
        let metrics = MacControlMetrics(controlSize)
        configuration
            .font(.system(size: metrics.fontSize))
            .padding(.horizontal, 6)
            .frame(minHeight: metrics.height - 2)
            .background {
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(colorScheme == .dark ? Color.white.opacity(0.06) : Color.white)
            }
            .overlay {
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.18), lineWidth: 0.5)
            }
    }
}

// MARK: - Group boxes

/// The Mac's default group box: its title in the label font above a lightly filled, bordered
/// box. iOS's own is a bold card that lays its content out at the ideal height, truncating
/// text the Mac wraps.
struct MacGroupBoxStyle: GroupBoxStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            configuration.label
                .font(.system(size: 11))
                .foregroundStyle(.primary)
                .padding(.leading, 4)
            VStack(alignment: .leading, spacing: 8) {
                configuration.content
            }
            .fixedSize(horizontal: false, vertical: true)
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.08), lineWidth: 0.5)
            }
        }
    }
}

// MARK: - Wiring

extension View {
    /// The Mac's defaults for controls that name no style, set once at the app root.
    func macControlDefaults() -> some View {
        toggleStyle(MacCheckboxToggleStyle())
            .buttonStyle(MacPushButtonStyle())
            .groupBoxStyle(MacGroupBoxStyle())
    }

    // Concrete-type overloads: Swift picks these over SwiftUI's generic modifiers.

    func toggleStyle(_ style: SwitchToggleStyle) -> some View {
        toggleStyle(MacSwitchToggleStyle())
    }

    func buttonStyle(_ style: BorderedButtonStyle) -> some View {
        buttonStyle(MacPushButtonStyle())
    }

    func buttonStyle(_ style: BorderedProminentButtonStyle) -> some View {
        buttonStyle(MacProminentButtonStyle())
    }

    func buttonStyle(_ style: BorderlessButtonStyle) -> some View {
        buttonStyle(MacBorderlessButtonStyle())
    }

    func textFieldStyle(_ style: RoundedBorderTextFieldStyle) -> some View {
        textFieldStyle(MacRoundedBorderTextFieldStyle())
    }
}

/// UIKit appearance the Mac controls need that SwiftUI cannot set, installed once at launch:
/// - the segmented control's 13pt titles (iOS uses 15pt); its height stays UIKit's, since
///   UISegmentedControl ignores a SwiftUI frame;
/// - the slider's Mac-sized 20pt knob (iOS draws a 28pt one over a taller track).
@MainActor
enum MacControlAppearance {
    static func install() {
        let segmented = UISegmentedControl.appearance()
        segmented.setTitleTextAttributes([.font: UIFont.systemFont(ofSize: 13)], for: .normal)
        segmented.setTitleTextAttributes([.font: UIFont.systemFont(ofSize: 13, weight: .medium)], for: .selected)

        let knob = sliderKnob()
        UISlider.appearance().setThumbImage(knob, for: .normal)
        UISlider.appearance().setThumbImage(knob, for: .highlighted)
    }

    /// A white 20pt knob with the Mac's hairline edge and soft shadow, drawn with room for it.
    private static func sliderKnob() -> UIImage {
        let side: CGFloat = 20, pad: CGFloat = 3
        let size = CGSize(width: side + pad * 2, height: side + pad * 2)
        return UIGraphicsImageRenderer(size: size).image { context in
            let rect = CGRect(x: pad, y: pad, width: side, height: side)
            let cg = context.cgContext
            cg.setShadow(offset: CGSize(width: 0, height: 0.5), blur: 2, color: UIColor.black.withAlphaComponent(0.3).cgColor)
            UIColor.white.setFill()
            cg.fillEllipse(in: rect)
            cg.setShadow(offset: .zero, blur: 0, color: nil)
            UIColor.black.withAlphaComponent(0.12).setStroke()
            cg.setLineWidth(0.5)
            cg.strokeEllipse(in: rect.insetBy(dx: 0.25, dy: 0.25))
        }
    }
}
#endif
