#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac window-chrome hook. UIKit scenes have no title bar to extend content
/// under, so there is nothing to draw. It lives for as long as its window, whatever the sidebar
/// shows, so it hosts the window's Open Project picker, and it gives the window's scene the
/// Mac window's minimum size.
struct WindowChromeConfigurator: View {
    var body: some View {
        SceneMinimumSize()
            .remoteProjectPicker()
    }
}

/// The Mac window's minimum size for the iPad window that shows this view. The Mac app's
/// window cannot shrink below 700×400 (`PurePointTheme.windowMinWidth`), so its layouts never
/// meet a narrower window; iPadOS lets a window shrink until the sidebar and panes overflow it.
/// The width is narrowed from the Mac's to a half-screen split, so the iPad still offers
/// side-by-side halves.
private struct SceneMinimumSize: UIViewRepresentable {
    static let size = CGSize(width: PurePointTheme.iPadWindowMinWidth, height: PurePointTheme.windowMinHeight)

    func makeUIView(context: Context) -> SceneSizeView { SceneSizeView() }
    func updateUIView(_ view: SceneSizeView, context: Context) {}

    final class SceneSizeView: UIView {
        override init(frame: CGRect) {
            super.init(frame: frame)
            isUserInteractionEnabled = false
            backgroundColor = .clear
        }

        required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            window?.windowScene?.sizeRestrictions?.minimumSize = SceneMinimumSize.size
        }
    }
}

/// Where a window's iPadOS window controls end, from the corner-adapted safe areas: the
/// vertically adapted top inset reaches below the controls, the horizontally adapted leading
/// inset to their right. The plain safe area covers only the status bar.
struct WindowControlsInsets: Equatable {
    var bottom: CGFloat = 0
    var leading: CGFloat = 0
}

/// Reports the window's `WindowControlsInsets` as its window moves and resizes. It must fill
/// the window (behind content that ignores the safe area) so the insets are the window's.
struct WindowControlsProbe: UIViewRepresentable {
    let onChange: (WindowControlsInsets) -> Void

    func makeUIView(context: Context) -> ProbeView {
        let view = ProbeView()
        view.onChange = onChange
        return view
    }

    func updateUIView(_ view: ProbeView, context: Context) {
        view.onChange = onChange
    }

    final class ProbeView: UIView {
        var onChange: ((WindowControlsInsets) -> Void)?
        private var last: WindowControlsInsets?

        override init(frame: CGRect) {
            super.init(frame: frame)
            isUserInteractionEnabled = false
            backgroundColor = .clear
        }

        required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

        override func layoutSubviews() {
            super.layoutSubviews()
            report()
        }

        override func safeAreaInsetsDidChange() {
            super.safeAreaInsetsDidChange()
            report()
        }

        private func report() {
            let plain = safeAreaInsets
            let vertical = edgeInsets(for: .safeArea(cornerAdaptation: .vertical))
            let horizontal = directionalEdgeInsets(for: .safeArea(cornerAdaptation: .horizontal))
            // Insets beyond the plain safe area come from the controls; none means no controls.
            let insets = WindowControlsInsets(
                bottom: vertical.top > plain.top ? vertical.top : 0,
                leading: horizontal.leading > plain.left ? horizontal.leading : 0)
            guard insets != last else { return }
            last = insets
            DispatchQueue.main.async { [onChange] in onChange?(insets) }
        }
    }
}
#endif
