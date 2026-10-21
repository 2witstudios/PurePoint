import SwiftUI

/// The court's own coordinate space, shared by hover tracking and the composer's measured edge.
private let courtSpace = "pointGuardCourt"

/// Point Guard's new-conversation screen: a half-court with the logo at the top of the key,
/// the composer under it, and four plays that each send a whole prompt when passed to.
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
    @State private var composerBottom: CGFloat = 0
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private static var accent: Color { Color(nsColor: Theme.pointGuardAccent) }

    var body: some View {
        GeometryReader { proxy in
            if compact || proxy.size.width < 520 {
                stacked(height: proxy.size.height)
            } else {
                court(proxy.size)
            }
        }
    }

    // MARK: - Court

    private func court(_ size: CGSize) -> some View {
        let positions = PointGuardCourtGeometry.positions(
            in: size, columnWidth: PurePointTheme.chatContentWidth, composerBottom: composerBottom)
        let pointer = reduceMotion ? nil : hover
        let drifted = positions.map { point in
            guard let pointer else { return point }
            return CGPoint(x: point.x + (pointer.x - point.x) * 0.05, y: point.y + (pointer.y - point.y) * 0.05)
        }
        let near = pointer.flatMap { PointGuardCourtGeometry.nearestSlot(to: $0, among: drifted) }
        let lit = passing ?? near
        let logo = PointGuardCourtGeometry.logoPoint(in: size)
        return ZStack(alignment: .topLeading) {
            CourtLines().stroke(Color.primary.opacity(0.065), lineWidth: 1.25)
            if let pointer {
                let center = UnitPoint(x: pointer.x / size.width, y: pointer.y / size.height)
                CourtLines().stroke(Self.accent, lineWidth: 1.5)
                    .mask(RadialGradient(colors: [.black, .clear], center: center, startRadius: 0, endRadius: 260))
                RadialGradient(
                    colors: [Self.accent.opacity(0.08), .clear], center: center, startRadius: 0, endRadius: 420)
            }
            passLines(from: logo, pointer: pointer, to: lit.map { drifted[$0] })
            column(size, pointer: pointer)
            ForEach(plays) { play in
                playButton(play, lit: lit == play.slot)
                    .position(drifted[play.slot])
                    .animation(.spring(response: 0.35, dampingFraction: 0.8), value: drifted[play.slot])
            }
            if let ball = passing.map({ drifted[$0] }) ?? pointer {
                Circle().fill(Self.accent).frame(width: 14, height: 14)
                    .shadow(color: Self.accent.opacity(0.6), radius: 8)
                    .position(ball)
                    .animation(.spring(response: passing == nil ? 0.22 : 0.45, dampingFraction: 0.85), value: ball)
                    .allowsHitTesting(false).accessibilityHidden(true)
            }
        }
        .frame(width: size.width, height: size.height)
        .contentShape(Rectangle())
        .coordinateSpace(.named(courtSpace))
        .onContinuousHover(coordinateSpace: .named(courtSpace)) { phase in
            switch phase {
            case .active(let point): hover = point
            case .ended: hover = nil
            }
        }
    }

    private func passLines(from logo: CGPoint, pointer: CGPoint?, to target: CGPoint?) -> some View {
        ZStack {
            if let target {
                Path {
                    $0.move(to: logo); $0.addLine(to: target)
                }
                .stroke(Self.accent, style: StrokeStyle(lineWidth: 1.75, lineCap: .round))
            } else if let pointer {
                Path {
                    $0.move(to: logo); $0.addLine(to: pointer)
                }
                .stroke(Color.primary.opacity(0.22), style: StrokeStyle(lineWidth: 1, lineCap: .round, dash: [2, 7]))
            }
        }
        .allowsHitTesting(false).accessibilityHidden(true)
    }

    private func column(_ size: CGSize, pointer: CGPoint?) -> some View {
        VStack(spacing: 0) {
            logoMark(diameter: 128, tilt: pointer.map { PointGuardCourtGeometry.tilt(toward: $0, in: size) } ?? 0)
            heading(size: 40).padding(.top, 36)
            slot.padding(.top, 24)
                .background {
                    GeometryReader { proxy in
                        let bottom = proxy.frame(in: .named(courtSpace)).maxY
                        Color.clear
                            .onAppear { composerBottom = bottom }
                            .onChange(of: bottom) { _, bottom in composerBottom = bottom }
                    }
                }
        }
        .frame(maxWidth: PurePointTheme.chatContentWidth + 2 * PurePointTheme.chatHorizontalPadding)
        .frame(width: size.width)
        .padding(.top, max(16, size.height * PointGuardCourtGeometry.logo.y - 64))
    }

    // MARK: - Narrow and sidebar

    private func stacked(height: CGFloat) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                logoMark(diameter: 88, tilt: 0)
                heading(size: 26).padding(.top, 24).padding(.horizontal, 16)
                slot.padding(.top, 16)
                VStack(spacing: 8) {
                    ForEach([0, 2], id: \.self) { row in
                        HStack(spacing: 8) {
                            ForEach(plays[row..<min(row + 2, plays.count)]) { play in pill(play) }
                        }
                    }
                }
                .padding(.horizontal, compact ? 8 : PurePointTheme.chatHorizontalPadding)
                .padding(.top, 8)
            }
            .frame(maxWidth: PurePointTheme.chatContentWidth)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 32)
            .frame(minHeight: height)
        }
    }

    // MARK: - Pieces

    private func logoMark(diameter: CGFloat, tilt: Double) -> some View {
        Image("PurePointLogo").resizable().scaledToFit()
            .frame(width: diameter * 0.6, height: diameter * 0.6)
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

    @ViewBuilder private var slot: some View {
        if connected {
            composer()
        } else {
            HStack(spacing: 16) {
                Text("Finish setup to start.").font(.system(size: 14)).foregroundStyle(.secondary)
                Spacer(minLength: 0)
                Button("Open Settings", action: onOpenSettings).buttonStyle(.borderedProminent).tint(Self.accent)
            }
            .padding(.vertical, 12).padding(.horizontal, 16)
            .background(Color(nsColor: Theme.composerBackground), in: RoundedRectangle(cornerRadius: 16))
            .overlay { RoundedRectangle(cornerRadius: 16).strokeBorder(Color.primary.opacity(0.08)) }
            .frame(maxWidth: 520)
            .padding(.horizontal, compact ? 8 : PurePointTheme.chatHorizontalPadding)
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

    /// Passes the ball, then sends once it lands.
    private func run(_ play: PointGuardPlay) {
        guard canRun, passing == nil else { return }
        guard !reduceMotion else { return onRun(play) }
        passing = play.slot
        Task {
            try? await Task.sleep(for: .milliseconds(450))
            onRun(play)
            passing = nil
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
