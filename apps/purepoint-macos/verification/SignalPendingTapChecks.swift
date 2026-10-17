import Foundation
@main enum SignalPendingTapChecks {
    static func main() {
        var taps = AgentSignalPendingTaps()
        let now = Date(timeIntervalSince1970: 100)
        taps.enqueue("event", now: now)
        taps.enqueue("event", now: now)
        precondition(taps.next(availableKeys: [], now: now) == nil)
        precondition(taps.next(availableKeys: ["event"], now: now) == "event")
        precondition(taps.next(availableKeys: ["event"], now: now) == "event", "failed fetch retains intent")
        taps.complete("event")
        precondition(taps.next(availableKeys: ["event"], now: now) == nil)
        taps.enqueue("expired", now: now)
        precondition(taps.next(availableKeys: ["expired"], now: Date(timeIntervalSince1970: 220)) == nil)
        taps.enqueue("future", now: now)
        precondition(taps.next(availableKeys: ["future"], now: Date(timeIntervalSince1970: 99)) == nil)
        for index in 0..<20 { taps.enqueue("key-\(index)", now: now) }
        precondition(taps.next(availableKeys: ["key-0"], now: now) == nil)
        precondition(taps.next(availableKeys: ["key-19"], now: now) == "key-19")
        print("Pending notification tap checks passed: retain until route load/fetch success, completion, expiry, clock rollback and finite capacity")
    }
}
