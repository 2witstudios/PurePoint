import Darwin
import Foundation

enum PointGuardTailnet {
    /// Find an explicit live tailnet interface; never use a wildcard/public bind.
    static func address() -> String? {
        var interfaces: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&interfaces) == 0 else { return nil }
        defer { freeifaddrs(interfaces) }
        var cursor = interfaces
        while let current = cursor {
            defer { cursor = current.pointee.ifa_next }
            guard let address = current.pointee.ifa_addr,
                current.pointee.ifa_flags & UInt32(IFF_UP) != 0,
                address.pointee.sa_family == UInt8(AF_INET) else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(address, socklen_t(address.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { continue }
            let bytes = host.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
            let value = String(decoding: bytes, as: UTF8.self)
            let parts = value.split(separator: ".").compactMap { Int($0) }
            if parts.count == 4, parts[0] == 100, (64...127).contains(parts[1]) { return value }
        }
        return nil
    }
}
