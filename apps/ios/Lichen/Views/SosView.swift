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
        VStack(spacing: 20) {
            HStack {
                Text("Distress alert").font(.system(size: 15, weight: .semibold)).foregroundStyle(Theme.sosFg)
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 16, weight: .bold))
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
                                Text("on").font(.system(size: 15, weight: .medium))
                            }
                            .foregroundStyle(.white)
                        }
                        .padding(26)
                        .overlay(Circle().stroke(Theme.sos, lineWidth: 8))
                    Button("Stop the SOS") { model.setSos(false) }
                        .buttonStyle(.primary)
                }
                .padding(.top, 8)
            } else {
                SosHoldButton { model.setSos(true) }
                    .padding(.top, 8)
            }
            VStack(spacing: 10) {
                card(String(localized: "What is sent"),
                     String(localized: "Your exact GPS position, again every 2 minutes"))
                card(String(localized: "To whom"), recipients)
                card(String(localized: "How"),
                     String(localized: "Over LoRa to each contact you share with, flooded to each group, on the mesh's usual radio settings."))
            }
            Spacer(minLength: 0)
            Text("The SOS stays on until you stop it. It does not replace 112.")
                .font(.system(size: 12))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
        }
        .padding(.horizontal, 24)
        .padding(.top, 16)
        .padding(.bottom, 24)
        .background(SosColor.screen.ignoresSafeArea())
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
            Text(title).font(.system(size: 13)).foregroundStyle(Theme.sosFg)
            Text(text).font(.system(size: 15)).fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(SosColor.card, in: RoundedRectangle(cornerRadius: 14))
    }
}

/// Someone sent an SOS: full-screen alert (mockup `sos-recu`).
struct SosReceivedView: View {
    let sos: PeerPosition
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.system(size: 30, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 72, height: 72)
                    .background(Theme.sosBtn, in: Circle())
                Text("SOS alert · \(MapScreen.ago(sos.atS))")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.sosFg)
                Text(model.name(of: sos.from))
                    .font(Theme.wordmark(40))
                    .tracking(-1)
                Text(via).font(.system(size: 15)).foregroundStyle(Theme.tx2)
            }
            .multilineTextAlignment(.center)
            .padding(.top, 24)
            CardList {
                CardRow(title: String(localized: "Last GPS position"),
                        subtitle: "\(MapScreen.ago(sos.atS)) · ±\(Int(sos.radiusM.rounded())) m · \(String(format: "%.4f, %.4f", sos.lat, sos.lon))")
                CardDivider()
                if let d = model.distanceTo(sos) {
                    CardRow(title: String(localized: "From you")) {
                        Text("\(distanceText(d.meters)) · \(Int(d.bearing))°").font(Theme.figure(18))
                    }
                } else {
                    CardRow(title: String(localized: "From you"), subtitle: String(localized: "Your position is not known yet")) {
                        Badge(text: String(localized: "unknown"), background: Theme.warnBg, foreground: Theme.warnFg)
                    }
                }
                CardDivider()
                CardRow(title: String(localized: "Beacon"), subtitle: String(localized: "Every 2 minutes, until they stop it")) {
                    Badge(text: String(localized: "active"), background: Theme.loraBg, foreground: Theme.loraFg)
                }
            }
            Spacer(minLength: 0)
            VStack(spacing: 8) {
                Button("Start the search") {
                    model.tab = .search
                    model.sosAlert = nil
                }
                .buttonStyle(.primary)
                HStack(spacing: 8) {
                    Button("Reply") {
                        model.tab = .messages
                        model.debugOpen = .contact(key: sos.from)
                        model.sosAlert = nil
                    }
                    .buttonStyle(.secondary)
                    .disabled(!model.contacts.contains { $0.key == sos.from })
                    Button("Show on the map") {
                        model.tab = .map
                        model.sosAlert = nil
                    }
                    .buttonStyle(.secondary)
                }
                Button("Dismiss") { model.sosAlert = nil }
                    .font(.system(size: 15, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 44)
                    .foregroundStyle(Theme.tx2)
            }
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 16)
        .background(SosColor.screen.ignoresSafeArea())
    }

    private var via: String {
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
        guard let me = myFix else { return nil }
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
