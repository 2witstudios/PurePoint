import Foundation

@MainActor private final class AdminFixture {
    var starts: [String: CheckedContinuation<PiJSONValue, Never>] = [:]
    var enrollment: CheckedContinuation<PiJSONValue, Never>?
    var canceled: [String] = []
    var revoked: [String] = []
    func request(_ operation: String, _ fields: [String: PiJSONValue]) async -> PiJSONValue {
        switch operation {
        case "auth.start": return await withCheckedContinuation { starts[fields["provider"]?.text ?? ""] = $0 }
        case "auth.cancel": canceled.append(fields["attemptId"]?.text ?? ""); return .object([:])
        case "auth.status": return .object(["attemptId": fields["attemptId"] ?? .null, "status": .string("pending")])
        case "pairing.create": return await withCheckedContinuation { enrollment = $0 }
        case "pairing.revoke": revoked.append(fields["enrollmentId"]?.text ?? ""); return .object([:])
        default: return .object([:])
        }
    }
}
@main struct PointGuardAuthChecks {
    @MainActor static func wait(_ condition: () -> Bool) async {
        for _ in 0..<1000 { if condition() { return }; try? await Task.sleep(for: .milliseconds(1)) }
        fatalError("fixture timed out")
    }
    @MainActor static func main() async {
        let fixture = AdminFixture()
        let service = PointGuardServiceModel(requestOverride: { await fixture.request($0, $1) }, remoteEndpoint: "wss://100.64.0.1:8787/v1")
        let first = Task { await service.login(provider: "first", type: "oauth") }
        await wait { fixture.starts["first"] != nil }
        await service.cancelLogin()
        fixture.starts.removeValue(forKey: "first")?.resume(returning: .object(["attemptId": .string("late")]))
        await first.value
        precondition(fixture.canceled == ["late"])
        precondition(service.auth["attemptId"].text == nil)
        // Given overlapping starts, the older completion cannot replace the current attempt.
        let old = Task { await service.login(provider: "old", type: "oauth") }
        await wait { fixture.starts["old"] != nil }
        let new = Task { await service.login(provider: "new", type: "oauth") }
        await wait { fixture.starts["new"] != nil }
        fixture.starts.removeValue(forKey: "new")?.resume(returning: .object(["attemptId": .string("current")]))
        await new.value
        await wait { service.auth["attemptId"].text == "current" }
        fixture.starts.removeValue(forKey: "old")?.resume(returning: .object(["attemptId": .string("superseded")]))
        await old.value
        precondition(service.auth["attemptId"].text == "current")
        precondition(fixture.canceled.contains("superseded"))
        await service.cancelLogin()
        // Given dismissal during creation, revoke the late code and never show its payload.
        let create = Task { await service.connectPhone() }
        await wait { fixture.enrollment != nil }
        await service.closeEnrollment()
        fixture.enrollment?.resume(returning: .object(["enrollmentId": .string("dismissed"), "payload": .string("private-qr")]))
        fixture.enrollment = nil
        await create.value
        precondition(fixture.revoked == ["dismissed"])
        precondition(service.enrollment["payload"].text == nil)
        print("Native auth and enrollment cancellation race checks passed")
    }
}
