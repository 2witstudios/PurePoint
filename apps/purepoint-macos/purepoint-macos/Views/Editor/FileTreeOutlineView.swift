import AppKit
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

        // Finder cannot show a remote file; offer a fetched local copy instead.
        vc.onOpenLocalCopy = { [fileTreeState] path in
            let host = fileTreeState.host
            Task {
                guard let url = try? await FileIOService.localCopy(of: path, host: host) else { return }
                NSWorkspace.shared.open(url)
            }
        }

        return vc
    }

    func updateNSViewController(_ vc: FileTreeOutlineViewController, context: Context) {
        vc.isLocal = fileTreeState.isLocal
        vc.updateNodes(fileTreeState.rootNodes)
    }
}
