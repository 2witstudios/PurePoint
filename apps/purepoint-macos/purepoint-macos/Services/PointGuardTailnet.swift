import Darwin
import Foundation

enum PointGuardTailnet {
    static func candidate(interface: String, isUp: Bool, address: String) -> Bool {
        guard isUp, interface.hasPrefix("utun"), !interface.dropFirst(4).isEmpty,
            interface.dropFirst(4).allSatisfy({ $0.isASCII && $0.isNumber }) else { return false }
        let parts = address.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4, parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy({ $0.isASCII && $0.isNumber }) }),
            let second = Int(parts[1]), parts[0] == "100", (64...127).contains(second),
            parts.allSatisfy({ Int($0).map { (0...255).contains($0) } ?? false }) else { return false }
        return true
    }
    static func matchedAddress(candidates: Set<String>, reported: String) -> String? {
        let values = reported.split(whereSeparator: { $0.isWhitespace }).map(String.init)
        guard values.count == 1, candidates.contains(values[0]) else { return nil }
        return values[0]
    }
    /// Confirm an installed Tailscale client agrees with a live tunnel address.
    /// Caller runs this bounded, read-only discovery off the UI actor.
    static func address() -> String? {
        var interfaces: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&interfaces) == 0 else { return nil }
        defer { freeifaddrs(interfaces) }
        var candidates = Set<String>()
        var cursor = interfaces
        while let current = cursor {
            defer { cursor = current.pointee.ifa_next }
            guard let address = current.pointee.ifa_addr, let name = current.pointee.ifa_name,
                address.pointee.sa_family == UInt8(AF_INET) else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(address, socklen_t(address.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0,
                let value = String(bytes: host.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, encoding: .utf8) else { continue }
            if candidate(interface: String(cString: name), isUp: current.pointee.ifa_flags & UInt32(IFF_UP) != 0, address: value) { candidates.insert(value) }
        }
        guard !candidates.isEmpty else { return nil }
        let home = FileManager.default.homeDirectoryForCurrentUser
        let paths = ["/Applications/Tailscale.app/Contents/MacOS/Tailscale",
                     home.appendingPathComponent("Applications/Tailscale.app/Contents/MacOS/Tailscale").path,
                     "/opt/homebrew/bin/tailscale", "/usr/local/bin/tailscale"]
        guard let executable = paths.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else { return nil }
        let child = Process(); let output = Pipe()
        child.executableURL = URL(fileURLWithPath: executable); child.arguments = ["ip", "-4"]
        // GUI variants choose CLI mode from terminal environment; no shell or PATH lookup.
        child.environment = ["HOME": home.path, "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "SHLVL": "1", "TERM": "dumb"]
        child.standardOutput = output; child.standardError = FileHandle.nullDevice; child.standardInput = FileHandle.nullDevice
        do { try child.run() } catch { return nil }
        let deadline = Date().addingTimeInterval(3)
        while child.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.01) }
        if child.isRunning { child.terminate(); return nil }
        guard child.terminationStatus == 0 else { return nil }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        guard data.count <= 4096, let reported = String(data: data, encoding: .utf8) else { return nil }
        return matchedAddress(candidates: candidates, reported: reported)
    }
}
