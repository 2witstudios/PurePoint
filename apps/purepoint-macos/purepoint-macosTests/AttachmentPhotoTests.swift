import CoreGraphics
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers

@testable import PurePoint

/// Photo preparation shared by the Mac and iPad: no AppKit or UIKit, so both test targets run it.
struct AttachmentPhotoTests {
    /// A solid PNG of the given size.
    private func png(width: Int, height: Int) throws -> Data {
        let context = try #require(
            CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        let image = try #require(context.makeImage())
        let output = NSMutableData()
        let destination = try #require(
            CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        #expect(CGImageDestinationFinalize(destination))
        return output as Data
    }

    private func pixelSize(_ data: Data) throws -> (type: String?, width: Int?, height: Int?) {
        let source = try #require(CGImageSourceCreateWithData(data as CFData, nil))
        let properties = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
        return (
            CGImageSourceGetType(source) as String?, properties[kCGImagePropertyPixelWidth] as? Int,
            properties[kCGImagePropertyPixelHeight] as? Int
        )
    }

    @Test func givenLargePhotoShouldBecomeJPEGWithinTheLongEdge() throws {
        let size = try pixelSize(try AttachmentUpload.photoJPEG(try png(width: 4000, height: 1000), name: "big.png"))
        #expect(size.type == UTType.jpeg.identifier)
        #expect(size.width == AttachmentUpload.photoMaxPixels)
        #expect(size.height == 512)
    }

    @Test func givenSmallPhotoShouldKeepItsSize() throws {
        let size = try pixelSize(try AttachmentUpload.photoJPEG(try png(width: 300, height: 200), name: "small.png"))
        #expect(size.type == UTType.jpeg.identifier)
        #expect(size.width == 300 && size.height == 200)
    }

    @Test func givenDataThatIsNotAnImageShouldRejectIt() {
        #expect(throws: AttachmentUpload.UploadError.self) {
            try AttachmentUpload.photoJPEG(Data("not an image".utf8), name: "notes.txt")
        }
    }

    @Test func givenSeveralPhotosShouldNumberTheirNames() {
        let date = Date(timeIntervalSince1970: 0)
        #expect(AttachmentUpload.photoName(at: date).hasSuffix(".jpg"))
        #expect(AttachmentUpload.photoName(at: date, index: 2).hasSuffix(" 2.jpg"))
        #expect(AttachmentUpload.photoName(at: date) != AttachmentUpload.photoName(at: date, index: 1))
    }
}
