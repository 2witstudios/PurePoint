import SwiftUI

/// The court's own coordinate space, shared by hover tracking and the column's measured edge.
private let courtSpace = "pointGuardCourt"

/// Point Guard's new-conversation screen: a half-court with the logo at the top of the key,
/// the composer under it, and four plays that each send a whole prompt when passed to. On the iPad the composer
/// docks under the court instead, a tall pane runs a portrait court, and a thumb on the logo drags a ball to a play.
struct PointGuardCourtView<Composer: View>: View {
    let plays: [PointGuardPlay]
    let prompts: [PointGuardPrompt]
    /// False while disconnected or mid-session; plays are shown but can't be run.
    let canRun: Bool
    let connected: Bool
    var compact = false
    @ViewBuilder let composer: () -> Composer
    let onRun: (PointGuardPlay) -> Void
    let onAssign: (Int, PointGuardPrompt) -> Void
    let onReset: (Int) -> Void
    let onOpenSettings: () -> Void

    @State private var hover: CGPoint?
    @State private var passing: Int?
    @State private var passTask: Task<Void, Never>?
    /// The bottom of the logo, heading and (on the Mac) composer, which no play may sit on.
    @State private var columnBottom: CGFloat = 0
    @State private var facesLeft = false
    /// The thumb on the logo (iPad); resets on its own when the system cancels the drag, so the ball can't get stuck.
    @GestureState private var grab: CGPoint?
    /// The ball flying to a play after release.
    @State private var flight: CGPoint?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private static var accent: Color { Color(nsColor: Theme.pointGuardAccent) }

    /// The pane's height without the keyboard, and the docked composer's (iPad only).
    @State private var restingHeight: CGFloat?
    @State private var slotHeight: CGFloat = 0

    var body: some View {
        if PointGuardCourtGeometry.docksComposer {
            // One composer under every layout. The keyboard, a rotation or a split resize relays out the court above it
            // without rebuilding the text field, which would drop its focus (and the keyboard) the moment it was tapped.
            // The court keeps its height without the keyboard, as on the phone: the keyboard and composer slide up over
            // its basket end rather than squeezing it into another layout.
            VStack(spacing: 0) {
                Color.clear
                    .overlay(alignment: .top) {
                        layouts.frame(height: restingHeight.map { max(0, $0 - slotHeight) })
                    }
                    .clipped()
                slot.background {
                    GeometryReader { proxy in
                        Color.clear
                            .onAppear { slotHeight = proxy.size.height }
                            .onChange(of: proxy.size.height) { _, height in slotHeight = height }
                    }
                }
            }
            .background {
                GeometryReader { proxy in
                    Color.clear
                        .onAppear { restingHeight = proxy.size.height }
                        .onChange(of: proxy.size.height) { _, height in restingHeight = height }
                }
                .ignoresSafeArea(.keyboard)
            }
        } else {
            layouts
        }
    }

    private var layouts: some View {
        GeometryReader { proxy in
            switch PointGuardCourtGeometry.layout(
                for: proxy.size, compact: compact, docked: PointGuardCourtGeometry.docksComposer)
            {
            case .wide: court(proxy.size, portrait: false)
            case .portrait: court(proxy.size, portrait: true)
            case .stacked: stacked(height: proxy.size.height)
            }
        }
        .onDisappear { passTask?.cancel(); passTask = nil; passing = nil; flight = nil }
    }

    // MARK: - Court

    private func court(_ size: CGSize, portrait: Bool) -> some View {
        let positions =
            portrait
            ? PointGuardCourtGeometry.portraitPositions(in: size, headingBottom: columnBottom)
            : PointGuardCourtGeometry.positions(
                in: size, columnWidth: PurePointTheme.chatContentWidth, composerBottom: columnBottom)
        let pointer = reduceMotion ? nil : (grab ?? hover)
        let drifted = positions.map { point in
            guard let pointer else { return point }
            return CGPoint(x: point.x + (pointer.x - point.x) * 0.05, y: point.y + (pointer.y - point.y) * 0.05)
        }
        // A dragged ball must come close to a play; the Mac's pointer lights one from further off.
        let near =
            grab.flatMap {
                PointGuardCourtGeometry.nearestSlot(to: $0, among: drifted, reach: PointGuardCourtGeometry.touchReach)
            } ?? pointer.flatMap { PointGuardCourtGeometry.nearestSlot(to: $0, among: drifted) }
        let lit = passing ?? near
        let logo = PointGuardCourtGeometry.logoPoint(in: size, portrait: portrait)
        let lines = portrait ? AnyShape(PortraitCourtLines(blocks: positions[2].y)) : AnyShape(CourtLines())
        // The logo faces the lit play, else the ball or pointer. A turn is a discrete change, so it follows the raw
        // ball or hover under Reduce Motion too, just without animating.
        let facing = lit.map { drifted[$0] } ?? grab ?? hover
        let nearWhileGrabbed: Int? = grab == nil ? nil : near
        // Built in steps: as one expression, the layers and all their modifiers take the compiler too long to type-check.
        let layers = courtLayers(
            size, portrait: portrait, lines: lines, pointer: pointer, logo: logo, lit: lit,
            drifted: drifted, positions: positions)
        let felt = layers
            .frame(width: size.width, height: size.height)
            .sensoryFeedback(trigger: nearWhileGrabbed) { _, new -> SensoryFeedback? in new == nil ? nil : .selection }
            .sensoryFeedback(trigger: passing) { _, new -> SensoryFeedback? in
                new == nil ? nil : .impact(weight: .light)
            }
        // A resize can carry the logo past a still pointer, so the logo's position re-decides the facing too.
        let facingLogo = felt
            .onChange(of: facing) { _, target in turn(toward: target, logoX: logo.x) }
            .onChange(of: logo.x) { _, logoX in turn(toward: facing, logoX: logoX) }
        return facingLogo
            .contentShape(Rectangle())
            .coordinateSpace(.named(courtSpace))
            .onContinuousHover(coordinateSpace: .named(courtSpace)) { phase in
                switch phase {
                case .active(let point): hover = point
                case .ended: hover = nil
                }
            }
    }

    /// The court's lines, glow, pass line, plays, column and ball, back to front.
    private func courtLayers(
        _ size: CGSize, portrait: Bool, lines: AnyShape, pointer: CGPoint?, logo: CGPoint, lit: Int?,
        drifted: [CGPoint], positions: [CGPoint]
    ) -> some View {
        ZStack(alignment: .topLeading) {
            lines.stroke(Color.primary.opacity(0.065), lineWidth: 1.25)
            if let pointer {
                let center = UnitPoint(x: pointer.x / size.width, y: pointer.y / size.height)
                lines.stroke(Self.accent, lineWidth: 1.5)
                    .mask(RadialGradient(colors: [.black, .clear], center: center, startRadius: 0, endRadius: 260))
                RadialGradient(
                    colors: [Self.accent.opacity(0.08), .clear], center: center, startRadius: 0, endRadius: 420)
            }
            passLine(from: logo, to: lit.map { drifted[$0] } ?? grab, aimed: lit != nil)
            ForEach(plays) { play in
                playButton(play, lit: lit == play.slot)
                    .position(drifted[play.slot])
                    .animation(.spring(response: 0.35, dampingFraction: 0.8), value: drifted[play.slot])
            }
            // Above the plays, so a play squeezed by a short pane can never take a click meant for the composer.
            column(size, pointer: pointer, portrait: portrait, pass: pass(among: drifted, landing: positions))
            if let ball = grab ?? flight {
                self.ball.position(ball)
            }
        }
    }

    private func turn(toward target: CGPoint?, logoX: CGFloat) {
        facesLeft = PointGuardCourtGeometry.facesLeft(toward: target, logoX: logoX, current: facesLeft)
    }

    /// A solid pass to the lit play, or a dotted one to a ball that isn't on a play yet.
    private func passLine(from logo: CGPoint, to target: CGPoint?, aimed: Bool) -> some View {
        ZStack {
            if let target {
                Path {
                    $0.move(to: logo); $0.addLine(to: target)
                }
                .stroke(
                    aimed ? Self.accent : Color.primary.opacity(0.22),
                    style: StrokeStyle(lineWidth: aimed ? 1.75 : 1, lineCap: .round, dash: aimed ? [] : [2, 7]))
            }
        }
        .allowsHitTesting(false).accessibilityHidden(true)
    }

    private var ball: some View {
        ZStack {
            Circle().fill(Color.primary.opacity(0.07)).frame(width: 56, height: 56)
            Circle()
                .fill(
                    RadialGradient(
                        colors: [Color.white.opacity(0.55), Self.accent], center: UnitPoint(x: 0.35, y: 0.3),
                        startRadius: 0, endRadius: 9)
                )
                .frame(width: 18, height: 18).scaleEffect(1.25)
                .shadow(color: Self.accent.opacity(0.6), radius: 8)
        }
        .allowsHitTesting(false).accessibilityHidden(true)
    }

    /// A thumb on the logo picks up a ball; released near a play, the ball flies there and the play runs.
    private func pass(among drifted: [CGPoint], landing positions: [CGPoint]) -> some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .named(courtSpace))
            .updating($grab) { value, grab, _ in if canRun { grab = value.location } }
            .onChanged { _ in
                // A new grab takes the ball back from a pass still in the air.
                guard canRun, passTask != nil else { return }
                passTask?.cancel(); passTask = nil; passing = nil; flight = nil
            }
            .onEnded { value in
                guard canRun,
                    let slot = PointGuardCourtGeometry.nearestSlot(
                        to: value.location, among: drifted, reach: PointGuardCourtGeometry.touchReach),
                    let play = plays.first(where: { $0.slot == slot })
                else { return }
                run(play, flying: (value.location, positions[slot]))
            }
    }

    private func column(_ size: CGSize, pointer: CGPoint?, portrait: Bool, pass: some Gesture) -> some View {
        let diameter: CGFloat = portrait ? 96 : 128
        return VStack(spacing: 0) {
            logoMark(
                diameter: diameter, tilt: pointer.map { PointGuardCourtGeometry.tilt(toward: $0, in: size) } ?? 0,
                facesLeft: facesLeft)
                .contentShape(Circle())
                // The Mac's pointer already passes by hovering.
                .gesture(pass, isEnabled: PointGuardCourtGeometry.docksComposer)
            // The portrait heading breaks into two lines, as on the phone.
            heading(size: portrait ? 30 : 40).frame(maxWidth: portrait ? 300 : nil).padding(.top, portrait ? 24 : 36)
            if !PointGuardCourtGeometry.docksComposer {
                slot.padding(.top, 24)
            }
        }
        .background {
            GeometryReader { proxy in
                let bottom = proxy.frame(in: .named(courtSpace)).maxY
                Color.clear
                    .onAppear { columnBottom = bottom }
                    .onChange(of: bottom) { _, bottom in columnBottom = bottom }
            }
        }
        .frame(maxWidth: PurePointTheme.chatContentWidth + 2 * PurePointTheme.chatHorizontalPadding)
        .frame(width: size.width)
        .padding(
            .top,
            max(16, PointGuardCourtGeometry.logoPoint(in: size, portrait: portrait).y - diameter / 2))
    }

    // MARK: - Narrow and sidebar

    private func stacked(height: CGFloat) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                logoMark(diameter: 88, tilt: 0, facesLeft: false)
                heading(size: 26).padding(.top, 24).padding(.horizontal, 16)
                if !PointGuardCourtGeometry.docksComposer {
                    slot.padding(.top, 16)
                }
                VStack(spacing: 8) {
                    ForEach([0, 2], id: \.self) { row in
                        HStack(spacing: 8) {
                            ForEach(plays[row..<min(row + 2, plays.count)]) { play in pill(play) }
                        }
                    }
                }
                .padding(.horizontal, compact ? 8 : PurePointTheme.chatHorizontalPadding)
                .padding(.top, PointGuardCourtGeometry.docksComposer ? 24 : 8)
            }
            .frame(maxWidth: PurePointTheme.chatContentWidth)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 32)
            .frame(minHeight: height)
        }
    }

    // MARK: - Pieces

    private func logoMark(diameter: CGFloat, tilt: Double, facesLeft: Bool) -> some View {
        Image("PurePointLogo").resizable().scaledToFit()
            .frame(width: diameter * 0.6, height: diameter * 0.6)
            .scaleEffect(x: facesLeft ? -1 : 1, y: 1)
            .animation(reduceMotion ? nil : .spring(response: 0.25, dampingFraction: 0.9), value: facesLeft)
            .rotationEffect(.degrees(tilt))
            .animation(reduceMotion ? nil : .spring(response: 0.4, dampingFraction: 0.8), value: tilt)
            .frame(width: diameter, height: diameter)
            .background(Color(nsColor: Theme.composerBackground), in: Circle())
            .overlay { Circle().strokeBorder(Color.primary.opacity(0.09)) }
            .shadow(color: Color(nsColor: Theme.composerShadow), radius: 24, y: 12)
            .accessibilityLabel("Point Guard")
    }

    private func heading(size: CGFloat) -> some View {
        Text("What play do you want to run?")
            .font(.system(size: size, weight: .bold)).tracking(-size * 0.02)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityAddTraits(.isHeader)
    }

    /// The composer stays put while disconnected, so a reconnecting session keeps its draft and receipts in view.
    private var slot: some View {
        VStack(spacing: 8) {
            if !connected {
                HStack(spacing: 8) {
                    Text("Finish setup to start.").font(.system(size: 13)).foregroundStyle(.secondary)
                    Button("Open Settings", action: onOpenSettings).buttonStyle(.borderless).tint(Self.accent)
                        .font(.system(size: 13, weight: .medium))
                }
            }
            composer()
        }
    }

    private func playButton(_ play: PointGuardPlay, lit: Bool) -> some View {
        Button {
            run(play)
        } label: {
            VStack(spacing: 10) {
                Image(systemName: play.symbol).font(.system(size: 16, weight: .semibold))
                    .frame(width: 44, height: 44)
                    .foregroundStyle(lit ? Color(nsColor: Theme.chatBackground) : Color.secondary)
                    .background(lit ? Self.accent : Color(nsColor: Theme.composerBackground), in: Circle())
                    .overlay { Circle().strokeBorder(Color.primary.opacity(lit ? 0 : 0.16), lineWidth: 1.5) }
                    .shadow(color: Self.accent.opacity(lit ? 0.5 : 0), radius: 14)
                    .scaleEffect(lit ? 1.08 : 1)
                Text(play.title).font(.system(size: 13, weight: lit ? .semibold : .medium)).lineLimit(1)
                    .foregroundStyle(lit ? Self.accent : Color.secondary)
            }
            .frame(width: 2 * PointGuardCourtGeometry.labelHalfWidth)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .opacity(passing != nil && passing != play.slot ? 0.35 : 1)
        .animation(.easeOut(duration: 0.2), value: lit)
        .disabled(!canRun)
        .help(play.prompt)
        .accessibilityLabel(play.title)
        .accessibilityHint(play.prompt.isEmpty ? "Sends your saved prompt" : play.prompt)
        .contextMenu { menu(for: play) }
    }

    private func pill(_ play: PointGuardPlay) -> some View {
        Button {
            if canRun { onRun(play) }
        } label: {
            Label(play.title, systemImage: play.symbol).font(.system(size: 13, weight: .medium)).lineLimit(1)
                .frame(maxWidth: .infinity, minHeight: 44)
                .background(
                    Color(nsColor: Theme.composerBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous)
                )
                .overlay {
                    RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Color.primary.opacity(0.09))
                }
                .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(!canRun)
        .accessibilityHint(play.prompt.isEmpty ? "Sends your saved prompt" : play.prompt)
        .contextMenu { menu(for: play) }
    }

    @ViewBuilder private func menu(for play: PointGuardPlay) -> some View {
        Menu("Replace With") {
            if prompts.isEmpty {
                Text("No global prompts yet")
            }
            ForEach(prompts) { prompt in
                Button(prompt.needsInput ? "\(prompt.name) (needs input)" : prompt.name) {
                    onAssign(play.slot, prompt)
                }
                .disabled(prompt.needsInput)
            }
        }
        if play.isCustom {
            Button("Reset to Default") { onReset(play.slot) }
        }
    }

    /// Draws the pass to the play, then sends once it lands. A ball released from the thumb flies there first.
    private func run(_ play: PointGuardPlay, flying path: (from: CGPoint, to: CGPoint)? = nil) {
        guard canRun, passing == nil else { return }
        guard !reduceMotion else { return onRun(play) }
        passing = play.slot
        if let path {
            flight = path.from
            withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) { flight = path.to }
        }
        passTask = Task {
            // A pass interrupted by the court going away (a new chat, a resumed one) or a new grab never sends.
            guard (try? await Task.sleep(for: .milliseconds(450))) != nil, !Task.isCancelled else { return }
            passing = nil; flight = nil; passTask = nil
            onRun(play)
        }
    }
}

/// Half-court markings in a 1000×640 space, scaled to fill like `preserveAspectRatio slice`.
private struct CourtLines: Shape {
    func path(in rect: CGRect) -> Path {
        let scale = max(rect.width / 1000, rect.height / 640)
        let origin = CGPoint(x: rect.midX - 500 * scale, y: rect.midY - 320 * scale)
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: origin.x + x * scale, y: origin.y + y * scale) }
        func circle(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat) -> CGRect {
            CGRect(origin: p(x - r, y - r), size: CGSize(width: 2 * r * scale, height: 2 * r * scale))
        }
        var path = Path()
        path.addRoundedRect(
            in: CGRect(origin: p(24, 24), size: CGSize(width: 952 * scale, height: 592 * scale)),
            cornerSize: CGSize(width: 6 * scale, height: 6 * scale))
        path.move(to: p(24, 192)); path.addLine(to: p(976, 192))
        path.addEllipse(in: circle(500, 192, 96))
        path.addEllipse(in: circle(500, 192, 34))
        // Three-point line, lane and free-throw circle at the basket end.
        path.move(to: p(210, 616)); path.addLine(to: p(210, 560))
        path.addArc(
            center: p(500, 560), radius: 290 * scale, startAngle: .degrees(180), endAngle: .degrees(360),
            clockwise: false)
        path.addLine(to: p(790, 616))
        path.addRect(CGRect(origin: p(436, 456), size: CGSize(width: 128 * scale, height: 160 * scale)))
        path.addEllipse(in: circle(500, 456, 64))
        // The far three-point line, peeking over the top edge.
        path.move(to: p(210, 24)); path.addLine(to: p(210, 40))
        path.addQuadCurve(to: p(790, 40), control: p(500, 280))
        path.addLine(to: p(790, 24))
        return path
    }
}

/// The portrait court, after the phone's: the half-court line through the logo, the basket end running on under the
/// docked composer. Widths scale with the pane and heights follow the plays, so circles stay round and the arc stays
/// just over the low blocks however far a short pane pushes them down.
private struct PortraitCourtLines: Shape {
    /// Where the low blocks stand, in the court's points.
    let blocks: CGFloat

    func path(in rect: CGRect) -> Path {
        let scale = rect.width / 390
        func x(_ value: CGFloat) -> CGFloat { rect.minX + value * scale }
        func y(_ value: CGFloat) -> CGFloat { rect.minY + value * scale }
        func circle(_ center: CGPoint, _ r: CGFloat) -> CGRect {
            CGRect(x: center.x - r * scale, y: center.y - r * scale, width: 2 * r * scale, height: 2 * r * scale)
        }
        let below = rect.maxY + 40 * scale
        let halfCourt = rect.minY + rect.height * PointGuardCourtGeometry.portraitLogo.y
        let arcTop = rect.minY + blocks - 40 * scale
        let laneTop = arcTop + 115 * scale
        var path = Path()
        path.addRoundedRect(
            in: CGRect(x: x(12), y: y(12), width: 366 * scale, height: below - y(12)),
            cornerSize: CGSize(width: 6 * scale, height: 6 * scale))
        path.move(to: CGPoint(x: x(12), y: halfCourt)); path.addLine(to: CGPoint(x: x(378), y: halfCourt))
        path.addEllipse(in: circle(CGPoint(x: rect.midX, y: halfCourt), 86))
        path.addEllipse(in: circle(CGPoint(x: rect.midX, y: halfCourt), 30))
        // The far three-point line, peeking over the top edge.
        path.move(to: CGPoint(x: x(70), y: y(12))); path.addLine(to: CGPoint(x: x(70), y: y(24)))
        path.addQuadCurve(to: CGPoint(x: x(320), y: y(24)), control: CGPoint(x: rect.midX, y: halfCourt - 66 * scale))
        path.addLine(to: CGPoint(x: x(320), y: y(12)))
        // Three-point line, lane and free-throw circle at the basket end.
        let arc = CGPoint(x: rect.midX, y: arcTop + 173 * scale)
        path.move(to: CGPoint(x: x(22), y: max(below, arc.y))); path.addLine(to: CGPoint(x: x(22), y: arc.y))
        path.addArc(
            center: arc, radius: 173 * scale, startAngle: .degrees(180), endAngle: .degrees(360), clockwise: false)
        path.addLine(to: CGPoint(x: x(368), y: max(below, arc.y)))
        path.addRect(CGRect(x: rect.midX - 60 * scale, y: laneTop, width: 120 * scale, height: max(0, below - laneTop)))
        path.addEllipse(in: circle(CGPoint(x: rect.midX, y: laneTop), 60))
        // The basket end runs on past the bottom edge; keep only what lies on the court.
        return path.lineIntersection(Path(rect))
    }
}
