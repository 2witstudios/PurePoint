#if os(macOS)
import Foundation
import ImageIO
import PDFKit
import UniformTypeIdentifiers

/// Uses the same attachment preparation limits as the iOS composer.
nonisolated enum PiAttachmentImport {
    static func file(_ url: URL) throws -> PiComposerAttachment {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let values = try url.resourceValues(forKeys: [.fileSizeKey, .contentTypeKey])
        guard let size = values.fileSize, size <= 20 * 1024 * 1024 else {
            throw PiComposerError("Choose a file smaller than 20 MB.")
        }
        let name = String(url.lastPathComponent.prefix(120))
        if values.contentType?.conforms(to: .image) == true {
            return try image(Data(contentsOf: url), name: name)
        }
        let text: String
        if values.contentType?.conforms(to: .pdf) == true {
            guard let document = PDFDocument(url: url), document.pageCount <= 30,
                let extracted = document.string,
                !extracted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                throw PiComposerError(
                    "Choose a PDF with selectable text and up to 30 pages. For scanned pages, attach images instead.")
            }
            text = extracted
        } else {
            guard size <= 32 * 1024,
                let decoded = String(data: try Data(contentsOf: url), encoding: .utf8), !decoded.contains("\0")
            else { throw PiComposerError("Choose a UTF-8 text file smaller than 32 KiB.") }
            text = decoded
        }
        guard text.utf8.count <= 32 * 1024 else {
            throw PiComposerError("The extracted text exceeds 32 KiB. Choose a shorter document.")
        }
        return PiComposerAttachment(id: UUID().uuidString, name: name, mimeType: "text/plain", data: Data(text.utf8))
    }

    static func image(_ data: Data, name: String) throws -> PiComposerAttachment {
        guard data.count <= 20 * 1024 * 1024,
            let source = CGImageSourceCreateWithData(data as CFData, nil)
        else { throw PiComposerError("Choose an image smaller than 20 MB.") }
        for dimension in [1600, 1200, 800] {
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: dimension,
            ]
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
                throw PiComposerError("This image could not be opened.")
            }
            for quality in [0.8, 0.6, 0.4] {
                let encoded = NSMutableData()
                guard
                    let destination = CGImageDestinationCreateWithData(
                        encoded, UTType.jpeg.identifier as CFString, 1, nil)
                else {
                    throw PiComposerError("This image could not be prepared.")
                }
                CGImageDestinationAddImage(
                    destination, image, [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
                if CGImageDestinationFinalize(destination), encoded.length <= 256 * 1024 {
                    return PiComposerAttachment(
                        id: UUID().uuidString, name: name, mimeType: "image/jpeg", data: encoded as Data)
                }
            }
        }
        throw PiComposerError("This image could not be prepared. Choose a smaller image.")
    }
}
#endif
