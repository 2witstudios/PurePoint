#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// The iPad composer prepares PhotosPicker and fileImporter picks with the Mac's limits (IPAD-8.3).
@Suite("Point Guard iPad attachments")
struct PointGuardAttachmentIOSTests {
    private func photo(side: CGFloat) -> Data {
        let format = UIGraphicsImageRendererFormat(); format.scale = 1
        return UIGraphicsImageRenderer(size: CGSize(width: side, height: side), format: format).pngData { context in
            for row in stride(from: 0, to: side, by: 16) {
                UIColor(hue: row / side, saturation: 1, brightness: 1, alpha: 1).setFill()
                context.fill(CGRect(x: 0, y: row, width: side, height: 16))
            }
        }
    }

    private func temporaryFile(_ name: String, _ data: Data) throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + "-" + name)
        try data.write(to: url)
        return url
    }

    @Test func photoBecomesABoundedJPEG() throws {
        let attachment = try PiAttachmentImport.image(photo(side: 3000), name: "Photo.jpg")
        #expect(attachment.mimeType == "image/jpeg" && attachment.name == "Photo.jpg")
        #expect(attachment.data.count <= 256 * 1024)
    }

    @Test func oversizedOrUnreadablePhotosAreRefused() {
        #expect(throws: PiComposerError.self) {
            try PiAttachmentImport.image(Data(count: 20 * 1024 * 1024 + 1), name: "Photo.jpg")
        }
        #expect(throws: PiComposerError.self) { try PiAttachmentImport.image(Data("not an image".utf8), name: "x") }
    }

    @Test func importedFilesKeepTheTextLimits() throws {
        let note = try temporaryFile("notes.txt", Data("hello".utf8))
        defer { try? FileManager.default.removeItem(at: note) }
        let text = try PiAttachmentImport.file(note)
        #expect(text.mimeType == "text/plain" && text.data == Data("hello".utf8))

        let large = try temporaryFile("large.txt", Data(repeating: 65, count: 32 * 1024 + 1))
        defer { try? FileManager.default.removeItem(at: large) }
        #expect(throws: PiComposerError.self) { try PiAttachmentImport.file(large) }

        let image = try temporaryFile("shot.png", photo(side: 64))
        defer { try? FileManager.default.removeItem(at: image) }
        #expect(try PiAttachmentImport.file(image).mimeType == "image/jpeg")
    }
}
#endif
