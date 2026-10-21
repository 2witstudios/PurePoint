#if os(iOS)
import SwiftUI
import UIKit
import Vision
import VisionKit

/// Scans a Mac's Point Guard enrollment QR with VisionKit, as the iPhone app does.
struct PointGuardPairingScanner: View {
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?
    let onPair: (PiEnrollmentCode) -> Void

    var body: some View {
        NavigationStack {
            PointGuardQRScanner(onPair: onPair, onError: { error = $0 })
                .ignoresSafeArea(edges: .bottom)
                .safeAreaInset(edge: .bottom) {
                    if let error {
                        Text(error).font(.callout).padding(20).frame(maxWidth: .infinity).background(.regularMaterial)
                    }
                }
                .navigationTitle("Pair with your Mac").navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                    }
                }
        }
    }
}

private struct PointGuardQRScanner: UIViewControllerRepresentable {
    let onPair: (PiEnrollmentCode) -> Void
    let onError: (String) -> Void

    func makeUIViewController(context: Context) -> ScannerController {
        ScannerController(onPair: onPair, onError: onError)
    }
    func updateUIViewController(_ uiViewController: ScannerController, context: Context) {}
    static func dismantleUIViewController(_ uiViewController: ScannerController, coordinator: ()) {
        uiViewController.scanner.stopScanning()
    }

    final class ScannerController: UIViewController, DataScannerViewControllerDelegate {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])], qualityLevel: .accurate,
            recognizesMultipleItems: false, isHighFrameRateTrackingEnabled: false, isPinchToZoomEnabled: true,
            isGuidanceEnabled: true, isHighlightingEnabled: true)
        private let onPair: (PiEnrollmentCode) -> Void
        private let onError: (String) -> Void
        private var finished = false
        private var lastPayload: String?

        init(onPair: @escaping (PiEnrollmentCode) -> Void, onError: @escaping (String) -> Void) {
            self.onPair = onPair; self.onError = onError
            super.init(nibName: nil, bundle: nil)
        }
        required init?(coder: NSCoder) { fatalError("Not used") }

        override func viewDidLoad() {
            super.viewDidLoad()
            scanner.delegate = self
            addChild(scanner); view.addSubview(scanner.view)
            scanner.view.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                scanner.view.topAnchor.constraint(equalTo: view.topAnchor),
                scanner.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
                scanner.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
                scanner.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            ])
            scanner.didMove(toParent: self)
        }
        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            do { try scanner.startScanning() } catch {
                onError("Could not start the camera. Close this screen and try again.")
            }
        }
        override func viewWillDisappear(_ animated: Bool) { scanner.stopScanning(); super.viewWillDisappear(animated) }

        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            read(addedItems)
        }
        func dataScanner(_ dataScanner: DataScannerViewController, didUpdate updatedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            read(updatedItems)
        }
        func dataScanner(_ dataScanner: DataScannerViewController, didTapOn item: RecognizedItem) { read([item]) }
        func dataScanner(
            _ dataScanner: DataScannerViewController,
            becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable
        ) {
            onError("The camera is unavailable. Close this screen and try again.")
        }

        private func read(_ items: [RecognizedItem]) {
            guard !finished else { return }
            for item in items {
                guard case .barcode(let barcode) = item, let payload = barcode.payloadStringValue, payload != lastPayload
                else { continue }
                lastPayload = payload
                guard let code = PiEnrollmentCode.parse(payload) else {
                    onError("This code is expired or unsupported. Create a new Connect phone QR in PurePoint on your Mac.")
                    continue
                }
                finished = true; scanner.stopScanning(); onPair(code); return
            }
        }
    }
}
#endif
