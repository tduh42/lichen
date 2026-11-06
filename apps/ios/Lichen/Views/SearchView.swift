import SwiftUI

/// "Search": follow an SOS — bearing and distance from our GPS to theirs (mockup `recherche`).
/// Radio bearings, pings and the particle filter come with search mode (L8).
struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var refresh: Timer?

    var body: some View {
        NavigationStack {
            Group {
                if let target {
                    content(target)
                } else {
                    empty
                }
            }
            .screen()
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 2) {
                        Text(title).font(.system(size: 17, weight: .semibold))
                        if target != nil {
                            Text("\(model.sosLog.count) positions received")
                                .font(.system(size: 12)).foregroundStyle(Theme.tx2)
                        }
                    }
                }
            }
        }
        .onAppear {
            // Our position, to measure the distance.
            model.mapVisible = true
            refresh = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in
                MainActor.assumeIsolated { model.refreshPositions() }
            }
        }
        .onDisappear {
            model.mapVisible = false
            refresh?.invalidate()
        }
    }

    private var target: PeerPosition? { model.incomingSos ?? model.sosLog.last }

    private var title: String {
        guard let t = target else { return String(localized: "Search") }
        return String(localized: "Search · \(model.name(of: t.from))")
    }

    private var empty: some View {
        VStack(spacing: 12) {
            Image(systemName: "scope").font(.system(size: 44)).foregroundStyle(Theme.tx2)
            Text("No search under way").font(.system(size: 17, weight: .semibold))
            Text("When someone sends an SOS, follow it from here: bearing and distance from your position.")
                .font(.system(size: 14))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
            LaterNote(text: "Search without GPS (radio bearings, pings) comes with search mode, L8.")
                .padding(.top, 4)
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func content(_ t: PeerPosition) -> some View {
        let d = model.distanceTo(t)
        return ScrollView {
            VStack(spacing: 12) {
                Banner(kind: .sos, title: String(localized: "SOS received \(MapScreen.ago(t.atS))"),
                       message: String(localized: "GPS ±\(Int(t.radiusM.rounded())) m · \(String(format: "%.4f, %.4f", t.lat, t.lon))"))
                Radar(target: t, distance: d)
                    .frame(height: 230)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
                bearingCard(t, d)
                if !model.sosLog.isEmpty {
                    CardList {
                        ForEach(Array(model.sosLog.reversed().prefix(8).enumerated()), id: \.offset) { i, p in
                            if i > 0 { CardDivider() }
                            MeasurementRow(
                                who: model.name(of: p.from),
                                detail: "±\(Int(p.radiusM.rounded())) m · \(MapScreen.ago(p.atS))",
                                value: model.lastSnr[p.from].map { String(format: "SNR %.0f dB", $0) }
                                    ?? model.distanceTo(p).map { distanceText($0.meters) } ?? "—"
                            )
                        }
                    }
                }
                HStack(spacing: 8) {
                    Button("Ping") {}.buttonStyle(SecondaryButtonStyle(height: 56))
                    Button("Add a bearing") {}.buttonStyle(SecondaryButtonStyle(height: 56))
                    Button("Lower power") {}.buttonStyle(SecondaryButtonStyle(height: 56))
                }
                .disabled(true)
                LaterNote(text: "Pings, radio bearings and power control come with search mode (L8).")
            }
            .padding(16)
        }
    }

    private func bearingCard(_ t: PeerPosition, _ d: (meters: Double, bearing: Double)?) -> some View {
        HStack(spacing: 16) {
            Image(systemName: "location.north.fill")
                .font(.system(size: 30))
                .foregroundStyle(Theme.lora)
                .rotationEffect(.degrees(d?.bearing ?? 0))
                .frame(width: 64, height: 64)
                .background(Theme.s2, in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                if let d {
                    Text("\(Int(d.bearing.rounded()))° · \(distanceText(d.meters))").font(Theme.figure(28))
                    Text("Bearing from north · uncertainty ±\(Int(t.radiusM.rounded())) m")
                        .font(.system(size: 13)).foregroundStyle(Theme.tx2)
                } else {
                    Text("Waiting for your position").font(.system(size: 17, weight: .semibold))
                    Text("The bearing and the distance appear with your GPS fix.")
                        .font(.system(size: 13)).foregroundStyle(Theme.tx2)
                }
            }
            Spacer(minLength: 0)
        }
        .card(padding: 16)
        .accessibilityElement(children: .combine)
    }
}

/// A schematic plan: the SOS position and its uncertainty in the middle, us on the side.
private struct Radar: View {
    let target: PeerPosition
    let distance: (meters: Double, bearing: Double)?

    var body: some View {
        Canvas { ctx, size in
            let c = CGPoint(x: size.width / 2, y: size.height / 2)
            let maxR = min(size.width, size.height) / 2 - 18
            for f in [0.33, 0.66, 1.0] {
                let r = maxR * f
                ctx.stroke(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)),
                           with: .color(Theme.line), lineWidth: 1)
            }
            // Scale: we sit on the outer ring; the uncertainty is to scale.
            let span = max(distance?.meters ?? target.radiusM * 4, target.radiusM * 2, 50)
            let ur = max(10, maxR * target.radiusM / span)
            ctx.fill(Path(ellipseIn: CGRect(x: c.x - ur, y: c.y - ur, width: 2 * ur, height: 2 * ur)),
                     with: .color(Theme.lora.opacity(0.25)))
            ctx.fill(Path(ellipseIn: CGRect(x: c.x - 6, y: c.y - 6, width: 12, height: 12)), with: .color(Theme.sos))
            if let d = distance {
                // We are at the opposite of their bearing from us.
                let a = (d.bearing + 180) * .pi / 180
                let p = CGPoint(x: c.x + maxR * sin(a), y: c.y - maxR * cos(a))
                var line = Path()
                line.move(to: p)
                line.addLine(to: c)
                ctx.stroke(line, with: .color(Theme.me.opacity(0.7)), style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
                ctx.fill(Path(ellipseIn: CGRect(x: p.x - 7, y: p.y - 7, width: 14, height: 14)), with: .color(Theme.me))
            }
            ctx.draw(Text("N").font(.system(size: 11, weight: .bold)).foregroundStyle(Theme.tx2),
                     at: CGPoint(x: c.x, y: 10))
        }
        .accessibilityLabel("Plan of the SOS position and yours")
    }
}
