#if os(iOS)
import UIKit

// The iPad sidebar's context menus, confirmations and inline rename: the Mac outline's menus
// built from the shared `SidebarMenus` entries and run through the shared `perform(_:on:)`.

extension SidebarOutlineViewController {
    /// The machine's menu actions, reachable from VoiceOver's actions rotor.
    func machineAccessibilityActions(_ machine: MachineState, key: String) -> [UIAccessibilityCustomAction] {
        SidebarMenus.machineAccessibilityActions(machine).map { entry in
            UIAccessibilityCustomAction(name: entry.name) { [weak self] _ in
                guard let self, let node = self.nodesByKey[key] else { return false }
                self.perform(entry.action, on: node)
                return true
            }
        }
    }

    // MARK: - Menus

    /// A menu from the shared entries; each separator starts a new inline group, which UIKit
    /// draws as the divider the Mac menu shows. Actions look up the row when chosen, so a
    /// rebuild in between acts on the current row.
    func makeMenu(_ entries: [SidebarMenuEntry], key: String) -> UIMenu {
        var groups: [[UIMenuElement]] = [[]]
        for entry in entries {
            switch entry {
            case .separator:
                groups.append([])
            case .action(let title, let action, let isEnabled):
                groups[groups.count - 1].append(
                    UIAction(title: title, attributes: isEnabled ? [] : .disabled) { [weak self] _ in
                        guard let self, let node = self.nodesByKey[key] else { return }
                        if action == .renameWorkspace {
                            // The field can only take focus once the menu has gone.
                            self.pendingRenameKey = key
                        } else {
                            self.perform(action, on: node)
                        }
                    })
            }
        }
        return UIMenu(children: groups.filter { !$0.isEmpty }.map { UIMenu(options: .displayInline, children: $0) })
    }

    // MARK: - Confirmation Dialog

    func showConfirmation(_ confirmation: SidebarConfirmation, action: @escaping () -> Void) {
        let alert = UIAlertController(title: confirmation.title, message: confirmation.message, preferredStyle: .alert)
        let confirm = UIAlertAction(title: confirmation.confirmTitle, style: .default) { _ in action() }
        alert.addAction(confirm)
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        alert.preferredAction = confirm

        var presenter: UIViewController = view.window?.rootViewController ?? self
        while let presented = presenter.presentedViewController { presenter = presented }
        presenter.present(alert, animated: true)
    }

    // MARK: - Inline Rename

    /// Turns a workspace row's name into an inline rename field.
    func beginRename(_ node: SidebarNode) {
        guard case .workspace(let workspace) = node.kind,
            let indexPath = dataSource.indexPath(for: node.rowKey),
            let cell = outlineView.cellForItem(at: indexPath) as? SidebarRowCell,
            let original = cell.nameLabel?.text,
            let field = cell.beginRenaming()
        else { return }

        InlineRenameFocus.isActive = true
        editingWorkspaceId = workspace.id
        editingOriginalName = original
        editingTextField = field
        field.delegate = self
        field.onCancel = { [weak self] in self?.cancelRename() }

        // Defer so the context menu's focus-restoration teardown completes first.
        DispatchQueue.main.async { [weak self] in
            self?.beginFieldEditing(field, retriesLeft: 2)
        }
    }

    /// Make the field first responder and select its text; if something stole focus in the
    /// meantime, try again on the next tick.
    private func beginFieldEditing(_ field: SidebarRenameField, retriesLeft: Int) {
        guard editingTextField === field, view.window != nil else { return }
        if field.becomeFirstResponder() {
            field.selectAll(nil)
        } else if retriesLeft > 0 {
            DispatchQueue.main.async { [weak self] in
                self?.beginFieldEditing(field, retriesLeft: retriesLeft - 1)
            }
        }
    }

    private func cancelRename() {
        guard let field = editingTextField else { return }
        let cell = renamingCell(for: field)
        let original = editingOriginalName
        cleanupEditingState()
        cell?.endRenaming(showing: original)
        outlineView.becomeFirstResponder()
        applyDeferredRebuild()
    }

    fileprivate func commitRename(_ field: SidebarRenameField) {
        guard field === editingTextField, let workspaceId = editingWorkspaceId else { return }
        let typed = field.text ?? ""
        let newName = typed.trimmingCharacters(in: .whitespaces)
        let cell = renamingCell(for: field)
        let original = editingOriginalName
        cleanupEditingState()

        // Renaming a workspace renames the agent it is named after.
        if !newName.isEmpty, newName != original,
            let agentId = findWorkspace(id: workspaceId)?.primaryAgentId,
            let project = findProject(forWorkspaceId: workspaceId)
        {
            cell?.endRenaming(showing: typed)
            onRenameAgent?(project, agentId, newName)
        } else {
            cell?.endRenaming(showing: original)
        }
        applyDeferredRebuild()
    }

    private func renamingCell(for field: SidebarRenameField) -> SidebarRowCell? {
        outlineView.visibleCells.lazy.compactMap { $0 as? SidebarRowCell }.first { $0.renameField === field }
    }

    private func cleanupEditingState() {
        InlineRenameFocus.isActive = false
        editingTextField = nil
        editingOriginalName = nil
        editingWorkspaceId = nil
    }

    private func applyDeferredRebuild() {
        guard let deferred = deferredRebuild else { return }
        deferredRebuild = nil
        rebuildNodes(projects: deferred.projects, machines: deferred.machines)
    }
}

// MARK: - Context menus

extension SidebarOutlineViewController {
    func collectionView(
        _ collectionView: UICollectionView, contextMenuConfigurationForItemsAt indexPaths: [IndexPath], point: CGPoint
    ) -> UIContextMenuConfiguration? {
        guard indexPaths.count == 1, let indexPath = indexPaths.first,
            let key = dataSource.itemIdentifier(for: indexPath), let node = nodesByKey[key]
        else { return nil }
        let entries = SidebarMenus.contextMenu(for: node)
        guard !entries.isEmpty else { return nil }
        return UIContextMenuConfiguration(identifier: key as NSString, previewProvider: nil) { [weak self] _ in
            self?.makeMenu(entries, key: key)
        }
    }

    func collectionView(
        _ collectionView: UICollectionView, contextMenuConfiguration configuration: UIContextMenuConfiguration,
        highlightPreviewForItemAt indexPath: IndexPath
    ) -> UITargetedPreview? {
        rowPreview(at: indexPath)
    }

    func collectionView(
        _ collectionView: UICollectionView, contextMenuConfiguration configuration: UIContextMenuConfiguration,
        dismissalPreviewForItemAt indexPath: IndexPath
    ) -> UITargetedPreview? {
        rowPreview(at: indexPath)
    }

    /// The menu lifts just the row's rounded frame, like the Mac's context-click ring.
    private func rowPreview(at indexPath: IndexPath) -> UITargetedPreview? {
        guard let cell = outlineView.cellForItem(at: indexPath) as? SidebarRowCell else { return nil }
        let parameters = UIPreviewParameters()
        parameters.visiblePath = UIBezierPath(roundedRect: cell.rowView.bounds, cornerRadius: 5)
        // The row draws its own fill; the preview's default white would cover the sidebar
        // background behind its text.
        parameters.backgroundColor = .clear
        return UITargetedPreview(view: cell.rowView, parameters: parameters)
    }

    func collectionView(
        _ collectionView: UICollectionView, willEndContextMenuInteraction configuration: UIContextMenuConfiguration,
        animator: (any UIContextMenuInteractionAnimating)?
    ) {
        guard pendingRenameKey != nil else { return }
        let start = { [weak self] in
            guard let self, let key = self.pendingRenameKey else { return }
            self.pendingRenameKey = nil
            guard let node = self.nodesByKey[key] else { return }
            self.perform(.renameWorkspace, on: node)
        }
        if let animator {
            animator.addCompletion(start)
        } else {
            DispatchQueue.main.async { start() }
        }
    }
}

// MARK: - Inline Rename (UITextFieldDelegate)

extension SidebarOutlineViewController: UITextFieldDelegate {
    func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        textField.resignFirstResponder()
        return false
    }

    func textFieldDidEndEditing(_ textField: UITextField) {
        guard let field = textField as? SidebarRenameField else { return }
        commitRename(field)
    }
}
#endif
