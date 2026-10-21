#if os(macOS)
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
                do {
                    NSWorkspace.shared.open(try await FileIOService.localCopy(of: path, host: host))
                } catch {
                    let alert = NSAlert()
                    alert.messageText = "Could not open a local copy of \((path as NSString).lastPathComponent)"
                    alert.informativeText = error.localizedDescription
                    alert.alertStyle = .warning
                    alert.runModal()
                }
            }
        }

        return vc
    }

    func updateNSViewController(_ vc: FileTreeOutlineViewController, context: Context) {
        vc.isLocal = fileTreeState.isLocal
        vc.updateNodes(fileTreeState.rootNodes)
    }
}
#endif
