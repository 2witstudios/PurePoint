import Testing
import Foundation
@testable import PurePoint

struct HexEncodingTests {

    @Test func testHexRoundTrip() {
        let original = Data("Hello, World!".utf8)
        let hex = original.hexString
        let decoded = Data(hexString: hex)
        #expect(decoded == original)
    }

    @Test func testEmptyData() {
        let empty = Data()
        let hex = empty.hexString
        #expect(hex == "")
        let decoded = Data(hexString: "")
        #expect(decoded.isEmpty)
    }

    @Test func testOddLengthHex() {
        // Odd-length hex string — last nibble is incomplete.
        // "ab" → 0xAB, then "c" alone: UInt8("c", radix: 16) → 12 → 0x0C
        let decoded = Data(hexString: "abc")
        #expect(decoded == Data([0xAB, 0x0C]))
    }

    @Test func testInvalidHexChars() {
        // "zz" is not valid hex — UInt8("zz", radix: 16) returns nil, byte is skipped
        let decoded = Data(hexString: "68zz6f")
        // "68" -> 'h', "zz" -> skipped, "6f" -> 'o'
        #expect(decoded == Data([0x68, 0x6f]))
    }

    @Test func testBinaryData() {
        let binary = Data([0x00, 0xFF, 0x80, 0x01])
        let hex = binary.hexString
        #expect(hex == "00ff8001")
        let decoded = Data(hexString: hex)
        #expect(decoded == binary)
    }
}
