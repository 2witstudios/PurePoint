import Foundation
import Testing
@testable import purepoint_macos

struct ClaudeProcessTests {

    @Test func givenClaudeBinaryAvailableShouldLocatePath() {
        let path = ClaudeProcess.locateBinary()
        // This test verifies binary discovery works — may be nil on CI
        if let path {
            #expect(FileManager.default.isExecutableFile(atPath: path))
        }
    }

    @Test(.disabled("Requires Claude CLI installed — run manually"))
    func givenSimplePromptShouldStreamEventsAndComplete() async throws {
        guard ClaudeProcess.locateBinary() != nil else {
            return
        }

        let process = ClaudeProcess()
        let stream = try await process.start(
            prompt: "Say exactly: hello",
            cwd: "/tmp",
            sessionId: nil
        )

        var gotAssistant = false
        var gotResult = false

        for await event in stream {
            switch event {
            case .assistant:
                gotAssistant = true
            case .result:
                gotResult = true
            default:
                break
            }
        }

        #expect(gotAssistant)
        #expect(gotResult)
    }
}
