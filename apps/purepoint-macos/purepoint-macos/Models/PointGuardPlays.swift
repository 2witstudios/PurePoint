import CoreGraphics
import Foundation
import Observation

/// A global `pu` prompt the court can offer. Prompts with `{{VAR}}` placeholders need input
/// before they can be sent, so a play can't hold them.
struct PointGuardPrompt: Identifiable, Equatable, Sendable {
    var id: String { name }
    let name: String
    var body: String
    let needsInput: Bool
}

/// One of the four positions on the Point Guard court. Clicking it sends `prompt` as-is.
struct PointGuardPlay: Identifiable, Equatable, Sendable {
    enum Source: Equatable, Sendable {
        case builtIn
        case saved(name: String)
    }

    var id: Int { slot }
    let slot: Int
    let title: String
    /// Empty for a saved prompt whose body hasn't been fetched yet.
    let prompt: String
    let symbol: String
    let source: Source

    var isCustom: Bool { source != .builtIn }

    /// Slot order: left wing, right wing, left block, right block.
    static let builtIns: [PointGuardPlay] = [
        PointGuardPlay(
            slot: 0, title: "Catch me up",
            prompt: "Give me an update on all my projects — what moved, what's blocked, and what needs me next.",
            symbol: "square.stack.3d.up", source: .builtIn),
        PointGuardPlay(
            slot: 1, title: "What's next?",
            prompt: "Look across my projects and tell me what I should work on next, and why.",
            symbol: "safari", source: .builtIn),
        PointGuardPlay(
            slot: 2, title: "Tidy my desktop",
            prompt:
                "Organize my desktop — group the files into folders that make sense and clear the clutter. Show me the plan before you move anything.",
            symbol: "square.grid.2x2", source: .builtIn),
        PointGuardPlay(
            slot: 3, title: "What's running?",
            prompt:
                "What's running on my machine right now? Agents, apps, and anything using a lot of memory or CPU.",
            symbol: "waveform.path.ecg", source: .builtIn),
    ]

    /// Fills each slot from its assigned prompt. A slot whose prompt was deleted, or now needs
    /// input, quietly plays its built-in again. `prompts` is nil until the global list has loaded:
    /// assigned slots keep their saved titles meanwhile, and their bodies are fetched when played.
    static func resolve(_ assignments: [PointGuardPlayAssignment], prompts: [PointGuardPrompt]?) -> [PointGuardPlay] {
        builtIns.map { builtIn in
            guard let assignment = assignments.last(where: { $0.slot == builtIn.slot }) else { return builtIn }
            guard let prompts else { return saved(slot: builtIn.slot, name: assignment.name, body: "") }
            guard let prompt = prompts.first(where: { $0.name == assignment.name }), !prompt.needsInput else {
                return builtIn
            }
            return saved(slot: builtIn.slot, name: prompt.name, body: prompt.body)
        }
    }

    private static func saved(slot: Int, name: String, body: String) -> PointGuardPlay {
        PointGuardPlay(slot: slot, title: name, prompt: body, symbol: "text.bubble", source: .saved(name: name))
    }
}

struct PointGuardPlayAssignment: Codable, Equatable, Sendable {
    let slot: Int
    let name: String
}

/// Which saved prompt each court slot plays. Stored per device (APP-018).
@MainActor
@Observable
final class PointGuardPlaybook {
    static let defaultsKey = "PP_pointGuardPlays"
    static let shared = PointGuardPlaybook()

    private(set) var assignments: [PointGuardPlayAssignment]
    /// The global prompts on offer; nil until they have loaded.
    var prompts: [PointGuardPrompt]?
    var plays: [PointGuardPlay] { PointGuardPlay.resolve(assignments, prompts: prompts) }

    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let data = defaults.data(forKey: Self.defaultsKey) ?? Data()
        assignments = (try? JSONDecoder().decode([PointGuardPlayAssignment].self, from: data)) ?? []
    }

    func assign(slot: Int, prompt: PointGuardPrompt) {
        guard PointGuardPlay.builtIns.indices.contains(slot), !prompt.needsInput else { return }
        assignments = assignments.filter { $0.slot != slot } + [PointGuardPlayAssignment(slot: slot, name: prompt.name)]
        save()
    }

    func reset(slot: Int) {
        assignments.removeAll { $0.slot == slot }
        save()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(assignments.sorted { $0.slot < $1.slot }) {
            defaults.set(data, forKey: Self.defaultsKey)
        }
    }
}

/// Where things stand on the court, in the court view's own points.
enum PointGuardCourtGeometry {
    /// The logo is the point guard, at the top of the key.
    static let logo = CGPoint(x: 0.5, y: 0.30)
    /// Wings beside the composer, then the low blocks either side of the lane.
    static let anchors = [
        CGPoint(x: 0.16, y: 0.61), CGPoint(x: 0.84, y: 0.61),
        CGPoint(x: 0.38, y: 0.84), CGPoint(x: 0.62, y: 0.84),
    ]
    /// How close the pointer must be for a play to light up.
    static let reach: CGFloat = 150
    /// Half a play's label width, which must stay clear of the centred column.
    static let labelHalfWidth: CGFloat = 70

    static func logoPoint(in size: CGSize) -> CGPoint {
        CGPoint(x: size.width * logo.x, y: size.height * logo.y)
    }

    /// Below this width the court can't hold the column and its plays, so they stack instead.
    static let minimumWidth: CGFloat = 680

    /// No play may sit on the composer. Wings beside it drop below it once the centred column would reach their
    /// labels, and the low blocks always stay below it, as far as the pane's height allows.
    static func positions(in size: CGSize, columnWidth: CGFloat, composerBottom: CGFloat) -> [CGPoint] {
        let columnLeft = (size.width - min(columnWidth, size.width - 48)) / 2
        let wingsCrowded = size.width * anchors[0].x + labelHalfWidth > columnLeft - 16
        // A play is centred on its icon-and-label stack, about 70pt tall.
        let belowComposer = composerBottom + 48
        return anchors.enumerated().map { index, anchor in
            let y = size.height * anchor.y
            let clear = index < 2 ? (wingsCrowded ? max(y, belowComposer) : y) : max(y, belowComposer)
            return CGPoint(x: size.width * anchor.x, y: min(clear, size.height - 40))
        }
    }

    static func nearestSlot(to point: CGPoint, among positions: [CGPoint]) -> Int? {
        positions.indices
            .map { ($0, hypot(positions[$0].x - point.x, positions[$0].y - point.y)) }
            .filter { $0.1 < reach }
            .min { $0.1 < $1.1 }?.0
    }

    /// The logo leans toward the pointer, never more than 8°.
    static func tilt(toward point: CGPoint, in size: CGSize) -> Double {
        guard size.width > 0 else { return 0 }
        return max(-8, min(8, (Double(point.x / size.width) * 100 - 50) * 0.18))
    }
}
