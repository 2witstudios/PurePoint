#if os(macOS)
import AppKit

// Dragging a project row reorders it within its section. The order is saved on the machine
// that owns the project, so every device lists it the same way.

@MainActor
extension SidebarOutlineViewController {
    static let projectDragType = NSPasteboard.PasteboardType("com.purepoint.sidebar-project")

    func registerProjectDrag() {
        outlineView.registerForDraggedTypes([Self.projectDragType])
        outlineView.setDraggingSourceOperationMask(.move, forLocal: true)
    }

    func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
        guard let node = item as? SidebarNode, case .project(let project) = node.kind else { return nil }
        let pasteboardItem = NSPasteboardItem()
        pasteboardItem.setString(project.projectId, forType: Self.projectDragType)
        return pasteboardItem
    }

    func outlineView(
        _ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?,
        proposedChildIndex index: Int
    ) -> NSDragOperation {
        guard let projectId = info.draggingPasteboard.string(forType: Self.projectDragType) else { return [] }
        var parent = item as? SidebarNode
        var childIndex = index
        // Hovering on or inside a project means "before that project", among its siblings.
        if let node = parent, case .project = node.kind {
            parent = outlineView.parent(forItem: node) as? SidebarNode
            childIndex = outlineView.childIndex(forItem: node)
            outlineView.setDropItem(parent, dropChildIndex: childIndex)
        }
        guard childIndex != NSOutlineViewDropOnItemIndex,
            SidebarTree.projectDrop(projectId: projectId, parent: parent, childIndex: childIndex, roots: projectNodes)
                != nil
        else { return [] }
        return .move
    }

    func outlineView(
        _ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int
    ) -> Bool {
        guard let projectId = info.draggingPasteboard.string(forType: Self.projectDragType),
            let drop = SidebarTree.projectDrop(
                projectId: projectId, parent: item as? SidebarNode, childIndex: index, roots: projectNodes)
        else { return false }
        drop.project.appState?.moveProject(drop.project, to: drop.index)
        return true
    }
}
#endif
