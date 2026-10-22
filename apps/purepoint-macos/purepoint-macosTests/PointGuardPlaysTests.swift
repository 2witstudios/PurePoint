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

    @Test func assignedSlotsKeepTheirTitlesUntilPromptsLoad() {
        let plays = PointGuardPlay.resolve([PointGuardPlayAssignment(slot: 1, name: "review")], prompts: nil)
        #expect(plays[1].title == "review")
        #expect(plays[1].source == .saved(name: "review"))
        #expect(plays[1].prompt.isEmpty)
        #expect(plays[0] == PointGuardPlay.builtIns[0])
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

    @Test func wingsDropAsSoonAsTheColumnReachesTheirLabels() {
        let size = CGSize(width: 1200, height: 800)
        let positions = PointGuardCourtGeometry.positions(in: size, columnWidth: 720, composerBottom: 560)
        #expect(positions[0].y == 608)
        #expect(positions[1].y == 608)
    }

    @Test func lowBlocksStayBelowTheComposerInAShortPane() {
        let size = CGSize(width: 1000, height: 550)
        let positions = PointGuardCourtGeometry.positions(in: size, columnWidth: 720, composerBottom: 446)
        #expect(positions.allSatisfy { $0.y == 494 })
    }

    @Test func playsNeverLeaveTheBottomOfThePane() {
        let size = CGSize(width: 1000, height: 550)
        let positions = PointGuardCourtGeometry.positions(in: size, columnWidth: 720, composerBottom: 500)
        #expect(positions.allSatisfy { $0.y == 510 })
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

    // MARK: - Docked (iPad) court

    @Test func aTallPaneRunsThePortraitCourtOnlyUnderADockedComposer() {
        let portrait = CGSize(width: 500, height: 900)
        #expect(PointGuardCourtGeometry.layout(for: portrait, compact: false, docked: true) == .portrait)
        #expect(PointGuardCourtGeometry.layout(for: portrait, compact: false, docked: false) == .stacked)
    }

    @Test func aWidePaneKeepsTheWideCourt() {
        let landscape = CGSize(width: 900, height: 700)
        #expect(PointGuardCourtGeometry.layout(for: landscape, compact: false, docked: true) == .wide)
        #expect(PointGuardCourtGeometry.layout(for: landscape, compact: false, docked: false) == .wide)
    }

    @Test func smallAndCompactPanesStack() {
        #expect(
            PointGuardCourtGeometry.layout(for: CGSize(width: 320, height: 460), compact: false, docked: true)
                == .stacked)
        #expect(
            PointGuardCourtGeometry.layout(for: CGSize(width: 500, height: 900), compact: true, docked: true)
                == .stacked)
    }

    @Test func portraitPlaysSitOnTheirAnchorsUnderTheHeading() {
        let positions = PointGuardCourtGeometry.portraitPositions(
            in: CGSize(width: 500, height: 900), headingBottom: 300)
        #expect(
            positions == [
                CGPoint(x: 105, y: 603), CGPoint(x: 395, y: 603), CGPoint(x: 165, y: 774), CGPoint(x: 335, y: 774),
            ])
    }

    @Test func portraitPlaysStayInsideAShortNarrowPane() {
        let size = CGSize(width: 360, height: 520)
        let positions = PointGuardCourtGeometry.portraitPositions(in: size, headingBottom: 340)
        #expect(positions[0] == CGPoint(x: 78, y: 388))
        #expect(positions[1] == CGPoint(x: 282, y: 388))
        #expect(positions[2].y == 480)
        #expect(positions.allSatisfy { $0.x - PointGuardCourtGeometry.labelHalfWidth >= 0 })
        #expect(positions.allSatisfy { $0.x + PointGuardCourtGeometry.labelHalfWidth <= size.width })
    }
}
