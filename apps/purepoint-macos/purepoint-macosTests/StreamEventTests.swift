import Foundation
import Testing
@testable import PurePoint

struct StreamEventTests {

    @Test func givenAssistantTextEventShouldParseMessageContent() {
        let json = """
            {"type":"assistant","message":{"id":"msg_01","type":"message","role":"assistant","content":[{"type":"text","text":"Hello, world!"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn"}}
            """

        let event = StreamEvent.parse(json)

        guard case .assistant(let blocks) = event else {
            Issue.record("Expected .assistant, got \(String(describing: event))")
            return
        }
        #expect(blocks.count == 1)
        guard case .text(let text) = blocks[0] else {
            Issue.record("Expected .text block")
            return
        }
        #expect(text == "Hello, world!")
    }

    @Test func givenToolUseEventShouldParseNameAndInput() {
        let json = """
            {"type":"assistant","message":{"id":"msg_02","type":"message","role":"assistant","content":[{"type":"text","text":"Let me read that."},{"type":"tool_use","id":"toolu_abc123","name":"Read","input":{"file_path":"/etc/hosts"}}],"model":"claude-sonnet-4-20250514","stop_reason":"tool_use"}}
            """

        let event = StreamEvent.parse(json)

        guard case .assistant(let blocks) = event else {
            Issue.record("Expected .assistant, got \(String(describing: event))")
            return
        }
        #expect(blocks.count == 2)
        guard case .toolUse(let id, let name, _) = blocks[1] else {
            Issue.record("Expected .toolUse block")
            return
        }
        #expect(id == "toolu_abc123")
        #expect(name == "Read")
    }

    @Test func givenToolResultEventShouldParseContentAndErrorFlag() {
        let json = """
            {"type":"tool_result","tool_use_id":"toolu_abc123","content":"127.0.0.1 localhost","is_error":false}
            """

        let event = StreamEvent.parse(json)

        guard case .toolResult(let toolUseId, let content, let isError) = event else {
            Issue.record("Expected .toolResult, got \(String(describing: event))")
            return
        }
        #expect(toolUseId == "toolu_abc123")
        #expect(content == "127.0.0.1 localhost")
        #expect(isError == false)
    }

    @Test func givenResultEventShouldParseSessionIdAndDuration() {
        let json = """
            {"type":"result","subtype":"success","session_id":"sess-abc-123","duration_ms":4567,"is_error":false,"num_turns":3}
            """

        let event = StreamEvent.parse(json)

        guard case .result(let sessionId, let durationMs) = event else {
            Issue.record("Expected .result, got \(String(describing: event))")
            return
        }
        #expect(sessionId == "sess-abc-123")
        #expect(durationMs == 4567)
    }

    @Test func givenUnknownEventTypeShouldReturnUnknown() {
        let json = """
            {"type":"system","message":"some internal event"}
            """

        let event = StreamEvent.parse(json)

        guard case .unknown = event else {
            Issue.record("Expected .unknown, got \(String(describing: event))")
            return
        }
    }

    @Test func givenMalformedJSONShouldReturnNil() {
        let event = StreamEvent.parse("not valid json {{{")
        #expect(event == nil)
    }

    // MARK: - stream_event wrapping tests

    @Test func givenStreamEventWithContentBlockDeltaShouldParseTextDelta() {
        let json = """
            {"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}
            """

        let event = StreamEvent.parse(json)

        guard case .contentBlockDelta(let index, let delta) = event else {
            Issue.record("Expected .contentBlockDelta, got \(String(describing: event))")
            return
        }
        #expect(index == 0)
        #expect(delta == "Hello")
    }

    @Test func givenStreamEventWithToolUseContentBlockStartShouldParse() {
        let json = """
            {"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_xyz789","name":"Write","input":{}}}}
            """

        let event = StreamEvent.parse(json)

        guard case .contentBlockStart(let index, let id, let name) = event else {
            Issue.record("Expected .contentBlockStart, got \(String(describing: event))")
            return
        }
        #expect(index == 1)
        #expect(id == "toolu_xyz789")
        #expect(name == "Write")
    }

    @Test func givenStreamEventWithTextContentBlockStartShouldReturnUnknown() {
        let json = """
            {"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}
            """

        let event = StreamEvent.parse(json)

        guard case .unknown = event else {
            Issue.record("Expected .unknown for text content_block_start, got \(String(describing: event))")
            return
        }
    }

    @Test func givenStreamEventWithUnknownSubtypeShouldReturnUnknown() {
        let json = """
            {"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_01"}}}
            """

        let event = StreamEvent.parse(json)

        guard case .unknown = event else {
            Issue.record("Expected .unknown, got \(String(describing: event))")
            return
        }
    }
}
