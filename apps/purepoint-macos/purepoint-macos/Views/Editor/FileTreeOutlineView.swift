import SwiftUI

struct FileTreeOutlineView: NSViewControllerRepresentable {
    var fileTreeState: FileTreeState
    var onFileSelected: (String, String) -> Void  // (absolutePath, name)

    func makeNSViewController(context: Context) -> FileTreeOutlineViewController {
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

    func updateNSViewController(_ vc: FileTreeOutlineViewController, context: Context) {
        vc.updateNodes(fileTreeState.rootNodes)
    }
}
