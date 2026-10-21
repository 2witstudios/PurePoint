#if os(iOS)
import UIKit

// MARK: - CommandPalettePanel

/// Placeholder twin of the Mac palette panel, with the same open and show entry points. It
/// presents a labelled sheet over the window until the iPad palette overlay lands (IPAD-6.8).
class CommandPalettePanel: UIViewController {
    private static weak var current: CommandPalettePanel?
    private static let openCoordinator = CommandPaletteOpenCoordinator()

    let paletteViewController = CommandPaletteViewController()

    init() {
        super.init(nibName: nil, bundle: nil)
        modalPresentationStyle = .formSheet
        preferredContentSize = CGSize(width: 500, height: 380)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        addChild(paletteViewController)
        paletteViewController.view.frame = view.bounds
        paletteViewController.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(paletteViewController.view)
        paletteViewController.didMove(toParent: self)
    }

    func showRelativeTo(window: NSWindow?) {
        guard var presenter = window?.rootViewController else { return }
        while let presented = presenter.presentedViewController { presenter = presented }
        presenter.present(self, animated: true)
    }

    func dismiss() {
        if CommandPalettePanel.current === self {
            CommandPalettePanel.current = nil
        }
        presentingViewController?.dismiss(animated: true)
    }

    private var isVisible: Bool { presentingViewController != nil }

    @discardableResult
    static func requestOpen(_ open: @escaping @MainActor () async -> Void) -> Task<Void, Never>? {
        // Toggle an already visible palette immediately, without starting another load.
        if let existing = current, existing.isVisible {
            existing.dismiss()
            return nil
        }
        return openCoordinator.request(open)
    }

    @discardableResult
    static func show(
        relativeTo window: NSWindow?,
        items: [CommandPaletteItem] = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.allVariants, agents: [], swarms: []),
        usePointGuardNames: Bool = true,
        initialItem: CommandPaletteItem? = nil,
        onSelect: @escaping (CommandPaletteResult) -> Void
    ) -> CommandPalettePanel {
        if let existing = current, existing.isVisible {
            existing.dismiss()
            return existing
        }
        let panel = CommandPalettePanel()
        let vc = panel.paletteViewController
        vc.onSelect = { [weak panel] result in
            panel?.dismiss()
            onSelect(result)
        }
        vc.onDismiss = { [weak panel] in
            panel?.dismiss()
        }
        vc.usePointGuardNames = usePointGuardNames
        vc.setItems(items)
        panel.showRelativeTo(window: window ?? NSApp.keyWindow)
        if let initialItem { vc.selectItem(initialItem) }
        current = panel
        return panel
    }
}

// MARK: - CommandPaletteViewController

/// Placeholder twin of the Mac palette content: it holds the items and reports nothing selected.
class CommandPaletteViewController: UIViewController {
    var onSelect: ((CommandPaletteResult) -> Void)?
    var onDismiss: (() -> Void)?
    var usePointGuardNames = true
    var availableItems: [CommandPaletteItem] = []

    override func viewDidLoad() {
        super.viewDidLoad()
        NotYetOnIPad.addLabel("CommandPalettePanel", to: view)
        let close = UIButton(
            configuration: .plain(),
            primaryAction: UIAction(title: "Close") { [weak self] _ in self?.handleEscape() })
        close.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(close)
        NSLayoutConstraint.activate([
            close.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 8),
            close.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -8),
        ])
    }

    func setItems(_ items: [CommandPaletteItem]) {
        availableItems = items
    }

    func selectItem(_ item: CommandPaletteItem) {}

    func handleEscape() {
        onDismiss?()
    }
}
#endif
