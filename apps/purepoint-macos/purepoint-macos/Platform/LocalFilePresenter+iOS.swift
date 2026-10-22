#if os(iOS)
import QuickLook
import UIKit

/// The iPad's replacement for the Mac's `NSWorkspace.open` of a file: there is no default app
/// to hand it to, so a file Quick Look can preview opens in Quick Look (whose share button
/// still reaches other apps), and anything else opens in a share sheet.
@MainActor
enum LocalFilePresenter {
    enum Presentation: Equatable {
        case quickLook, shareSheet
    }

    /// Where a share sheet's popover points: a view and a rect in its coordinates.
    struct Anchor {
        let view: UIView
        let rect: CGRect
    }

    /// Quick Look when it can preview `url`, otherwise the share sheet.
    static func presentation(for url: URL) -> Presentation {
        QLPreviewController.canPreview(url as NSURL) ? .quickLook : .shareSheet
    }

    /// Presents `url` from `presenter` (the front-most view controller when nil).
    static func present(_ url: URL, from presenter: UIViewController? = nil, anchor: Anchor? = nil) {
        guard let presenter = presenter.map(frontMost(from:)) ?? frontMostViewController() else { return }
        let controller: UIViewController
        switch presentation(for: url) {
        case .quickLook:
            controller = LocalFilePreviewController(url: url)
        case .shareSheet:
            controller = UIActivityViewController(activityItems: [url], applicationActivities: nil)
            if let popover = controller.popoverPresentationController {
                let anchor = anchor ?? Anchor(view: presenter.view, rect: centerRect(in: presenter.view))
                popover.sourceView = anchor.view
                popover.sourceRect = anchor.rect
            }
        }
        presenter.present(controller, animated: true)
    }

    /// The Mac's Open Local Copy: fetch the remote file to a private temp copy, then present it.
    /// A failed fetch shows the Mac's warning, as an alert.
    static func openLocalCopy(
        of path: String, host: any ProjectHost, from presenter: UIViewController? = nil, anchor: Anchor? = nil
    ) {
        Task {
            do {
                let url = try await FileIOService.localCopy(of: path, host: host)
                present(url, from: presenter, anchor: anchor)
            } catch {
                showWarning(
                    title: "Could not open a local copy of \((path as NSString).lastPathComponent)",
                    message: error.localizedDescription, from: presenter)
            }
        }
    }

    static func showWarning(title: String, message: String, from presenter: UIViewController? = nil) {
        guard let presenter = presenter.map(frontMost(from:)) ?? frontMostViewController() else { return }
        let alert = UIAlertController(title: title, message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default))
        presenter.present(alert, animated: true)
    }

    // MARK: - Presenter lookup

    /// The top of the presentation stack in the foreground key window.
    static func frontMostViewController() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let active = scenes.filter { $0.activationState == .foregroundActive }
        let window = (active.isEmpty ? scenes : active).lazy.compactMap { scene in
            scene.keyWindow ?? scene.windows.first { $0.isKeyWindow } ?? scene.windows.first
        }.first
        return window?.rootViewController.map(frontMost(from:))
    }

    private static func frontMost(from controller: UIViewController) -> UIViewController {
        var top = controller
        while let presented = top.presentedViewController, !presented.isBeingDismissed {
            top = presented
        }
        return top
    }

    private static func centerRect(in view: UIView) -> CGRect {
        CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 0, height: 0)
    }
}

/// A Quick Look preview of one file. It is its own data source, since Quick Look holds its
/// data source weakly.
final class LocalFilePreviewController: QLPreviewController, QLPreviewControllerDataSource {
    let url: URL

    init(url: URL) {
        self.url = url
        super.init(nibName: nil, bundle: nil)
        dataSource = self
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: previews are built in code")
    }

    nonisolated func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

    nonisolated func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> any QLPreviewItem {
        url as NSURL
    }
}
#endif
