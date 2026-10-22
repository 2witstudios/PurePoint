#if os(macOS)
import Foundation
import Security

/// Checks that the app bundle on disk still carries the code signature the
/// running app was launched with, including the resource seal over the bundled
/// Point Guard bridge (JavaScript, node_modules and helpers).
enum BundleSeal {
    static let modifiedMessage = "PurePoint's files were modified after it was installed, so Point Guard won't start. Reinstall PurePoint."

    static let flags = SecCSFlags(rawValue: SecCSFlags.RawValue(kSecCSStrictValidate | kSecCSCheckAllArchitectures | kSecCSCheckNestedCode))

    /// Validates `bundleURL` against `requirement`. Throws when the signature,
    /// the resource seal or any nested code no longer verifies.
    static func verify(_ bundleURL: URL, requirement: SecRequirement?) throws {
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(bundleURL as CFURL, [], &code) == errSecSuccess, let code else {
            throw PiChatError(modifiedMessage)
        }
        let status = SecStaticCodeCheckValidity(code, flags, requirement)
        guard status == errSecSuccess else {
            NSLog("PurePoint bundle seal check failed (OSStatus %d) for %@", status, bundleURL.path)
            throw PiChatError(modifiedMessage)
        }
    }

    /// The designated requirement of the running app, so a bundle that was
    /// modified and then re-signed by someone else also fails.
    static func runningRequirement() throws -> SecRequirement {
        var running: SecCode?
        var staticCode: SecStaticCode?
        var requirement: SecRequirement?
        guard SecCodeCopySelf([], &running) == errSecSuccess, let running,
              SecCodeCopyStaticCode(running, [], &staticCode) == errSecSuccess, let staticCode,
              SecCodeCopyDesignatedRequirement(staticCode, [], &requirement) == errSecSuccess,
              let requirement else {
            throw PiChatError(modifiedMessage)
        }
        return requirement
    }

    /// Checks the main bundle before the bridge starts. Debug builds are signed
    /// for local development and rebuilt in place, so they skip the check.
    static func verifyMainBundle() throws {
        #if DEBUG
        NSLog("PurePoint bundle seal check skipped in Debug build")
        #else
        try verify(Bundle.main.bundleURL, requirement: runningRequirement())
        #endif
    }
}
#endif
