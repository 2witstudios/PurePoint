#if os(iOS)
import SwiftUI

/// iOS twin of the Mac file-tree representable, with the same inputs.
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

        // The Mac opens a fetched local copy in its default app; the iPad shows it in Quick
        // Look or a share sheet anchored on the row.
        vc.onOpenLocalCopy = { [fileTreeState, weak vc] path in
            let anchor = vc.flatMap { vc in
                vc.sourceRect(forPath: path).map { LocalFilePresenter.Anchor(view: vc.outlineView, rect: $0) }
            }
            LocalFilePresenter.openLocalCopy(of: path, host: fileTreeState.host, from: vc, anchor: anchor)
        }

        return vc
    }

    func updateUIViewController(_ vc: FileTreeOutlineViewController, context: Context) {
        vc.isLocal = fileTreeState.isLocal
        vc.updateNodes(fileTreeState.rootNodes)
    }
}
#endif
