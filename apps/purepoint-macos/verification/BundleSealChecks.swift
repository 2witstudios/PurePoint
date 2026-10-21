import Foundation
import Security

// Standalone checks for BundleSeal against throwaway ad-hoc signed bundles.
@main struct BundleSealChecks {
    static func codesign(_ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        try process.run(); process.waitUntilExit()
        precondition(process.terminationStatus == 0, "codesign \(arguments) failed")
    }

    static func makeBundle(in folder: URL) throws -> URL {
        let bundle = folder.appendingPathComponent("Seal.app", isDirectory: true)
        let contents = bundle.appendingPathComponent("Contents")
        let bridge = contents.appendingPathComponent("Resources/PointGuard/bridge", isDirectory: true)
        try FileManager.default.createDirectory(at: contents.appendingPathComponent("MacOS"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: bridge, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: URL(fileURLWithPath: "/usr/bin/true"), to: contents.appendingPathComponent("MacOS/Seal"))
        try """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0"><dict>
        <key>CFBundleExecutable</key><string>Seal</string>
        <key>CFBundleIdentifier</key><string>dev.purepoint.seal-check</string>
        <key>CFBundlePackageType</key><string>APPL</string>
        </dict></plist>
        """.write(to: contents.appendingPathComponent("Info.plist"), atomically: true, encoding: .utf8)
        try "export const ok = true;\n".write(to: bridge.appendingPathComponent("network.js"), atomically: true, encoding: .utf8)
        try codesign(["--force", "--sign", "-", bundle.path])
        return bundle
    }

    static func designatedRequirement(_ bundle: URL) -> SecRequirement {
        var code: SecStaticCode?
        var requirement: SecRequirement?
        precondition(SecStaticCodeCreateWithPath(bundle as CFURL, [], &code) == errSecSuccess)
        precondition(SecCodeCopyDesignatedRequirement(code!, [], &requirement) == errSecSuccess)
        return requirement!
    }

    static func rejects(_ bundle: URL, _ requirement: SecRequirement?) -> Bool {
        do { try BundleSeal.verify(bundle, requirement: requirement); return false }
        catch { return error.localizedDescription == BundleSeal.modifiedMessage }
    }

    static func main() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("bundle-seal-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        let bundle = try makeBundle(in: folder)
        let shipped = designatedRequirement(bundle)
        try BundleSeal.verify(bundle, requirement: shipped)
        print("ok: intact bundle verifies against its own requirement")

        let script = bundle.appendingPathComponent("Contents/Resources/PointGuard/bridge/network.js")
        try "export const ok = false;\n".write(to: script, atomically: true, encoding: .utf8)
        precondition(rejects(bundle, shipped), "modified bridge script must fail the resource seal")
        precondition(rejects(bundle, nil), "modified bridge script must fail even without a requirement")
        print("ok: modified bridge script is rejected")

        try "export const extra = 1;\n".write(to: script.deletingLastPathComponent().appendingPathComponent("extra.js"), atomically: true, encoding: .utf8)
        try codesign(["--force", "--sign", "-", bundle.path])
        try BundleSeal.verify(bundle, requirement: nil)
        precondition(rejects(bundle, shipped), "a re-signed bundle must not satisfy the original requirement")
        print("ok: re-signed bundle fails the running app's requirement")

        precondition(rejects(folder.appendingPathComponent("Missing.app"), nil), "a missing bundle must be rejected")
        print("ok: missing bundle is rejected")
    }
}
