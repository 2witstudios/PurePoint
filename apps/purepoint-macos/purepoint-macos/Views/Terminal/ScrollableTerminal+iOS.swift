#if os(iOS)
import SwiftTerm
import UIKit

/// Placeholder twin of the Mac terminal host, with the same properties and size calls. It
/// holds SwiftTerm's iOS `TerminalView` but renders only a label until the interactive
/// terminal lands (IPAD-6.1, IPAD-6.15).
class ScrollableTerminal: UIView {
    let terminalView: TerminalView
    var attachSession: DaemonAttachSession?
    /// Remote TUIs must not handle browser/clipboard gestures on their host.
    var usesRemoteHost = false
    var openLink: (URL) -> Void = { UIApplication.shared.open($0) }
    var acceptsFileDrops = true
    private(set) var localGridSize = (cols: 120, rows: 40)
    private var tornDown = false

    init(frame: CGRect, terminalView: TerminalView? = nil) {
        self.terminalView = terminalView ?? TerminalView(frame: frame)
        super.init(frame: frame)
        backgroundColor = TerminalTheme.background
        NotYetOnIPad.addLabel("ScrollableTerminal", to: self)
    }

    required init?(coder: NSCoder) { fatalError() }

    func refreshLocalGeometry(size: CGSize) {}

    func tearDown() {
        guard !tornDown else { return }
        tornDown = true
        let session = attachSession
        Task { await session?.stop() }
    }

    func applyReportedSize(cols: Int, rows: Int) {}

    func link(at point: CGPoint) -> URL? { nil }
}
#endif
