import SwiftUI

/// "Locate" (mockup `recherche`): follow an SOS, and search for a group member by radio
/// (protocol.md §11): where they probably are (heat map, 68 % / 95 % ellipses), which way and
/// how far, the receptions everyone shared, and the actions (ping, bearing, lower power).
struct SearchView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme
    @AppStorage("map.style") private var style = MapStyle.auto
    @State private var recenter = 0
    @State private var bearing = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Group {
                if let target {
                    TimelineView(.periodic(from: .now, by: 5)) { _ in content(target) }
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
                    Text(target.map { String(localized: "Locate · \(model.name(of: $0))") } ?? String(localized: "Locate"))
                        .font(.system(.body, weight: .semibold))
                }
            }
            .sheet(isPresented: $bearing) {
                if let target { BearingSheet(target: target) }
            }
            .alert(error ?? "", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
                Button("OK", role: .cancel) {}
            }
        }
        .onAppear {
            model.needsLocation("search", true)
            model.useHeading(true)
            model.refreshSearches()
        }
        .onDisappear {
            model.needsLocation("search", false)
            model.useHeading(false)
        }
        .onChange(of: target) { recenter += 1 }
    }

    private var target: Data? {
        if let t = model.searchTarget, model.searchTargets.contains(t) { return t }
        return model.searchTargets.first
    }

    private var empty: some View {
        VStack(spacing: 12) {
            Image(systemName: "scope").font(.system(size: 44)).foregroundStyle(Theme.tx2)
            Text("Nobody being located").font(.system(.body, weight: .semibold))
            Text("When a member of your groups sends an SOS, follow it here, with or without their GPS. To locate someone else, open a group and choose them.")
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func content(_ t: Data) -> some View {
        let search = model.search(for: t)
        let estimate = search == nil ? nil : model.searchEstimate(t)
        let incident = model.incidents[t]
        return ScrollView {
            VStack(spacing: 12) {
                if model.searchTargets.count > 1 { picker }
                status(t, incident: incident, search: search)
                map(t, estimate: estimate, incident: incident, search: search)
                    .frame(height: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 14))
                pointer(t, estimate: estimate, incident: incident, search: search)
                if search != nil {
                    actions(t, search: search!)
                } else {
                    startCard(t)
                }
                if search != nil { readings(t, estimate: estimate) }
            }
            .padding(16)
        }
    }

    // MARK: parts

    private var picker: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(model.searchTargets, id: \.self) { t in
                    let on = t == target
                    Button {
                        model.searchTarget = t
                    } label: {
                        HStack(spacing: 6) {
                            if let i = model.incidents[t], i.state() != .ended {
                                Circle().fill(i.state() == .active ? Theme.sos : Theme.warn).frame(width: 8, height: 8)
                            }
                            Text(model.name(of: t)).font(.system(.subheadline, weight: .semibold))
                        }
                        .padding(.horizontal, 14)
                        .frame(minHeight: 44)
                        .background(on ? Theme.actBg : Theme.s2, in: Capsule())
                        .foregroundStyle(on ? Theme.actFg : Theme.tx)
                    }
                    .accessibilityAddTraits(on ? .isSelected : [])
                }
            }
        }
    }

    @ViewBuilder
    private func status(_ t: Data, incident: SosIncident?, search: SearchState?) -> some View {
        let battery = (incident?.battery ?? search?.battery).map { String(localized: " · battery \($0) %") } ?? ""
        if let i = incident {
            let heard = MapScreen.ago(UInt64(i.lastHeard.timeIntervalSince1970))
            switch i.state() {
            case .active:
                Banner(kind: .sos, title: String(localized: "SOS · heard \(heard)\(battery)"),
                       message: i.position == nil
                        ? String(localized: "No GPS position: their phone sends beacons every 2 minutes, to be located by radio.")
                        : String(localized: "Their GPS position comes every 2 minutes."))
            case .stale:
                Banner(kind: .warn, title: String(localized: "SOS · nothing heard since \(heard)"),
                       message: String(localized: "Their beacons no longer reach you: move, or ask the group."))
            case .ended:
                Banner(kind: .ok, title: String(localized: "SOS ended"),
                       message: String(localized: "They turned their SOS off."))
            }
        } else if let s = search {
            let heard = s.lastHeardS.map { String(localized: "heard \(MapScreen.ago($0))") } ?? String(localized: "not heard yet")
            Banner(kind: .lora, title: String(localized: "Locating · \(heard)\(battery)"),
                   message: String(localized: "They answer pings in SOS or when they let the group locate them."))
        }
    }

    private func map(_ t: Data, estimate: SearchEstimate?, incident: SosIncident?, search: SearchState?) -> some View {
        let dark = style == .dark || (style == .auto && scheme == .dark)
        let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        var items: [MapItem] = []
        let name = model.name(of: t)
        // Their own position: the latest SOS or search fix.
        if let p = incident?.position {
            items.append(MapItem(id: "target", kind: .sos, lat: p.lat, lon: p.lon, radius: p.radiusM,
                                 initials: initials(name), label: MapScreen.ago(p.atS),
                                 color: UIColor(Theme.sosBtn).resolvedColor(with: traits), dark: dark))
        } else if let f = search?.lastFix {
            items.append(MapItem(id: "target", kind: .sos, lat: f.lat, lon: f.lon, radius: f.radiusM,
                                 initials: initials(name), label: MapScreen.ago(f.atS),
                                 color: UIColor(Theme.sosBtn).resolvedColor(with: traits), dark: dark))
        }
        if let me = model.myFix {
            items.append(MapItem(id: "me", kind: .me, lat: me.lat, lon: me.lon, radius: me.accuracyM,
                                 initials: "", label: "", color: UIColor(Theme.me).resolvedColor(with: traits), dark: dark))
        }
        let overlay = estimate.map { e in
            MapOverlay(version: model.searchVersion &* 31 &+ t.hashValue, heat: e.heatImage(),
                       north: e.heatNorth, south: e.heatSouth, west: e.heatWest, east: e.heatEast,
                       ellipse68: e.ellipse(major: e.major68M, minor: e.minor68M),
                       ellipse95: e.ellipse(major: e.major95M, minor: e.minor95M))
        }
        return MapGate {
            MapLibreView(items: items, style: style.resolved(dark: dark), recenter: recenter, onCamera: { _, _ in },
                         overlay: overlay, insets: UIEdgeInsets(top: 30, left: 30, bottom: 30, right: 30))
        }
            .onAppear { recenter += 1 }
            .onChange(of: estimate != nil) { recenter += 1 }
            .accessibilityLabel(estimate.map { String(localized: "Map: \(name) is probably within \(distanceText($0.major68M)) of the centre") }
                ?? String(localized: "Map"))
    }

    /// Which way and how far: to the estimate, else to their last position.
    private func pointer(_ t: Data, estimate: SearchEstimate?, incident: SosIncident?, search: SearchState?) -> some View {
        let goal: (lat: Double, lon: Double, radius: Double)? = estimate.map { ($0.lat, $0.lon, $0.major68M) }
            ?? incident?.position.map { ($0.lat, $0.lon, $0.radiusM) }
            ?? search?.lastFix.map { ($0.lat, $0.lon, $0.radiusM) }
        let d = goal.flatMap { model.distanceTo(lat: $0.lat, lon: $0.lon) }
        // Inside the 68 % ellipse (a ring around us, two places), there is no direction to give.
        let unknownWay = estimate.map { e in model.myFix.map { e.contains68(lat: $0.lat, lon: $0.lon) } ?? false } ?? false
        let relative = d.flatMap { d in model.heading.map { (d.bearing - $0 + 360).truncatingRemainder(dividingBy: 360) } }
        return HStack(spacing: 16) {
            Image(systemName: unknownWay || goal == nil ? "questionmark" : "location.north.fill")
                .font(.system(.title))
                .foregroundStyle(Theme.sos)
                .rotationEffect(.degrees(unknownWay || goal == nil ? 0 : relative ?? d?.bearing ?? 0))
                .frame(width: 64, height: 64)
                .background(Theme.s2, in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                if unknownWay, let goal {
                    Text("Direction not known yet").font(.system(.body, weight: .semibold))
                    Text("Within \(distanceText(goal.radius)) of here, around you. Move a few hundred metres or add a bearing: other places narrow it down.")
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                } else if let d, let goal {
                    Text("\(Int(d.bearing.rounded()))° · \(distanceText(d.meters))").font(Theme.figure(28))
                    Text(relative != nil
                         ? String(localized: "The arrow follows your phone · ±\(distanceText(goal.radius))")
                         : String(localized: "Bearing from north · ±\(distanceText(goal.radius))"))
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                } else if goal == nil {
                    Text("Position unknown").font(.system(.body, weight: .semibold))
                    Text("Start locating: every reception of their radio narrows it down.")
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                } else {
                    Text("Waiting for your position").font(.system(.body, weight: .semibold))
                    Text("The bearing and the distance appear with your GPS fix.")
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                }
            }
            Spacer(minLength: 0)
        }
        .card(padding: 16)
        .accessibilityElement(children: .combine)
    }

    private func startCard(_ t: Data) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Locate by radio").font(.system(.body, weight: .semibold))
            Text("Your radio measures each of their transmissions it hears directly, where you stand; the group shares its measures and every phone works out where they probably are.")
                .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            Button("Start locating") {
                error = model.startSearch(t)
            }
            .buttonStyle(.primary)
            .disabled(model.searchGroup(for: t) == nil)
            if model.searchGroup(for: t) == nil {
                Text("Locating by radio needs a group you share with them.")
                    .font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
        }
        .card(padding: 16)
    }

    private func actions(_ t: Data, search: SearchState) -> some View {
        let wait = Int((Double(search.pingInMs) / 1000).rounded(.up))
        let gid = search.gid
        let cost = model.estimate(.group(gid: gid), datagramLen: 8).map { String(format: "%.0f", Double($0.airtimeMs) / 1000) }
        return VStack(spacing: 8) {
            HStack(spacing: 8) {
                Button {
                    error = model.ping(t)
                } label: {
                    VStack(spacing: 2) {
                        Text("Ping")
                        Text(wait > 0 ? String(localized: "in \(wait) s") : cost.map { String(localized: "≈ \($0) s air") } ?? "")
                            .font(.system(.caption2, weight: .regular)).foregroundStyle(Theme.tx2)
                    }
                }
                .buttonStyle(SecondaryButtonStyle(height: 56))
                .disabled(wait > 0 || !model.isConnected)
                Button("Add a bearing") { bearing = true }
                    .buttonStyle(SecondaryButtonStyle(height: 56))
                    .disabled(model.myFix == nil)
                Menu {
                    Button("10 dBm for 10 min") { error = model.lowerPower(t, dbm: 10) }
                    Button("2 dBm for 10 min") { error = model.lowerPower(t, dbm: 2) }
                } label: {
                    Text("Lower power")
                        .font(.system(.subheadline, weight: .semibold))
                        .frame(maxWidth: .infinity, minHeight: 56)
                        .background(Theme.s2, in: RoundedRectangle(cornerRadius: 14))
                        .foregroundStyle(Theme.tx)
                }
                .disabled(!model.isConnected)
            }
            HStack(spacing: 8) {
                if let url = model.searchCSV(t) {
                    ShareLink(item: url) {
                        Label("Export", systemImage: "square.and.arrow.up")
                            .font(.system(.subheadline, weight: .semibold))
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    .foregroundStyle(Theme.tx)
                }
                Button(role: .destructive) {
                    model.stopSearch(t)
                } label: {
                    Text("Stop locating")
                        .font(.system(.subheadline, weight: .semibold))
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
            }
            if let tx = search.txDbm {
                Text("Their radio transmits at \(tx) dBm.")
                    .font(.system(.caption)).foregroundStyle(Theme.tx2)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func readings(_ t: Data, estimate: SearchEstimate?) -> some View {
        let ms = model.measurements(t)
        return VStack(alignment: .leading, spacing: 6) {
            SectionLabel(LocalizedStringKey(estimate.map {
                $0.calibrated
                    ? String(localized: "\(ms.count) measures · range calibrated on their GPS")
                    : String(localized: "\(ms.count) measures")
            } ?? String(localized: "No measure yet")))
            if !ms.isEmpty {
                CardList {
                    ForEach(Array(ms.prefix(20).enumerated()), id: \.offset) { i, m in
                        if i > 0 { CardDivider() }
                        MeasurementRow(who: m.mine ? String(localized: "You") : model.name(of: m.by),
                                       detail: detail(m), value: value(m))
                    }
                }
            }
        }
    }

    private func detail(_ m: SearchMeasurement) -> String {
        var parts = [MapScreen.ago(m.atS)]
        if m.rssi != nil { parts.append(String(format: "SNR %.0f dB", m.snrDb)) }
        if m.estimated { parts.append(m.saturated ? String(localized: "estimated, saturated") : String(localized: "estimated")) }
        if let b = m.bearingDeg {
            parts.append(m.bearingYagi ? String(localized: "bearing \(b)° (antenna)") : String(localized: "bearing \(b)° (body)"))
        }
        return parts.joined(separator: " · ")
    }

    private func value(_ m: SearchMeasurement) -> String {
        if let r = m.rssi { return "\(r) dBm" }
        if let b = m.bearingDeg { return "\(b)°" }
        return "—"
    }
}

/// A bearing: face where the signal seems strongest (the last readings help), and keep it.
struct BearingSheet: View {
    let target: Data
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var yagi = false
    @State private var manual = 0.0
    @State private var error: String?

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 16) {
                Text("Face where their signal seems strongest (the last readings below help), or point a directional antenna at it, then keep this direction.")
                    .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    VStack(spacing: 6) {
                        Image(systemName: "location.north.line.fill")
                            .font(.system(size: 44))
                            .foregroundStyle(Theme.sos)
                            .accessibilityHidden(true)
                        Text("\(Int(direction.rounded()))°").font(Theme.figure(40))
                        Text(model.heading == nil ? String(localized: "No compass: set it by hand") : String(localized: "From the compass"))
                            .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    }
                    Spacer()
                }
                if model.heading == nil {
                    Slider(value: $manual, in: 0...359, step: 1)
                        .accessibilityLabel("Direction")
                }
                if let m = model.measurements(target).first(where: { $0.mine && $0.rssi != nil }), let r = m.rssi {
                    Text("Your last reading: \(r) dBm, \(MapScreen.ago(m.atS))")
                        .font(.system(.subheadline))
                }
                Toggle("Directional antenna", isOn: $yagi)
                Text(yagi ? String(localized: "Trusted to ±15°.") : String(localized: "Trusted to ±40° (the body shadows the signal)."))
                    .font(.system(.caption)).foregroundStyle(Theme.tx2)
                if let error { Banner(kind: .sos, title: error) }
                Spacer()
                Button("Use this direction") {
                    error = model.addBearing(target, degrees: direction, yagi: yagi)
                    if error == nil { dismiss() }
                }
                .buttonStyle(.primary)
            }
            .padding(16)
            .screen()
            .navigationTitle("Add a bearing")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
        .presentationDetents([.large])
    }

    private var direction: Double { model.heading ?? manual }
}
