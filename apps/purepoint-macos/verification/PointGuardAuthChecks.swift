import Foundation

@MainActor private final class AdminFixture {
    var starts: [String: CheckedContinuation<PiJSONValue, Never>] = [:]
    var enrollment: CheckedContinuation<PiJSONValue, Never>?
    var canceled: [String] = []
    var revoked: [String] = []
    var failReplacement = false
    var failCancel = false
    var failRevoke = false
    var failStop = false
    var authStatus: PiJSONValue?
    var heldStatus: CheckedContinuation<PiJSONValue, Never>?
    var holdStatus = false
    var savedProviders: [PiJSONValue] = []
    func request(_ operation: String, _ fields: [String: PiJSONValue]) async throws -> PiJSONValue {
        switch operation {
        case "providers": return .object(["providers": .array(savedProviders)])
        case "runtime.stop":
            if failStop { throw PiChatError("Fixture runtime busy or unavailable") }
            return .object([:])
        case "auth.start":
            if failReplacement && fields["provider"]?.text == "replacement" { throw PiChatError("Fixture failed before dispatch") }
            return await withCheckedContinuation { starts[fields["provider"]?.text ?? ""] = $0 }
        case "auth.cancel":
            if failCancel { throw PiChatError("Fixture cancellation transport unavailable") }
            canceled.append(fields["attemptId"]?.text ?? ""); return .object([:])
        case "auth.status":
            if holdStatus { return await withCheckedContinuation { heldStatus = $0 } }
            if let authStatus { return authStatus }
            return .object(["attemptId": fields["attemptId"] ?? .null, "status": .string("pending")])
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
        // Given confirmed OAuth completion, show provider-specific success and no obsolete interaction.
        let successFixture = AdminFixture()
        successFixture.authStatus = .object(["status": .string("complete"), "events": .array([.object(["url": .string("https://example.com/login")])]), "prompt": .object(["id": .string("obsolete")])])
        let success = PointGuardServiceModel(requestOverride: { try await successFixture.request($0, $1) })
        let login = Task { await success.login(provider: "Example Provider", type: "oauth") }
        await wait { successFixture.starts["Example Provider"] != nil }
        successFixture.starts.removeValue(forKey: "Example Provider")?.resume(returning: .object(["attemptId": .string("success")]))
        await login.value
        await wait { success.auth["status"].text == "complete" }
        precondition(success.authMessage == "Signed in to Example Provider.")
        if case .null = success.auth["events"] {} else { fatalError("Completed login must hide browser events") }
        if case .null = success.auth["prompt"] {} else { fatalError("Completed login must hide prompts") }
        precondition(success.restartRequired)
        await success.cancelLogin() // Setup's selection synchronization must not erase success.
        precondition(success.authMessage == "Signed in to Example Provider.")
        // Given cancellation while status is in flight, a late success must never appear.
        let staleFixture = AdminFixture(); staleFixture.holdStatus = true
        let stale = PointGuardServiceModel(requestOverride: { try await staleFixture.request($0, $1) })
        let staleLogin = Task { await stale.login(provider: "Canceled Provider", type: "oauth") }
        await wait { staleFixture.starts["Canceled Provider"] != nil }
        staleFixture.starts.removeValue(forKey: "Canceled Provider")?.resume(returning: .object(["attemptId": .string("stale")]))
        await staleLogin.value
        await wait { staleFixture.heldStatus != nil }
        await stale.cancelLogin()
        staleFixture.heldStatus?.resume(returning: .object(["status": .string("complete")]))
        staleFixture.heldStatus = nil
        for _ in 0..<10 { await Task.yield() }
        precondition(stale.authMessage == "Sign-in canceled. Try again when you’re ready.")
        precondition(!stale.restartRequired)
        // Given each terminal provider outcome, hide pending instructions without claiming success.
        for (status, expected) in [("failed", "Sign-in failed. Start a new sign-in to try again."),
                                   ("expired", "Sign-in expired. Start a new sign-in to try again."),
                                   ("canceled", "Sign-in canceled. Try again when you’re ready.")] {
            let terminalFixture = AdminFixture()
            terminalFixture.authStatus = .object(["status": .string(status), "events": .array([.object(["userCode": .string("obsolete")])]), "prompt": .object(["id": .string("obsolete")])])
            let terminal = PointGuardServiceModel(requestOverride: { try await terminalFixture.request($0, $1) })
            let task = Task { await terminal.login(provider: "Example", type: "oauth") }
            await wait { terminalFixture.starts["Example"] != nil }
            terminalFixture.starts.removeValue(forKey: "Example")?.resume(returning: .object(["attemptId": .string(status)]))
            await task.value
            await wait { terminal.auth["status"].text == status }
            precondition(terminal.authMessage == expected && !terminal.restartRequired)
            if case .null = terminal.auth["events"] {} else { fatalError("Terminal state retains obsolete instructions") }
            if case .null = terminal.auth["prompt"] {} else { fatalError("Terminal state retains obsolete prompt") }
        }
        // Given reopening setup, saved credential metadata still comes from the durable provider catalog.
        let reopenedFixture = AdminFixture()
        reopenedFixture.savedProviders = [.object(["id": .string("example"), "name": .string("Example Provider"), "configured": .bool(true)])]
        let reopened = PointGuardServiceModel(requestOverride: { try await reopenedFixture.request($0, $1) })
        await reopened.refresh()
        precondition(reopened.providers.first?["id"].text == "example")
        if case .bool(true) = reopened.providers.first?["configured"] {} else { fatalError("Saved credentials must remain visible") }
        precondition(reopened.authMessage == nil, "Saved credentials alone must not claim a new successful OAuth login")
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
        // Cleanup belongs to one owned runtime. Its exit invalidates SDK attempts
        // and memory-only enrollments, including completions arriving after exit.
        let dying = Task { await service.login(provider: "dying", type: "oauth") }
        await wait { fixture.starts["dying"] != nil }
        await service.cancelLogin()
        fixture.failCancel = true
        fixture.starts.removeValue(forKey: "dying")?.resume(returning: .object(["attemptId": .string("dead-id")]))
        await dying.value
        precondition(service.hasPendingAuthCleanup)
        service.handleOwnedTermination(Process(), launchId: "", status: 1)
        precondition(service.hasPendingAuthCleanup, "Late old-child callback must not invalidate current runtime")
        service.ownedRuntimeExited(instanceId: "foreign")
        precondition(service.hasPendingAuthCleanup, "Foreign exit must not discard live cleanup")
        service.ownedRuntimeExited(instanceId: "")
        precondition(!service.hasPendingAuthCleanup)
        let afterCrash = Task { await service.login(provider: "after-crash", type: "oauth") }
        await wait { fixture.starts["after-crash"] != nil }
        service.ownedRuntimeExited(instanceId: "")
        fixture.starts.removeValue(forKey: "after-crash")?.resume(returning: .object(["attemptId": .string("dead-late-id")]))
        await afterCrash.value
        precondition(!service.hasPendingAuthCleanup && service.auth["attemptId"].text == nil)
        fixture.failCancel = false
        let afterStop = Task { await service.login(provider: "after-stop", type: "oauth") }
        await wait { fixture.starts["after-stop"] != nil }
        await service.cancelLogin(); fixture.failCancel = true
        fixture.starts.removeValue(forKey: "after-stop")?.resume(returning: .object(["attemptId": .string("stop-cleanup-id")]))
        await afterStop.value
        precondition(service.hasPendingAuthCleanup)
        try! await service.stop() // No child remains: cleanup is still invalidated.
        precondition(!service.hasPendingAuthCleanup && !service.hasPendingEnrollmentCleanup)
        // A rejected authoritative Stop leaves both the exact child and login alive.
        fixture.failCancel = false
        let liveLogin = Task { await service.login(provider: "live-stop", type: "oauth") }
        await wait { fixture.starts["live-stop"] != nil }
        fixture.starts.removeValue(forKey: "live-stop")?.resume(returning: .object(["attemptId": .string("live-stop-id")]))
        await liveLogin.value
        await wait { service.auth["attemptId"].text == "live-stop-id" }
        let liveQr = Task { await service.connectPhone() }
        await wait { fixture.enrollment != nil }
        fixture.enrollment?.resume(returning: .object(["enrollmentId": .string("live-stop-qr"), "payload": .string("live-qr")]))
        fixture.enrollment = nil
        await liveQr.value
        let child = Process(); child.executableURL = URL(fileURLWithPath: "/bin/sleep"); child.arguments = ["30"]
        try! child.run(); service.fixtureAttach(child)
        fixture.failStop = true
        do { try await service.stop(); fatalError("Stop must reject") } catch { }
        precondition(child.isRunning && service.ready && service.auth["attemptId"].text == "live-stop-id")
        precondition(service.enrollment["enrollmentId"].text == "live-stop-qr")
        await service.closeEnrollment()
        precondition(fixture.revoked.contains("live-stop-qr"), "Rejected Stop must preserve QR revocation")
        await service.cancelLogin()
        precondition(fixture.canceled.contains("live-stop-id"), "Rejected Stop must preserve explicit cancellation")
        try! await service.stopOwnedForQuit()
        precondition(!child.isRunning && !service.ready, "Deliberate quit must await its exact child exit")
        print("Native auth/enrollment late-response and failed-cleanup retry checks passed")
    }
}
