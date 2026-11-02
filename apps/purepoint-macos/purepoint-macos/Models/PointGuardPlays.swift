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
    /// How close a ball dragged off the logo must be to a play to pass to it.
    static let touchReach: CGFloat = 72
    /// Half a play's label width, which must stay clear of the centred column.
    static let labelHalfWidth: CGFloat = 70

    static func logoPoint(in size: CGSize, portrait: Bool = false) -> CGPoint {
        let logo = portrait ? portraitLogo : logo
        return CGPoint(x: size.width * logo.x, y: size.height * logo.y)
    }

    /// Below this size the court can't hold the column and its plays, so they stack instead. The height leaves room
    /// for the low blocks under a composer with a two-line heading above it.
    static let minimumSize = CGSize(width: 680, height: 600)

    enum Layout: Equatable { case wide, portrait, stacked }

    /// The iPad docks the composer under the court; the Mac keeps it in the court's centred column.
    #if os(iOS)
        static let docksComposer = true
    #else
        static let docksComposer = false
    #endif

    /// The court for a pane of `size`. The iPad always runs a court, whatever the pane: a tall or narrow pane runs the
    /// portrait court, any other the wide one. Its composer docks under the court, so `size` is the court's own space
    /// without the keyboard and the composer's slot is never part of this choice. The Mac stacks a pane too small for
    /// the wide court, or a compact (sidebar) one.
    static func layout(for size: CGSize, compact: Bool, docked: Bool) -> Layout {
        if docked { return size.height > size.width || size.width < minimumSize.width ? .portrait : .wide }
        if compact { return .stacked }
        return size.width >= minimumSize.width && size.height >= minimumSize.height ? .wide : .stacked
    }

    /// The portrait court: the logo at the top of the key, wings outside the arc, low blocks inside it.
    static let portraitLogo = CGPoint(x: 0.5, y: 0.28)
    static let portraitAnchors = [
        CGPoint(x: 0.21, y: 0.67), CGPoint(x: 0.79, y: 0.67),
        CGPoint(x: 0.33, y: 0.86), CGPoint(x: 0.67, y: 0.86),
    ]

    /// Every play stays under the logo and heading, inside the pane, and its label clear of the side lines.
    static func portraitPositions(in size: CGSize, headingBottom: CGFloat) -> [CGPoint] {
        let belowHeading = headingBottom + 48
        let inset = labelHalfWidth + 8
        return portraitAnchors.enumerated().map { index, anchor in
            // The blocks keep a row's gap under the wings, as far as the pane allows.
            let floor = index < 2 ? belowHeading : belowHeading + 96
            let y = min(max(size.height * anchor.y, floor), size.height - 40)
            var x = size.width * anchor.x
            if index >= 2 {
                // The low blocks sit side by side, so a narrow pane spreads them a play's width apart.
                let spread = max(abs(x - size.width / 2), labelHalfWidth + 4)
                x = size.width / 2 + (anchor.x < 0.5 ? -spread : spread)
            }
            return CGPoint(x: min(max(x, inset), size.width - inset), y: y)
        }
    }

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

    static func nearestSlot(to point: CGPoint, among positions: [CGPoint], reach: CGFloat = reach) -> Int? {
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

    /// How far either side of the logo a target must be before the logo turns to face it.
    static let facingDeadZone: CGFloat = 24

    /// The logo is drawn facing right. It turns to face a target clearly to one side, and otherwise keeps its facing.
    static func facesLeft(toward target: CGPoint?, logoX: CGFloat, current: Bool) -> Bool {
        guard let target, abs(target.x - logoX) > facingDeadZone else { return current }
        return target.x < logoX
    }
}
