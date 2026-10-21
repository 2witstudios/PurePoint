#if os(iOS)
import Foundation
import QuickLook
import Testing
import UIKit

@testable import PurePoint

@MainActor
struct LocalFilePresenterTests {
    @Test func previewableFilesOpenInQuickLookAndOthersInAShareSheet() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let text = directory.appendingPathComponent("notes.txt")
        try Data("hello".utf8).write(to: text)
        #expect(LocalFilePresenter.presentation(for: text) == .quickLook)
        #expect(LocalFilePresenter.presentation(for: URL(string: "https://example.com/x")!) == .shareSheet)
    }

    @Test func aQuickLookPreviewServesItsOneFile() {
        let url = URL(fileURLWithPath: "/tmp/a.txt")
        let preview = LocalFilePreviewController(url: url)
        #expect(preview.numberOfPreviewItems(in: preview) == 1)
        #expect(preview.previewController(preview, previewItemAt: 0).previewItemURL == url)
    }

    @Test func localCopiesOfLocalFilesAreTheFilesThemselves() async throws {
        let url = try await FileIOService.localCopy(of: "/tmp/a.txt", host: LocalProjectHost.shared)
        #expect(url == URL(fileURLWithPath: "/tmp/a.txt"))
    }

    @Test func tooLargeCopiesNameTheIPad() {
        #expect(LocalCopyError.tooLarge.errorDescription?.contains("this iPad") == true)
    }
}
#endif
