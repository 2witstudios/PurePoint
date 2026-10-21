#if os(iOS)
import SwiftUI

/// iOS twin of the Mac file-tree representable, with the same inputs. Opening a fetched local
/// copy (the Mac's Open in default app) is a share sheet on iPad (IPAD-7.2), so it is not wired.
struct FileTreeOutlineView: UIViewControllerRepresentable {
    var fileTreeState: FileTreeState
    var onFileSelected: (String, String) -> Void  // (absolutePath, name)

    func makeUIViewController(context: Context) -> FileTreeOutlineViewController {
        let vc = FileTreeOutlineViewController()
        vc.onFileSelected = { path, name in
            onFileSelected(path, name)
        }
        vc.onNodeExpanded = { [fileTreeState] node in
            fileTreeState.expandNode(node)
        }
        vc.onNodeCollapsed = { [fileTreeState] node in
            fileTreeState.collapseNode(node)
        }
        return vc
    }

    func updateUIViewController(_ vc: FileTreeOutlineViewController, context: Context) {
        vc.isLocal = fileTreeState.isLocal
        vc.updateNodes(fileTreeState.rootNodes)
    }
}
#endif
