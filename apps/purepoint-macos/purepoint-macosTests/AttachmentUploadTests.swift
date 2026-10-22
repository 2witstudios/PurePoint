import AppKit
import Foundation
import SwiftTerm
import Testing

@testable import PurePoint

struct AttachmentUploadTests {
    private func json(_ request: DaemonRequest) throws -> [String: Any] {
        let data = try JSONEncoder().encode(request)
        return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func givenAttachFileRequestShouldEncodeChunkAsBase64() throws {
        let object = try json(
            .attachFile(uploadId: "u1", name: "shot.png", offset: 4, data: Data("hi".utf8), done: true))
        #expect(object["type"] as? String == "attach_file")
        #expect(object["upload_id"] as? String == "u1")
        #expect(object["name"] as? String == "shot.png")
        #expect(object["offset"] as? Int == 4)
        #expect(object["data_base64"] as? String == "aGk=")
        #expect(object["done"] as? Bool == true)
    }

    @Test func givenAttachFileResultShouldDecodePathOnlyWhenFinished() {
        guard case .attachFileResult(6, nil) = DaemonClient.parse(Data(#"{"type":"attach_file_result","received":6}"#.utf8))
        else {
            Issue.record("expected an unfinished upload")
            return
        }
        let done = DaemonClient.parse(
            Data(#"{"type":"attach_file_result","received":6,"path":"/h/.pu/attachments/u1/a.png"}"#.utf8))
        guard case .attachFileResult(6, "/h/.pu/attachments/u1/a.png") = done else {
            Issue.record("expected a finished upload, got \(done)")
            return
        }
    }

    @Test func givenDataLargerThanOneChunkShouldSplitInOrderAndFinishOnLast() throws {
        let data = Data(repeating: 7, count: AttachmentUpload.chunkSize * 2 + 10)
        let objects = try AttachmentUpload.requests(for: data, name: "a.bin", uploadId: "u1").map(json)
        #expect(objects.map { $0["offset"] as? Int } == [0, AttachmentUpload.chunkSize, AttachmentUpload.chunkSize * 2])
        #expect(objects.map { $0["done"] as? Bool } == [false, false, true])
        let sizes = objects.compactMap { ($0["data_base64"] as? String).flatMap { Data(base64Encoded: $0)?.count } }
        #expect(sizes == [AttachmentUpload.chunkSize, AttachmentUpload.chunkSize, 10])
    }

    @Test func givenEmptyFileShouldSendOneFinishedChunk() throws {
        let objects = try AttachmentUpload.requests(for: Data(), name: "empty", uploadId: "u1").map(json)
        #expect(objects.count == 1)
        #expect(objects.first?["done"] as? Bool == true)
    }

    @Test func givenCommandVWithCapsLockShouldStillBeAPaste() {
        #expect(AttachmentUpload.isCommandOnly(.command))
        #expect(AttachmentUpload.isCommandOnly([.command, .capsLock]))
        #expect(AttachmentUpload.isCommandOnly([.command, .function]))
    }

    @Test func givenCommandWithAnotherShortcutModifierShouldNotBeAPaste() {
        #expect(!AttachmentUpload.isCommandOnly([.command, .shift]))
        #expect(!AttachmentUpload.isCommandOnly([.command, .option]))
        #expect(!AttachmentUpload.isCommandOnly([.command, .control]))
        #expect(!AttachmentUpload.isCommandOnly([.capsLock]))
    }

    private func temporaryDirectory() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    @Test func givenDroppedRegularFileShouldReadItsBytes() throws {
        let dir = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("a.txt")
        try Data("hello".utf8).write(to: file)
        #expect(try AttachmentUpload.readFile(at: file) == Data("hello".utf8))
    }

    @Test func givenDroppedFolderShouldRejectIt() throws {
        let dir = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        #expect(throws: AttachmentUpload.UploadError.self) { try AttachmentUpload.readFile(at: dir) }
    }

    @Test func givenDroppedFileOverTheLimitShouldRejectItWithoutReading() throws {
        let dir = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("big.bin")
        #expect(FileManager.default.createFile(atPath: file.path, contents: nil))
        // Sparse: extends the size without writing the bytes.
        let handle = try FileHandle(forWritingTo: file)
        try handle.truncate(atOffset: UInt64(AttachmentUpload.maxBytes + 1))
        try handle.close()
        let error = #expect(throws: AttachmentUpload.UploadError.self) { try AttachmentUpload.readFile(at: file) }
        guard case .tooLarge = error else {
            Issue.record("expected tooLarge, got \(String(describing: error))")
            return
        }
    }

    @Test func givenMissingFileShouldThrow() throws {
        let dir = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        #expect(throws: (any Error).self) { try AttachmentUpload.readFile(at: dir.appendingPathComponent("gone")) }
    }

    @Test @MainActor func givenNoAttachConnectionShouldReportInputUndelivered() async {
        let session = DaemonAttachSession(agentId: "ag-1", terminalView: TerminalView(frame: .zero))
        #expect(await session.deliverInput(Data("x".utf8), timeout: .milliseconds(150)) == false)
        await session.stop()
        #expect(await session.deliverInput(Data("x".utf8)) == false)
    }
}
