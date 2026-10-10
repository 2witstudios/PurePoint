import Foundation

@MainActor private final class AdminFixture {
    var starts: [String: CheckedContinuation<PiJSONValue, Never>] = [:]
    var enrollment: CheckedContinuation<PiJSONValue, Never>?
    var canceled: [String] = []
    var revoked: [String] = []
    var failReplacement = false
    var failCancel = false
    var failRevoke = false
    func request(_ operation: String, _ fields: [String: PiJSONValue]) async throws -> PiJSONValue {
        switch operation {
        case "auth.start":
            if failReplacement && fields["provider"]?.text == "replacement" { throw PiChatError("Fixture failed before dispatch") }
            return await withCheckedContinuation { starts[fields["provider"]?.text ?? ""] = $0 }
        case "auth.cancel":
            if failCancel { throw PiChatError("Fixture cancellation transport unavailable") }
            canceled.append(fields["attemptId"]?.text ?? ""); return .object([:])
        case "auth.status": return .object(["attemptId": fields["attemptId"] ?? .null, "status": .string("pending")])
        case "pairing.create": return await withCheckedContinuation { enrollment = $0 }
        case "pairing.revoke":
            if failRevoke { throw PiChatError("Fixture revocation transport unavailable") }
            revoked.append(fields["enrollmentId"]?.text ?? ""); return .object([:])
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
        let service = PointGuardServiceModel(requestOverride: { try await fixture.request($0, $1) }, remoteEndpoint: "wss://100.64.0.1:8787/v1")
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
        let established = Task { await service.login(provider: "established", type: "oauth") }
        await wait { fixture.starts["established"] != nil }
        fixture.starts.removeValue(forKey: "established")?.resume(returning: .object(["attemptId": .string("established-id")]))
        await established.value
        await wait { service.auth["attemptId"].text == "established-id" }
        fixture.failReplacement = true
        await service.login(provider: "replacement", type: "oauth")
        precondition(fixture.canceled.contains("established-id"), "A failed replacement must not strand the previous live login")
        // A failed cancellation keeps the ID available for deliberate retry.
        let retry = Task { await service.login(provider: "retry", type: "oauth") }
        await wait { fixture.starts["retry"] != nil }
        fixture.starts.removeValue(forKey: "retry")?.resume(returning: .object(["attemptId": .string("retry-id")]))
        await retry.value
        await wait { service.auth["attemptId"].text == "retry-id" }
        fixture.failCancel = true
        await service.cancelLogin()
        precondition(service.auth["attemptId"].text == "retry-id")
        fixture.failCancel = false
        await service.cancelLogin()
        precondition(fixture.canceled.contains("retry-id"))
        // Given dismissal during creation, revoke the late code and never show its payload.
        let create = Task { await service.connectPhone() }
        await wait { fixture.enrollment != nil }
        await service.closeEnrollment()
        fixture.enrollment?.resume(returning: .object(["enrollmentId": .string("dismissed"), "payload": .string("private-qr")]))
        fixture.enrollment = nil
        await create.value
        precondition(fixture.revoked == ["dismissed"])
        precondition(service.enrollment["payload"].text == nil)
        // Given a late attempt and a failed cleanup transport, retain its ID for explicit retry.
        let lateFailure = Task { await service.login(provider: "late-failure", type: "oauth") }
        await wait { fixture.starts["late-failure"] != nil }
        await service.cancelLogin()
        fixture.failCancel = true
        fixture.starts.removeValue(forKey: "late-failure")?.resume(returning: .object(["attemptId": .string("cleanup-id")]))
        await lateFailure.value
        precondition(service.hasPendingAuthCleanup && service.error != nil)
        fixture.failCancel = false
        await service.retryAuthCleanup()
        precondition(fixture.canceled.contains("cleanup-id") && !service.hasPendingAuthCleanup)
        let lateQrFailure = Task { await service.connectPhone() }
        await wait { fixture.enrollment != nil }
        await service.closeEnrollment()
        fixture.failRevoke = true
        fixture.enrollment?.resume(returning: .object(["enrollmentId": .string("cleanup-qr"), "payload": .string("late-qr")]))
        fixture.enrollment = nil
        await lateQrFailure.value
        precondition(service.hasPendingEnrollmentCleanup && service.enrollment["payload"].text == nil)
        fixture.failRevoke = false
        await service.retryEnrollmentCleanup()
        precondition(fixture.revoked.contains("cleanup-qr") && !service.hasPendingEnrollmentCleanup)
        print("Native auth/enrollment late-response and failed-cleanup retry checks passed")
    }
}
