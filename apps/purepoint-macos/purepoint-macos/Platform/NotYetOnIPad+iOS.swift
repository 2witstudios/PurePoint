#if os(iOS)
import SwiftUI
import UIKit

/// The labelled stand-in that placeholder twins render until their real UIKit twin lands
/// (Phase 6). Each twin names its Mac type, so the iPad shell shows which surface is missing.
enum NotYetOnIPad {
    static func text(_ name: String) -> String { "\(name) — not yet on iPad" }

    /// A centered secondary label filling `view`, for UIKit placeholder twins.
    static func addLabel(_ name: String, to view: UIView) {
        let label = UILabel()
        label.text = text(name)
        label.font = .preferredFont(forTextStyle: .footnote)
        label.textColor = .secondaryLabel
        label.textAlignment = .center
        label.numberOfLines = 0
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            label.leadingAnchor.constraint(greaterThanOrEqualTo: view.leadingAnchor, constant: 8),
            label.trailingAnchor.constraint(lessThanOrEqualTo: view.trailingAnchor, constant: -8),
        ])
    }
}

/// The SwiftUI form of the placeholder label.
struct NotYetOnIPadLabel: View {
    let name: String

    var body: some View {
        Text(NotYetOnIPad.text(name))
            .font(.footnote)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .padding(8)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
#endif
