#if os(iOS)
import PhotosUI
import SwiftTerm
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// UIViewRepresentable bridge that creates a ScrollableTerminal connected to
/// the daemon for an agent. Handles lazy creation and deferred start
/// to prevent the 1-column PTY bug.
struct TerminalPaneView: UIViewRepresentable {
    let agent: AgentModel

    func makeUIView(context: Context) -> TerminalPaneNSView {
        TerminalPaneNSView(agent: agent)
    }

    func updateUIView(_ uiView: TerminalPaneNSView, context: Context) {
        // Agent identity doesn't change; status updates are cosmetic only
    }

    static func dismantleUIView(_ uiView: TerminalPaneNSView, coordinator: ()) {
        uiView.tearDown()
    }
}

/// iOS twin of the Mac pane view: wraps a ScrollableTerminal and manages the daemon attach
/// lifecycle. It keeps the Mac type name, which `TerminalViewCache` and the shell use.
class TerminalPaneNSView: UIView {
    let agent: AgentModel
    var onMouseDown: (() -> Void)?
    /// Live status lookup. `agent` is a snapshot from creation, so its status
    /// is stale; set by `TerminalViewCache`.
    var isAgentAlive: () -> Bool = { true }
    /// Called when output arrives while this terminal is not on screen — a background tab.
    /// Set by `TerminalViewCache`.
    var onBackgroundOutput: (() -> Void)?
    /// The agent's daemon. Set by `TerminalViewCache`; on iPad every agent is remote.
    var client = DaemonEndpoint.local.makeClient() {
        didSet {
            terminal?.attachmentClient = client
            terminal?.usesRemoteHost = true
        }
    }
    /// A resize sends SIGWINCH, and full-screen programs answer by redrawing everything.
    /// That redraw is not new activity, so output until this moment does not count.
    private var resizeRedrawSettlesAt = Date.distantPast
    private var lastLayoutSize: CGSize = .zero
    private(set) var terminal: ScrollableTerminal?
    private var attachTask: Task<Void, Never>?
    private var attachStarted = false
    private var isAttachDone = false
    /// The daemon closed the last stream because the agent exited.
    private var didStreamEnd = false
    private(set) var isAgentGone = false
    private var heartbeatTimer: Timer?
    private var spinner: UIActivityIndicatorView?
    /// Sends photos and files to the agent's machine and types their paths.
    private var attachButton: UIButton?
    private var spinnerShownAt = Date()

    init(agent: AgentModel) {
        self.agent = agent
        super.init(frame: .zero)
        backgroundColor = TerminalTheme.background
    }

    required init?(coder: NSCoder) { fatalError() }

    private func ensureTerminal() {
        guard terminal == nil else { return }

        let tv = ScrollableTerminal(frame: bounds)
        tv.clipsToBounds = true
        tv.pinToEdges(of: self)
        tv.terminalView.hideCursor(source: tv.terminalView.getTerminal())
        tv.attachmentClient = client
        tv.usesRemoteHost = true
        tv.onAttachingChanged = { [weak self] attaching in self?.showAttaching(attaching) }
        tv.onAttachmentError = { [weak self] message in self?.showAttachmentError(message) }
        terminal = tv
        installAttachButton()

        // Show a small spinner while waiting for first output
        let indicator = UIActivityIndicatorView(style: .medium)
        indicator.translatesAutoresizingMaskIntoConstraints = false
        indicator.startAnimating()
        addSubview(indicator)
        NSLayoutConstraint.activate([
            indicator.centerXAnchor.constraint(equalTo: centerXAnchor),
            indicator.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        spinner = indicator
        spinnerShownAt = Date()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            setNeedsLayout()
            // Force terminal to redraw after being re-parented between containers.
            // Without this, closing a grid pane (2→1) leaves the surviving terminal blank
            // because the AnyView type change in PaneGridView destroys and recreates the
            // SwiftUI view hierarchy, moving this view to a new container.
            if let tv = terminal {
                tv.setNeedsDisplay()
                tv.terminalView.setNeedsDisplay()
            }
        }
    }

    override func layoutSubviews() {
        if bounds.size != lastLayoutSize {
            lastLayoutSize = bounds.size
            resizeRedrawSettlesAt = Date().addingTimeInterval(1.5)
        }
        super.layoutSubviews()
        // Create terminal only after we have a real frame, preventing 0-column grids
        if window != nil && terminal == nil && bounds.width > 1 {
            ensureTerminal()
            terminal?.layoutIfNeeded()
        }
        // Start daemon attach only after the first layout pass gives us a real frame.
        // Starting while frame is .zero causes SwiftTerm to report 1-column size.
        if let tv = terminal, tv.bounds.width > 1 {
            if !attachStarted {
                attachStarted = true
                startDaemonAttach()
                startHeartbeat()
                // New panes should be immediately interactive.
                DispatchQueue.main.async { [weak self] in
                    self?.focusTerminal()
                }
            } else if let task = attachTask, task.isCancelled || isAttachDone, canReattach {
                // Session died — restart
                startDaemonAttach()
            }
        }
    }

    private func noteOutput() {
        guard Date() >= resizeRedrawSettlesAt, isHidden || window == nil else { return }
        onBackgroundOutput?()
    }

    private func startDaemonAttach() {
        guard let tv = terminal else { return }

        // Clean up previous session before creating a new one
        attachTask?.cancel()
        let oldSession = tv.attachSession
        if oldSession != nil {
            Task { await oldSession?.stop() }
        }

        isAttachDone = false
        didStreamEnd = false
        let session = DaemonAttachSession(
            agentId: agent.id,
            terminalView: tv.terminalView,
            // Every attach replays the daemon's whole buffer; only the first
            // one lands in an empty terminal.
            resetBeforeReplay: oldSession != nil,
            localCols: tv.localGridSize.cols, localRows: tv.localGridSize.rows,
            onFirstOutput: { [weak self] in self?.handleFirstOutput() },
            onOutput: { [weak self] in self?.noteOutput() },
            client: client
        )
        tv.attachSession = session

        attachTask = Task { [weak self] in
            await session.start()
            let agentGone = await session.isAgentGone
            let streamEnded = await session.didStreamEnd
            await MainActor.run {
                self?.isAttachDone = true
                self?.didStreamEnd = streamEnded
                if agentGone {
                    self?.isAgentGone = true
                    self?.attachButton?.isHidden = true
                    self?.heartbeatTimer?.invalidate()
                    self?.heartbeatTimer = nil
                }
            }
        }
    }

    private func handleFirstOutput() {
        removeSpinner()
        #if DEBUG
        terminal?.runScriptedInputIfRequested()
        #endif
    }

    /// Restart the attach session if it has died and the view has a valid frame.
    func reconnectIfNeeded() {
        guard !isAgentGone else { return }
        guard isAttachDone, canReattach, let tv = terminal, tv.bounds.width > 1 else { return }
        startDaemonAttach()
    }

    /// After the agent exits, the terminal already holds all of its output.
    /// Reattach only if it is running again (e.g. resumed).
    private var canReattach: Bool {
        !didStreamEnd || isAgentAlive()
    }

    override var canBecomeFirstResponder: Bool { true }

    func focusTerminal() {
        guard !InlineRenameFocus.isActive, let tv = terminal?.terminalView else { return }
        guard window != nil, tv.isFirstResponder || tv.becomeFirstResponder() else { return }
        let session = terminal?.attachSession
        Task { await session?.claimSize() }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        onMouseDown?()
        focusTerminal()
        super.touchesBegan(touches, with: event)
    }

    private func startHeartbeat() {
        guard heartbeatTimer == nil else { return }
        heartbeatTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.reconnectIfNeeded() }
        }
    }

    private func removeSpinner() {
        guard let spinner else { return }
        let elapsed = Date().timeIntervalSince(spinnerShownAt)
        if elapsed >= 0.5 {
            spinner.stopAnimating()
            spinner.removeFromSuperview()
            self.spinner = nil
        } else {
            DispatchQueue.main.asyncAfter(deadline: .now() + (0.5 - elapsed)) { [weak self] in
                self?.spinner?.stopAnimating()
                self?.spinner?.removeFromSuperview()
                self?.spinner = nil
            }
        }
    }

    // MARK: - Attachments

    private func installAttachButton() {
        var config = UIButton.Configuration.glass()
        config.image = UIImage(systemName: "paperclip")
        config.cornerStyle = .capsule
        config.buttonSize = .small
        let button = UIButton(configuration: config)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.accessibilityLabel = "Attach photos or files"
        button.showsMenuAsPrimaryAction = true
        button.menu = UIMenu(children: [
            UIAction(title: "Photos", image: UIImage(systemName: "photo")) { [weak self] _ in self?.presentPhotoPicker() },
            UIAction(title: "Files", image: UIImage(systemName: "doc")) { [weak self] _ in self?.presentFilePicker() },
        ])
        addSubview(button)
        NSLayoutConstraint.activate([
            button.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            button.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
        ])
        attachButton = button
    }

    private func showAttaching(_ attaching: Bool) {
        attachButton?.configuration?.showsActivityIndicator = attaching
        attachButton?.isEnabled = !attaching
    }

    private func showAttachmentError(_ message: String) {
        let alert = UIAlertController(title: "Couldn't attach", message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default))
        presenter?.present(alert, animated: true)
    }

    /// The view controller showing this pane, which presents its pickers.
    private var presenter: UIViewController? {
        var responder: UIResponder? = self
        while let next = responder?.next {
            if let controller = next as? UIViewController { return controller }
            responder = next
        }
        return nil
    }

    private func presentPhotoPicker() {
        var config = PHPickerConfiguration()
        config.filter = .images
        config.selectionLimit = 10
        let picker = PHPickerViewController(configuration: config)
        picker.delegate = self
        presenter?.present(picker, animated: true)
    }

    private func presentFilePicker() {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: true)
        picker.allowsMultipleSelection = true
        picker.delegate = self
        presenter?.present(picker, animated: true)
    }

    func tearDown() {
        removeSpinner()
        heartbeatTimer?.invalidate()
        heartbeatTimer = nil
        attachTask?.cancel()
        attachTask = nil
        terminal?.tearDown()
    }
}

extension TerminalPaneNSView: PHPickerViewControllerDelegate, UIDocumentPickerDelegate {
    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        picker.dismiss(animated: true)
        guard !results.isEmpty else { return }
        let providers = results.map(\.itemProvider)
        let date = Date()
        Task { [weak self] in
            do {
                var files: [(data: Data, name: String)] = []
                for (index, provider) in providers.enumerated() {
                    let name = AttachmentUpload.photoName(at: date, index: providers.count > 1 ? index + 1 : nil)
                    let data = try await ScrollableTerminal.loadData(provider, type: .image)
                    files.append((try await Task.detached { try AttachmentUpload.photoJPEG(data, name: name) }.value, name))
                }
                self?.terminal?.attach(files)
            } catch {
                self?.showAttachmentError(error.localizedDescription)
            }
        }
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        // Picked as copies, so these files are readable without security-scoped access.
        Task { [weak self] in
            let read = await Task.detached {
                Result { try urls.map { (data: try AttachmentUpload.readFile(at: $0), name: $0.lastPathComponent) } }
            }.value
            switch read {
            case .success(let files): self?.terminal?.attach(files)
            case .failure(let error): self?.showAttachmentError(error.localizedDescription)
            }
        }
    }
}
#endif
