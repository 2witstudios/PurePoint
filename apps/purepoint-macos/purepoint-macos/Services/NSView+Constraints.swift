import AppKit

extension NSView {
    /// Pin all edges to a parent view and add as subview.
    /// Sets `translatesAutoresizingMaskIntoConstraints` to false automatically.
    func pinToEdges(of parent: NSView) {
        translatesAutoresizingMaskIntoConstraints = false
        parent.addSubview(self)
        NSLayoutConstraint.activate([
            topAnchor.constraint(equalTo: parent.topAnchor),
            leadingAnchor.constraint(equalTo: parent.leadingAnchor),
            trailingAnchor.constraint(equalTo: parent.trailingAnchor),
            bottomAnchor.constraint(equalTo: parent.bottomAnchor),
        ])
    }
}
