#if os(iOS)
import UIKit

/// Placeholder twin of the Mac file-tree outline controller, with the same callbacks and node
/// input. It shows a label until the daemon-backed tree lands (IPAD-6.4).
@MainActor
class FileTreeOutlineViewController: UIViewController {
    var rootNodes: [FileTreeNode] = []

    var onFileSelected: ((String, String) -> Void)?  // (absolutePath, name)
    var onNodeExpanded: ((FileTreeNode) -> Void)?
    var onNodeCollapsed: ((FileTreeNode) -> Void)?
    /// Remote trees cannot be revealed in Finder; their files open as fetched local copies.
    var isLocal = true
    var onOpenLocalCopy: ((String) -> Void)?

    override func loadView() {
        view = UIView()
        NotYetOnIPad.addLabel("FileTreeOutlineViewController", to: view)
    }

    func updateNodes(_ nodes: [FileTreeNode]) {
        rootNodes = nodes
    }
}
#endif
