#if os(iOS)
import UIKit

// MARK: - CommandPalettePanel

/// iOS twin of the Mac's floating palette panel: the same 500pt wide rounded panel with a
/// shadow, placed where the Mac places it over the window (centered, 60pt above the middle at
/// its full height, keeping its top edge as it resizes). It is presented over the window
/// without a transition, as the Mac panel orders front, and a click or tap outside it
/// dismisses it, as the Mac's mouse-down monitor does.
class CommandPalettePanel: UIViewController {
    private static weak var current: CommandPalettePanel?
    private static let openCoordinator = CommandPaletteOpenCoordinator()

    static let panelWidth: CGFloat = 500
    static let maxPanelHeight: CGFloat = 380
    /// The Mac panel opens at its full height with its center 60pt above the window's.
    static let topOffsetAboveCenter: CGFloat = maxPanelHeight / 2 + 60
    static let cornerRadius: CGFloat = 12

    let paletteViewController = CommandPaletteViewController()

    /// A dismissal asked for while the panel was still being presented, which UIKit ignores.
    private var dismissWhenPresented = false

    /// Carries the shadow; the palette's rounded, clipped content sits inside it.
    private let frameView = UIView()
    /// Lazy so the palette content can size the panel while it loads, before `viewDidLoad` ends.
    private lazy var heightConstraint: NSLayoutConstraint = {
        let constraint = frameView.heightAnchor.constraint(equalToConstant: Self.maxPanelHeight)
        constraint.priority = .required - 1
        return constraint
    }()

    init() {
        super.init(nibName: nil, bundle: nil)
        modalPresentationStyle = .overFullScreen
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .clear

        let backdrop = UIView()
        backdrop.backgroundColor = .clear
        backdrop.frame = view.bounds
        backdrop.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(backdrop)
        // Primary and secondary clicks outside the panel close it, as on the Mac; so does a tap.
        for buttons in [UIEvent.ButtonMask.primary, .secondary] {
            let tap = UITapGestureRecognizer(target: self, action: #selector(backdropTapped))
            tap.buttonMaskRequired = buttons
            backdrop.addGestureRecognizer(tap)
        }

        frameView.translatesAutoresizingMaskIntoConstraints = false
        frameView.layer.shadowColor = UIColor.black.cgColor
        frameView.layer.shadowOpacity = 0.3
        frameView.layer.shadowRadius = 20
        frameView.layer.shadowOffset = CGSize(width: 0, height: 10)
        view.addSubview(frameView)

        addChild(paletteViewController)
        let content = paletteViewController.view!
        content.translatesAutoresizingMaskIntoConstraints = false
        frameView.addSubview(content)
        paletteViewController.didMove(toParent: self)

        let width = frameView.widthAnchor.constraint(equalToConstant: Self.panelWidth)
        width.priority = .required - 1
        let top = frameView.topAnchor.constraint(equalTo: view.centerYAnchor, constant: -Self.topOffsetAboveCenter)
        top.priority = .defaultHigh

        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo: frameView.topAnchor),
            content.leadingAnchor.constraint(equalTo: frameView.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: frameView.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: frameView.bottomAnchor),

            frameView.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            width,
            frameView.widthAnchor.constraint(lessThanOrEqualTo: view.safeAreaLayoutGuide.widthAnchor, constant: -32),
            heightConstraint,
            top,
            // A short window or the on-screen keyboard pushes the panel up, then shortens it.
            frameView.topAnchor.constraint(greaterThanOrEqualTo: view.safeAreaLayoutGuide.topAnchor, constant: 8),
            frameView.bottomAnchor.constraint(lessThanOrEqualTo: view.keyboardLayoutGuide.topAnchor, constant: -8),
        ])
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        if dismissWhenPresented {
            dismissWhenPresented = false
            super.dismiss(animated: false)
        }
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        frameView.layer.shadowPath =
            UIBezierPath(roundedRect: frameView.bounds, cornerRadius: Self.cornerRadius).cgPath
    }

    /// The panel's height, kept from its top edge like the Mac panel's `setFrame`.
    func setPanelHeight(_ height: CGFloat, animate: Bool) {
        guard heightConstraint.constant != height else { return }
        heightConstraint.constant = height
        guard animate, isViewLoaded, view.window != nil else { return }
        UIView.animate(withDuration: 0.2) {
            self.view.layoutIfNeeded()
        }
    }

    func showRelativeTo(window: NSWindow?) {
        guard var presenter = window?.rootViewController else { return }
        while let presented = presenter.presentedViewController, !presented.isBeingDismissed {
            presenter = presented
        }
        presenter.present(self, animated: false)
    }

    func dismiss() {
        if CommandPalettePanel.current === self {
            CommandPalettePanel.current = nil
        }
        guard isVisible else { return }
        if isBeingPresented {
            dismissWhenPresented = true
        } else {
            super.dismiss(animated: false)
        }
    }

    @objc private func backdropTapped() {
        dismiss()
    }

    private var isVisible: Bool { presentingViewController != nil && !isBeingDismissed && !dismissWhenPresented }

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
        panel.loadViewIfNeeded()
        vc.setItems(items)
        panel.showRelativeTo(window: window ?? NSApp.keyWindow)
        if let initialItem { vc.selectItem(initialItem) }
        current = panel
        return panel
    }

    /// Fills `AppLifecycleHooks.presentPalette`, the menu bar's New Agent command: the palette
    /// opens over the window the user is working in, as the Mac's opens over the key window.
    static func registerPresenter() {
        AppLifecycleHooks.presentPalette = { presentation in
            show(
                relativeTo: NSApp.keyWindow, items: presentation.items,
                usePointGuardNames: presentation.usePointGuardNames, onSelect: presentation.onSelect)
        }
    }
}
#endif
