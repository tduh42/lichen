import SwiftUI

/// SOS screens' own background and cards (mockups `sos`, `sos-recu`).
private enum SosColor {
    static let screen = Color(dark: 0x1A0B0B, light: 0xFEF2F2)
    static let card = Color(dark: 0x2A1212, light: 0xFEE2E2)
}

/// Start or stop an SOS: hold for 3 s (mockup `sos`).
struct SosView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        // Scrolls at large text sizes and in French (audit M12).
        ScrollView {
        VStack(spacing: 20) {
            HStack {
                Text("Distress alert").font(.system(.subheadline, weight: .semibold)).foregroundStyle(Theme.sosFg)
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark")
                        .font(.system(.callout, weight: .bold))
                        .frame(width: 44, height: 44)
                        .background(Theme.sosBg, in: Circle())
                        .foregroundStyle(Theme.sosFg)
                }
                .accessibilityLabel("Close")
            }
            if model.sos {
                VStack(spacing: 14) {
                    Circle()
                        .fill(Theme.sosBtn)
                        .frame(width: 168, height: 168)
                        .overlay {
                            VStack(spacing: 4) {
                                Text("SOS").font(.system(size: 44, weight: .heavy))
                                Text("on").font(.system(.subheadline, weight: .medium))
                            }
                            .foregroundStyle(.white)
                        }
                        .padding(26)
                        .overlay(Circle().stroke(Theme.sos, lineWidth: 8))
                    Button("Stop the SOS") { model.setSos(false) }
                        .buttonStyle(.primary)
                }
                .padding(.top, 8)
                statusBanner
            } else {
                SosHoldButton { model.setSos(true) }
                    .padding(.top, 8)
            }
            VStack(spacing: 10) {
                card(String(localized: "What is sent"),
                     String(localized: "Your exact GPS position, again every 2 minutes. Without GPS, a beacon your groups can locate by radio."))
                card(String(localized: "To whom"), recipients)
                card(String(localized: "How"),
                     String(localized: "Over LoRa to each contact you share with, flooded to each group, on the mesh's usual radio settings."))
            }
            Spacer(minLength: 0)
            Text("The SOS stays on until you stop it. It does not replace 112.")
                .font(.system(.caption))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
        }
        .padding(.horizontal, 24)
        .padding(.top, 16)
        .padding(.bottom, 24)
        }
        .background(SosColor.screen.ignoresSafeArea())
    }

    /// What the SOS really does now: never a delivery claim (nobody acknowledges a flood).
    @ViewBuilder
    private var statusBanner: some View {
        let st = model.sosStatus
        if !model.isConnected {
            Banner(kind: .warn, title: String(localized: "Radio disconnected"),
                   message: String(localized: "Nothing goes out until your radio is back."))
        } else if st?.recipients == 0 {
            Banner(kind: .warn, title: String(localized: "Nobody receives it"),
                   message: String(localized: "Share your position with a contact or join a group."))
        } else if st?.beaconOnly == true {
            Banner(kind: .warn,
                   title: model.locationDenied ? String(localized: "Location is off") : String(localized: "Waiting for GPS"),
                   message: String(localized: "Meanwhile your groups get a beacon without position every 2 minutes, to locate you by radio."),
                   action: model.locationDenied ? (String(localized: "Settings"), {
                       if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                   }) : nil)
        } else if let st, st.refused > 0 {
            Banner(kind: .warn, title: String(localized: "\(st.refused) conversations can’t get your position"),
                   message: String(localized: "A group you left or that was closed, or an identity not ready yet. The others still get it."))
        } else if let at = st?.lastQueuedS {
            // Queued, not sent: it leaves when the radio has airtime (recheck R09).
            Banner(kind: .ok, title: String(localized: "Queued \(MapScreen.ago(at)) for \(st?.recipients ?? 0) conversations"),
                   message: String(localized: "Again every 2 minutes. It leaves as soon as the radio has airtime, and nobody confirms reception: watch for answers."))
        } else {
            Banner(kind: .ok, title: String(localized: "Starting"),
                   message: String(localized: "The first position goes out in a moment."))
        }
    }

    private var recipients: String {
        var names = model.groups.map { "\($0.name) (\($0.members.count))" }
        for s in model.shares {
            if case .contact = s.chat { names.append(model.title(of: s.chat)) }
        }
        if names.isEmpty { return String(localized: "Nobody yet: share your position with a contact or join a group.") }
        return ListFormatter.localizedString(byJoining: names)
    }

    private func card(_ title: String, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.system(.footnote)).foregroundStyle(Theme.sosFg)
            Text(text).font(.system(.subheadline)).fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(SosColor.card, in: RoundedRectangle(cornerRadius: 14))
    }
}

/// Someone sent an SOS: full-screen alert (mockup `sos-recu`), live while it is up.
struct SosReceivedView: View {
    let from: Data
    @Environment(AppModel.self) private var model
    @State private var error: String?

    var body: some View {
        TimelineView(.periodic(from: .now, by: 5)) { _ in
            if let sos = model.incidents[from] { content(sos) }
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 16)
        .background(SosColor.screen.ignoresSafeArea())
        // Our position, for the distance and bearing.
        .onAppear { model.needsLocation("sosAlert", true) }
        .onDisappear { model.needsLocation("sosAlert", false) }
    }

    private func content(_ sos: SosIncident) -> some View {
        let heard = MapScreen.ago(UInt64(sos.lastHeard.timeIntervalSince1970))
        return VStack(spacing: 16) {
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.system(.title, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 72, height: 72)
                    .background(Theme.sosBtn, in: Circle())
                Text("SOS alert · \(MapScreen.ago(UInt64(sos.started.timeIntervalSince1970)))")
                    .font(.system(.subheadline, weight: .semibold))
                    .foregroundStyle(Theme.sosFg)
                Text(model.name(of: from))
                    .font(Theme.wordmark(40))
                    .tracking(-1)
                Text(via(sos)).font(.system(.subheadline)).foregroundStyle(Theme.tx2)
            }
            .multilineTextAlignment(.center)
            .padding(.top, 24)
            CardList {
                if let p = sos.position {
                    CardRow(title: String(localized: "Last GPS position"),
                            subtitle: "\(MapScreen.ago(p.atS)) · ±\(Int(p.radiusM.rounded())) m · \(String(format: "%.4f, %.4f", p.lat, p.lon))")
                    CardDivider()
                    if let d = model.distanceTo(p) {
                        CardRow(title: String(localized: "From you")) {
                            Text("\(distanceText(d.meters)) · \(Int(d.bearing.rounded()))°").font(Theme.figure(18))
                        }
                    } else {
                        CardRow(title: String(localized: "From you"), subtitle: String(localized: "Your position is not known yet")) {
                            Badge(text: String(localized: "unknown"), background: Theme.warnBg, foreground: Theme.warnFg)
                        }
                    }
                } else {
                    CardRow(title: String(localized: "No GPS position"),
                            subtitle: String(localized: "Their phone sends beacons: locate them by radio."))
                }
                CardDivider()
                CardRow(title: String(localized: "Beacon"),
                        subtitle: String(localized: "Heard \(heard)") + (sos.battery.map { String(localized: " · battery \($0) %") } ?? "")) {
                    switch sos.state() {
                    case .active: Badge(text: String(localized: "active"), background: Theme.loraBg, foreground: Theme.loraFg)
                    case .stale: Badge(text: String(localized: "silent"), background: Theme.warnBg, foreground: Theme.warnFg)
                    case .ended: Badge(text: String(localized: "ended"), background: Theme.okBg, foreground: Theme.okFg)
                    }
                }
            }
            if let error { Banner(kind: .warn, title: error) }
            Spacer(minLength: 0)
            VStack(spacing: 8) {
                Button(model.search(for: from) == nil ? "Start locating" : "Follow in Locate") {
                    if model.search(for: from) == nil, model.searchGroup(for: from) != nil {
                        if let e = model.startSearch(from) {
                            error = e
                            return
                        }
                    }
                    model.searchTarget = from
                    model.tab = .search
                    model.sosAlert = nil
                }
                .buttonStyle(.primary)
                HStack(spacing: 8) {
                    Button("Reply") {
                        model.tab = .messages
                        model.openRequest = .contact(key: from)
                        model.sosAlert = nil
                    }
                    .buttonStyle(.secondary)
                    .disabled(!model.contacts.contains { $0.key == from })
                    Button("Show on the map") {
                        model.tab = .map
                        model.sosAlert = nil
                    }
                    .buttonStyle(.secondary)
                    .disabled(sos.position == nil)
                }
                Button("Dismiss") { model.sosAlert = nil }
                    .font(.system(.subheadline, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 44)
                    .foregroundStyle(Theme.tx2)
            }
        }
    }

    private func via(_ sos: SosIncident) -> String {
        if let gid = sos.group {
            let name = model.groups.first { $0.gid == gid }?.name ?? String(localized: "Group")
            return String(localized: "Received over LoRa · group \(name)")
        }
        return String(localized: "Received over LoRa · direct")
    }
}

/// "850 m", "2.4 km".
func distanceText(_ m: Double) -> String {
    let f = MeasurementFormatter()
    f.unitOptions = .naturalScale
    f.numberFormatter.maximumFractionDigits = m < 1_000 ? 0 : 1
    return f.string(from: Measurement(value: m, unit: UnitLength.meters))
}

extension AppModel {
    /// Distance (m) and bearing (degrees from north) from us to `p`.
    func distanceTo(_ p: PeerPosition) -> (meters: Double, bearing: Double)? {
        distanceTo(lat: p.lat, lon: p.lon)
    }

    /// Distance (m) and bearing (degrees from north) from us to a point.
    func distanceTo(lat: Double, lon: Double) -> (meters: Double, bearing: Double)? {
        guard let me = myFix else { return nil }
        let p = (lat: lat, lon: lon)
        let r = 6_371_000.0
        let (la1, lo1, la2, lo2) = (me.lat * .pi / 180, me.lon * .pi / 180, p.lat * .pi / 180, p.lon * .pi / 180)
        let dla = la2 - la1, dlo = lo2 - lo1
        let a = sin(dla / 2) * sin(dla / 2) + cos(la1) * cos(la2) * sin(dlo / 2) * sin(dlo / 2)
        let d = 2 * r * atan2(sqrt(a), sqrt(1 - a))
        let y = sin(dlo) * cos(la2)
        let x = cos(la1) * sin(la2) - sin(la1) * cos(la2) * cos(dlo)
        let b = (atan2(y, x) * 180 / .pi + 360).truncatingRemainder(dividingBy: 360)
        return (d, b)
    }
}
