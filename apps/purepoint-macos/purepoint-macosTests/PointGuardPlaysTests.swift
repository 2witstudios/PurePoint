import CoreGraphics
import Foundation
import Testing
@testable import PurePoint

@MainActor
struct PointGuardPlaysTests {

    private let review = PointGuardPrompt(name: "review", body: "Review my open PRs.", needsInput: false)
    private let branch = PointGuardPrompt(name: "branch", body: "Review {{BRANCH}}.", needsInput: true)

    // MARK: - Resolving

    @Test func unassignedSlotsPlayTheirBuiltIns() {
        let plays = PointGuardPlay.resolve([], prompts: [review])
        #expect(plays == PointGuardPlay.builtIns)
        #expect(plays.allSatisfy { !$0.prompt.isEmpty && !$0.prompt.contains("[") })
    }

    @Test func assignedSlotPlaysItsSavedPrompt() {
        let plays = PointGuardPlay.resolve([PointGuardPlayAssignment(slot: 2, name: "review")], prompts: [review])
        #expect(plays[2].title == "review")
        #expect(plays[2].prompt == "Review my open PRs.")
        #expect(plays[2].source == .saved(name: "review"))
        #expect(plays[2].isCustom)
        #expect(plays[0] == PointGuardPlay.builtIns[0])
    }

    @Test func deletedOrInputNeedingPromptFallsBackToBuiltIn() {
        let assignments = [
            PointGuardPlayAssignment(slot: 0, name: "gone"), PointGuardPlayAssignment(slot: 1, name: "branch"),
        ]
        let plays = PointGuardPlay.resolve(assignments, prompts: [review, branch])
        #expect(plays[0] == PointGuardPlay.builtIns[0])
        #expect(plays[1] == PointGuardPlay.builtIns[1])
    }

    // MARK: - Persisting

    @Test func assignmentsPersistAcrossPlaybooks() throws {
        let suite = "PointGuardPlaysTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let playbook = PointGuardPlaybook(defaults: defaults)
        playbook.assign(slot: 3, prompt: review)
        playbook.assign(slot: 3, prompt: PointGuardPrompt(name: "standup", body: "Standup notes.", needsInput: false))
        #expect(
            PointGuardPlaybook(defaults: defaults).assignments == [PointGuardPlayAssignment(slot: 3, name: "standup")])

        playbook.reset(slot: 3)
        #expect(PointGuardPlaybook(defaults: defaults).assignments.isEmpty)
    }

    @Test func promptsNeedingInputAndUnknownSlotsAreNeverAssigned() throws {
        let suite = "PointGuardPlaysTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let playbook = PointGuardPlaybook(defaults: defaults)
        playbook.assign(slot: 0, prompt: branch)
        playbook.assign(slot: 4, prompt: review)
        #expect(playbook.assignments.isEmpty)
    }

    // MARK: - Court geometry

    @Test func wingsStayBesideAWideComposer() {
        let size = CGSize(width: 1440, height: 800)
        let positions = PointGuardCourtGeometry.positions(in: size, columnWidth: 720, composerBottom: 560)
        #expect(positions[0] == CGPoint(x: 1440 * 0.16, y: 800 * 0.61))
        #expect(positions[1].y == positions[0].y)
    }

    @Test func wingsDropBelowANarrowComposer() {
        let size = CGSize(width: 900, height: 800)
        let positions = PointGuardCourtGeometry.positions(in: size, columnWidth: 720, composerBottom: 560)
        #expect(positions[0].y == 608)
        #expect(positions[1].y == 608)
        #expect(positions[2].y == 800 * 0.84)
    }

    @Test func onlyAPlayWithinReachLightsUp() {
        let positions = [CGPoint(x: 100, y: 100), CGPoint(x: 400, y: 100)]
        #expect(PointGuardCourtGeometry.nearestSlot(to: CGPoint(x: 380, y: 120), among: positions) == 1)
        #expect(PointGuardCourtGeometry.nearestSlot(to: CGPoint(x: 250, y: 400), among: positions) == nil)
    }

    @Test func logoTiltIsClamped() {
        let size = CGSize(width: 1000, height: 800)
        #expect(PointGuardCourtGeometry.tilt(toward: CGPoint(x: 500, y: 0), in: size) == 0)
        #expect(PointGuardCourtGeometry.tilt(toward: CGPoint(x: 1000, y: 0), in: size) == 8)
        #expect(PointGuardCourtGeometry.tilt(toward: CGPoint(x: 0, y: 0), in: size) == -8)
    }
}
