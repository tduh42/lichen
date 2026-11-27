import MapLibre
import SwiftUI

/// "Map": positions shared with us, ours, SOS, and our shares (mockup `carte`); and Locate,
/// the search by radio (`SearchView`), its other mode. The SOS button, the mode and the layers
/// stay at the top in both.
struct MapScreen: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme
    @State private var offline = OfflineMaps()
    @AppStorage("map.style") private var style = MapStyle.auto
    /// MeshCore nodes at the positions they advertise (protocol.md §16.1).
    @AppStorage("map.nodes") private var showNodes = true
    @State private var recenter = 0
    /// Who was on the map already: someone new reframes the map around everyone.
    @State private var seen: Set<String> = []
    @State private var camera: (MLNCoordinateBounds, Double)?
    @State private var showingSos = false
    @State private var editing = false
    @State private var refresh: Timer?
    @State private var showingOffline = false
    /// The route's legs whose chip finds no room on the map: said in its card.
    @State private var hiddenLegs: [Int] = []

    private var locating: Bool { model.mapMode == .locate }

    var body: some View {
        ZStack(alignment: .top) {
            if locating {
                // Below the bar, which keeps its place at the top.
                SearchView()
                    .safeAreaInset(edge: .top, spacing: 0) {
                        topBar
                            .padding(.horizontal, 16)
                            .padding(.vertical, 8)
                            .background(Theme.s1)
                    }
            } else {
                MapGate {
                    let route = model.mapRouteTo.flatMap(mapRoute)
                    MapLibreView(items: items, style: style.resolved(dark: dark), recenter: recenter, onCamera: { bounds, zoom in
                        camera = (bounds, zoom)
                    }, focus: focus, route: route, onSelect: { id in
                        // A node tapped: its route (§16.3).
                        if id.hasPrefix("n:"), let key = Data(hex: String(id.dropFirst(2))) { model.mapRouteTo = key }
                    }, onHiddenChips: { hiddenLegs = $0 })
                }
                .ignoresSafeArea(edges: .top)
                VStack(spacing: 8) {
                    topBar
                    if let pin = model.mapPin { PinBar(pin: pin) }
                }
                .padding(.horizontal, 16)
                .padding(.top, 8)
                if let p = offline.progress {
                    DownloadBanner(done: p.done, total: p.total)
                        .padding(.top, 64)
                }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if !locating {
                // A node's route takes the foot of the map; our shares otherwise.
                if let key = model.mapRouteTo {
                    RouteMapCard(key: key, drawn: model.mapRouteTo.flatMap(mapRoute).map { !$0.lines.isEmpty } ?? false,
                                 unlabelled: hiddenLegs.compactMap { i in model.mapRouteTo.flatMap(mapRoute)?.lines[safe: i] })
                } else {
                    SharePanel(editing: $editing)
                }
            }
        }
        .sheet(isPresented: $editing) { ShareSheet() }
        .fullScreenCover(isPresented: $showingSos) { SosView() }
        .sheet(isPresented: $showingOffline) { OfflineMapsSheet(offline: offline) }
        .alert(offline.lastError ?? "", isPresented: Binding(get: { offline.lastError != nil }, set: { if !$0 { offline.lastError = nil } })) {
            Button("OK", role: .cancel) {}
        }
        .onAppear {
            model.needsLocation("map", true)
            model.refreshPositions()
            refresh = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in
                MainActor.assumeIsolated { model.refreshPositions() }
            }
            seen = Set(items.map(\.id))
            // A place from a message, or a node shown from its page, keeps the map on itself.
            if !items.isEmpty, model.mapPin == nil, model.mapFocus == nil, model.mapRouteTo == nil { recenter += 1 }
            #if DEBUG
            if model.debugScreen == "sos" { showingSos = true }
            #endif
        }
        .onDisappear {
            model.needsLocation("map", false)
            refresh?.invalidate()
            // Shown once: the map frames everyone again next time.
            model.mapFocus = nil
        }
        .onChange(of: items.filter { $0.kind != .node }.map(\.id)) { _, ids in
            // A place from a message is centred on by itself (`focus`).
            let new = Set(ids).subtracting(seen).subtracting([Self.placeId])
            if !new.isEmpty {
                seen.formUnion(new)
                recenter += 1
            }
        }
    }

    private var dark: Bool { scheme == .dark }

    /// Where the map centres: the later of a place from a message and a node shown from its
    /// page.
    private var focus: MapPin? {
        [model.mapPin, model.mapFocus].compactMap { $0 }.max { $0.version < $1.version }
    }

    /// SOS, Positions | Locate, the map's buttons: in one row, or with larger text the modes
    /// under the SOS button, their words whole. The same views either way (`SosAndModes`), so
    /// that they keep growing with the text instead of being swapped for others.
    private var topBar: some View {
        HStack(alignment: .top, spacing: 10) {
            SosAndModes(spacing: 10) {
                sosButton
                modePicker
            }
            mapButtons
        }
    }

    private var sosButton: some View {
        Button {
            // Still true after a presentation that failed (tapped while an SOS alert was going
            // away): the button would do nothing. Reset first, then present.
            if showingSos {
                showingSos = false
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { showingSos = true }
            } else {
                showingSos = true
            }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle")
                Text(model.sos ? "SOS on" : "SOS").fixedSize(horizontal: false, vertical: true)
            }
            .font(.system(.subheadline, weight: .bold))
            .padding(.horizontal, 18)
            .padding(.vertical, 6)
            .frame(minHeight: 44)
            .background(Theme.sosBtn, in: Capsule())
            .foregroundStyle(.white)
            .overlay(Capsule().stroke(.white.opacity(0.9), lineWidth: model.sos ? 3 : 0))
        }
        .accessibilityLabel(model.sos ? "SOS on: open" : "SOS")
    }

    private var modePicker: some View {
        ModePicker(mode: Binding(get: { model.mapMode }, set: { model.mapMode = $0 }))
    }

    private var mapButtons: some View {
        VStack(spacing: 10) {
            layers
            if !locating {
                Button { recenter += 1 } label: {
                    MapIcon(systemName: "scope", tint: Theme.me)
                }
                .accessibilityLabel("Show everyone")
            }
        }
    }

    private var layers: some View {
        Menu {
                Picker("Map style", selection: $style) {
                    ForEach(MapStyle.allCases) { Text($0.label).tag($0) }
                }
                Toggle(isOn: $showNodes) {
                    Label("MeshCore nodes", systemImage: "antenna.radiowaves.left.and.right")
                }
                // Which nodes: as the Nodes tab's chips, a choice of the map's own.
                if showNodes {
                    let now = UInt64(Date().timeIntervalSince1970)
                    Picker(selection: Binding(get: { model.mapNodeFilter }, set: { model.mapNodeFilter = $0 })) {
                        ForEach(NodeFilter.allCases) { f in
                            Text(verbatim: f.chip(model.nodes, now: now)).tag(f)
                        }
                    } label: {
                        Text("Which nodes")
                    }
                    .pickerStyle(.inline)
                }
                Section {
                    Button {
                        if let (b, z) = camera { offline.download(b, zoom: z, style: style.resolved(dark: dark)) }
                    } label: {
                        Label("Download this area", systemImage: "arrow.down.circle")
                    }
                    .disabled(camera == nil || offline.progress != nil)
                    if !offline.areas.isEmpty {
                        Button { showingOffline = true } label: {
                            Label("Offline maps (\(offline.areas.count), \(ByteCountFormatter.string(fromByteCount: Int64(offline.bytes), countStyle: .file)))",
                                  systemImage: "square.stack.3d.down.right")
                        }
                    }
                } footer: {
                    Text("Downloaded areas work without a network.")
                }
                // How a node's route reads (§16.3).
                Section("the last signal per leg") {
                    ForEach(RouteLegend.items, id: \.label) { item in
                        Label { Text(verbatim: item.label) } icon: { Image(uiImage: RouteLegend.swatch(item.color, dashed: item.dashed)) }
                    }
                }
            } label: {
                MapIcon(systemName: "map")
            }
            .accessibilityLabel("Map layers")
    }

    // MARK: items

    private static let placeId = "place"

    private var items: [MapItem] {
        let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        var out: [MapItem] = []
        if let pin = model.mapPin {
            out.append(MapItem(
                id: Self.placeId, kind: .place, lat: pin.lat, lon: pin.lon, radius: 0, initials: "",
                label: "", color: UIColor(Theme.me).resolvedColor(with: traits), dark: dark
            ))
        }
        for p in model.positions {
            let name = model.name(of: p.from)
            let color = p.sos ? Theme.sosBtn : Theme.marker(for: p.from)
            out.append(MapItem(
                id: p.from.hex,
                kind: p.sos ? .sos : .peer,
                lat: p.lat,
                lon: p.lon,
                radius: p.radiusM,
                initials: initials(name),
                label: label(p, name: name),
                color: UIColor(color).resolvedColor(with: traits),
                dark: dark
            ))
        }
        if showNodes {
            // The layer's filter, and a route's stops whatever it leaves (§16.3).
            let now = UInt64(Date().timeIntervalSince1970)
            let stops = routeStops ?? []
            for n in model.nodes {
                guard let lat = n.lat, let lon = n.lon,
                      model.mapNodeFilter.keeps(n, now: now) || stops.contains("n:" + n.key.hex) else { continue }
                out.append(MapItem(
                    id: "n:" + n.key.hex, kind: .node, lat: lat, lon: lon, radius: 0,
                    initials: "", label: n.name,
                    color: UIColor(n.kind == .repeater ? Theme.lora : Color(hex: 0xD6D6D0)).resolvedColor(with: traits),
                    dark: dark, symbol: n.kind.symbol,
                    // Heard directly: a ring in its rating's colour (ADR-0049).
                    ring: n.signal == .none ? nil : UIColor(n.signal.color).resolvedColor(with: traits)
                ))
            }
        }
        if let me = model.myFix {
            out.append(MapItem(
                id: "me", kind: .me, lat: me.lat, lon: me.lon, radius: me.accuracyM,
                initials: "", label: "", color: UIColor(Theme.me).resolvedColor(with: traits), dark: dark
            ))
        }
        // A route shown reads only its own nodes: the others fade (us, at its start, stays). An
        // SOS never fades: it stays at full strength over the route.
        if let stops = routeStops {
            for i in out.indices where !stops.contains(out[i].id) && out[i].kind != .sos { out[i].dimmed = true }
        }
        return out
    }

    /// The ids of the map's items that are stops of the route shown: us, each stop's node and
    /// whoever shares a position under its key; nil without a route drawn.
    private var routeStops: Set<String>? {
        guard let key = model.mapRouteTo, let route = model.nodeRoute(key),
              mapRoute(key).map({ !$0.lines.isEmpty }) == true else { return nil }
        var ids: Set<String> = ["me"]
        for s in route.stops {
            guard let k = s.key else { continue }
            ids.insert("n:" + k.hex)
            ids.insert(k.hex)
        }
        return ids
    }

    /// The route the map draws to `key` (§16.3): our radio at the phone, the other stops where
    /// they advertise; a line per leg in its rating's colour, dashed in `line` when nothing
    /// measured it; stops without a position skipped, the stops around them joined by a dashed
    /// line.
    private func mapRoute(_ key: Data) -> MapRoute? {
        guard let route = model.nodeRoute(key) else { return nil }
        let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        let ui = { (c: Color) in UIColor(c).resolvedColor(with: traits) }
        let notMeasured = String(localized: "not measured")
        let names = route.stops.enumerated().map { i, s in i == 0 ? String(localized: "You") : s.display }
        let places: [(Double, Double)?] = route.stops.enumerated().map { i, s in
            if i == 0 { return model.myFix.map { ($0.lat, $0.lon) } }
            if let lat = s.lat, let lon = s.lon { return (lat, lon) }
            return nil
        }
        let text = { (l: RouteLeg) in l.snrDb.map(dbText) ?? notMeasured }
        var lines: [MapRoute.Line] = []
        var prev: Int?
        for (j, place) in places.enumerated() {
            guard let to = place else { continue }
            defer { prev = j }
            guard let i = prev, let from = places[i] else { continue }
            let legs = Array(route.legs[min(i, route.legs.count)..<min(j, route.legs.count)])
            if legs.count == 1, let leg = legs.first {
                let measured = leg.snrDb != nil
                let color = !measured ? Theme.line : leg.signal == .none ? Theme.tx2 : leg.signal.color
                lines.append(MapRoute.Line(
                    fromLat: from.0, fromLon: from.1, toLat: to.0, toLon: to.1, color: ui(color), dashed: !measured,
                    chip: text(leg), chipColor: ui(measured ? color : Theme.tx2),
                    said: String(localized: "\(names[i]) to \(names[j]), \(text(leg))")
                ))
            } else {
                // Stops without a position between: one dashed line across them, its chip each leg's.
                lines.append(MapRoute.Line(
                    fromLat: from.0, fromLon: from.1, toLat: to.0, toLon: to.1, color: ui(Theme.line), dashed: true,
                    chip: legs.map(text).joined(separator: " · "), chipColor: ui(Theme.tx2),
                    said: legs.enumerated().map { k, l in String(localized: "\(names[i + k]) to \(names[i + k + 1]), \(text(l))") }
                        .joined(separator: "; ")
                ))
            }
        }
        return MapRoute(key: key.hex, lines: lines)
    }

    private func label(_ p: PeerPosition, name: String) -> String {
        if p.sos { return "SOS · \(name)" }
        if let coarse = PrecisionText.nominal(radius: p.radiusM) { return "\(name) · ±\(coarse)" }
        return "\(name) · \(Self.ago(p.atS))"
    }

    static func ago(_ unixS: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unixS))
        if Date().timeIntervalSince(date) < 60 { return String(localized: "now") }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f.localizedString(for: date, relativeTo: Date())
    }
}

/// The place from a message on the map (ADR-0034): its coordinates, to copy, and a way to put
/// it away.
private struct PinBar: View {
    let pin: MapPin
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "mappin").foregroundStyle(Theme.me).accessibilityHidden(true)
            Text(verbatim: pin.label)
                .font(.system(.subheadline, design: .monospaced))
                .foregroundStyle(Theme.tx)
                .textSelection(.enabled)
            Spacer(minLength: 0)
            Button { UIPasteboard.general.string = pin.label } label: {
                Image(systemName: "doc.on.doc").frame(width: 32, height: 32)
            }
            .accessibilityLabel("Copy")
            Button { model.mapPin = nil } label: {
                Image(systemName: "xmark").frame(width: 32, height: 32)
            }
            .accessibilityLabel("Remove the pin")
        }
        .foregroundStyle(Theme.tx)
        .padding(.leading, 14)
        .padding(.trailing, 6)
        .frame(height: 44)
        .background(Theme.s2, in: Capsule())
        .shadow(color: .black.opacity(0.25), radius: 4, y: 2)
    }
}

/// Positions | Locate, the Map's two modes: a capsule of two 44 pt segments, the chosen one
/// dark (`actBg`).
/// The SOS button, then the modes centred in the room left beside it; the modes under it
/// when both do not fit on one line (larger text).
private struct SosAndModes: Layout {
    var spacing: CGFloat

    private func arrange(_ width: CGFloat?, _ subviews: Subviews) -> (side: Bool, sos: CGSize, modes: CGSize) {
        guard subviews.count == 2 else { return (true, .zero, .zero) }
        let sos = subviews[0].sizeThatFits(.unspecified)
        let modes = subviews[1].sizeThatFits(.unspecified)
        guard let width, sos.width + spacing + modes.width > width else { return (true, sos, modes) }
        let fitted = { (v: LayoutSubview, w: CGFloat) in v.sizeThatFits(ProposedViewSize(width: w, height: nil)) }
        return (false, fitted(subviews[0], min(sos.width, width)), fitted(subviews[1], min(modes.width, width)))
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        // All the width offered, or what it needs when offered any.
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil }
        let (side, sos, modes) = arrange(width, subviews)
        return side
            ? CGSize(width: width ?? sos.width + spacing + modes.width, height: max(sos.height, modes.height))
            : CGSize(width: width ?? max(sos.width, modes.width), height: sos.height + spacing + modes.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let (side, sos, modes) = arrange(bounds.width, subviews)
        subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(sos))
        if side {
            let room = bounds.width - sos.width - spacing
            let x = bounds.minX + sos.width + spacing + max(0, (room - modes.width) / 2)
            subviews[1].place(at: CGPoint(x: x, y: bounds.minY), proposal: ProposedViewSize(modes))
        } else {
            subviews[1].place(at: CGPoint(x: bounds.minX, y: bounds.minY + sos.height + spacing), proposal: ProposedViewSize(modes))
        }
    }
}

private struct ModePicker: View {
    @Binding var mode: AppModel.MapMode

    var body: some View {
        HStack(spacing: 2) {
            segment(.positions, String(localized: "Positions"))
            segment(.locate, String(localized: "Locate"))
        }
        .padding(3)
        // The shadow under the control, not on its words.
        .background { Capsule().fill(Theme.s2).shadow(color: .black.opacity(0.25), radius: 4, y: 2) }
    }

    private func segment(_ m: AppModel.MapMode, _ title: String) -> some View {
        let on = mode == m
        return Button { mode = m } label: {
            Text(title)
                .font(.system(.subheadline, weight: .semibold))
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .frame(minHeight: 44)
                .background(on ? Theme.actBg : .clear, in: Capsule())
                .foregroundStyle(on ? Theme.actFg : Theme.tx)
                // The whole segment takes the tap, not only its letters.
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// A 44 pt round button on the map (`s2`).
private struct MapIcon: View {
    let systemName: String
    var tint: Color = Theme.tx

    var body: some View {
        Image(systemName: systemName)
            .font(.system(.body, weight: .medium))
            .foregroundStyle(tint)
            .frame(width: 44, height: 44)
            .background(Theme.s2, in: Circle())
            .shadow(color: .black.opacity(0.25), radius: 4, y: 2)
    }
}

/// The areas kept for use without a network, one by one: what they cover, whether they are
/// complete, and what to do when they are not.
private struct OfflineMapsSheet: View {
    let offline: OfflineMaps
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(offline.areas) { a in
                        row(a)
                            .swipeActions {
                                Button("Delete", role: .destructive) { offline.delete(a) }
                            }
                    }
                } footer: {
                    Text("A complete area works without a network. A partial one misses tiles at some zoom levels: resume it while you still have a network.")
                }
            }
            .navigationTitle("Offline maps")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }

    private func row(_ a: OfflineArea) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(a.name).font(.system(.callout, weight: .semibold))
                Spacer()
                badge(a.state)
            }
            Text([a.style.capitalized,
                  ByteCountFormatter.string(fromByteCount: Int64(a.bytes), countStyle: .file),
                  a.created?.formatted(date: .abbreviated, time: .omitted) ?? ""]
                .filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.system(.footnote)).foregroundStyle(Theme.tx2)
            if a.state != .complete, a.total > 0 {
                ProgressView(value: Double(a.done), total: Double(max(a.total, 1)))
                    .tint(Theme.tx)
                Text("\(Int(a.done)) / \(Int(a.total)) tiles and resources")
                    .font(.system(.caption)).foregroundStyle(Theme.tx2)
            }
            if case let .failed(why) = a.state {
                Text(why).font(.system(.footnote)).foregroundStyle(Theme.sosFg)
            }
            HStack(spacing: 16) {
                switch a.state {
                case .downloading:
                    Button("Pause") { offline.pause(a) }
                case .partial:
                    Button("Resume") { offline.resume(a) }
                case .failed:
                    Button("Retry") { offline.resume(a) }
                case .complete:
                    EmptyView()
                }
                Spacer()
                Button("Delete", role: .destructive) { offline.delete(a) }
            }
            .buttonStyle(.borderless)
            .font(.system(.subheadline, weight: .semibold))
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func badge(_ s: OfflineArea.State) -> some View {
        switch s {
        case .complete: Badge(text: String(localized: "complete"), background: Theme.okBg, foreground: Theme.okFg)
        case .downloading: Badge(text: String(localized: "downloading"), background: Theme.irohBg, foreground: Theme.irohFg)
        case .partial: Badge(text: String(localized: "partial"), background: Theme.warnBg, foreground: Theme.warnFg)
        case .failed: Badge(text: String(localized: "failed"), background: Theme.sosBg, foreground: Theme.sosFg)
        }
    }
}

private struct DownloadBanner: View {
    let done: UInt64
    let total: UInt64

    var body: some View {
        HStack(spacing: 8) {
            ProgressView(value: Double(done), total: Double(max(total, 1)))
                .tint(Theme.tx)
                .frame(width: 80)
            Text("Downloading the map: \(Int(done)) / \(Int(total))")
                .font(.system(.caption, weight: .medium))
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Theme.s1, in: Capsule())
    }
}

/// Precision names, and the nominal precision of a received radius.
enum PrecisionText {
    static func name(_ p: SharePrecision) -> String {
        switch p {
        case .exact: String(localized: "Exact")
        case .m100: String(localized: "100 m")
        case .km1: String(localized: "1 km")
        case .km10: String(localized: "10 km")
        }
    }

    /// "1 km" for a position rounded to the 1 km grid (its radius is at least 0.7 cell), nil
    /// for an exact one.
    static func nominal(radius: Double) -> String? {
        if radius >= 7_000 { return name(.km10) }
        if radius >= 700 { return name(.km1) }
        if radius >= 70 { return name(.m100) }
        return nil
    }
}

// MARK: - sharing panel

/// Bottom card: our shares and their precision (mockup `carte`).
struct SharePanel: View {
    @Environment(AppModel.self) private var model
    @Binding var editing: Bool
    @State private var expanded = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // A 44-point target around the small handle, over the card's top padding.
            Button { withAnimation(.snappy) { expanded.toggle() } } label: {
                Capsule()
                    .fill(Theme.line)
                    .frame(width: 36, height: 5)
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.row)
            .padding(.vertical, -14)
            .accessibilityLabel(expanded ? "Collapse" : "Expand")
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Location sharing")
                        .font(.system(.title3, weight: .bold))
                    Text(status)
                        .font(.system(.footnote))
                        .foregroundStyle(model.sos ? Theme.sosFg : Theme.tx2)
                }
                .fixedSize(horizontal: false, vertical: true)
                Spacer()
                Toggle("Location sharing", isOn: Binding(
                    get: { !model.shares.isEmpty },
                    set: { on in if on { editing = true } else { model.stopAllShares() } }
                ))
                .labelsHidden()
                .tint(Theme.ok)
            }
            if expanded {
                if model.locationDenied && !model.isDemo {
                    Banner(kind: .warn, title: String(localized: "Location access is off"),
                           message: String(localized: "Allow it in Settings to share your position."),
                           action: (label: String(localized: "Settings"), run: {
                               if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                           }))
                }
                VStack(spacing: 0) {
                    ForEach(Array(model.shares.enumerated()), id: \.element.chat) { i, s in
                        if i > 0 { CardDivider() }
                        Button { editing = true } label: { ShareRow(share: s) }
                            .buttonStyle(.plain)
                    }
                    if !model.shares.isEmpty { CardDivider() }
                    Button { editing = true } label: {
                        HStack(spacing: 12) {
                            Image(systemName: "plus")
                                .font(.system(.subheadline, weight: .bold))
                                .frame(width: 32, height: 32)
                                .background(Theme.s1, in: Circle())
                            Text(model.shares.isEmpty ? "Share my position…" : "Precision per contact")
                                .font(.system(.callout))
                                .fixedSize(horizontal: false, vertical: true)
                            Spacer()
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
                Text("Sent only when you stray more than 50 m from your track. Encryption doesn’t hide your radio position.")
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 16)
        .background(Theme.s1, in: UnevenRoundedRectangle(topLeadingRadius: 20, topTrailingRadius: 20))
    }

    private var status: String {
        if model.sos { return String(localized: "SOS on: your exact position goes out every 2 minutes") }
        guard let soonest = model.shares.map(\.untilS).min() else { return String(localized: "Off") }
        let left = max(0, TimeInterval(soonest) - Date().timeIntervalSince1970)
        let f = DateComponentsFormatter()
        f.unitsStyle = .abbreviated
        f.allowedUnits = left >= 3600 ? [.hour, .minute] : [.minute]
        return String(localized: "Active · ends in \(f.string(from: left) ?? "")")
    }
}

private struct ShareRow: View {
    @Environment(AppModel.self) private var model
    let share: ShareInfo

    var body: some View {
        HStack(spacing: 12) {
            Avatar(name: model.title(of: share.chat), key: avatarKey, size: 32)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.title(of: share.chat)).font(.system(.callout))
                Text(detail).font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            Spacer()
            Text(PrecisionText.name(share.precision))
                .font(.system(.subheadline, weight: .semibold))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 11)
        .contentShape(Rectangle())
    }

    private var avatarKey: Data {
        switch share.chat {
        case let .contact(key), let .mesh(key), let .room(key): key
        case let .group(gid): gid
        case let .channel(id): id
        }
    }

    private var detail: String {
        switch share.chat {
        case let .contact(key):
            guard let hops = model.contacts.first(where: { $0.key == key })?.hops else {
                return String(localized: "Contact · path not known yet")
            }
            if hops == 0 { return String(localized: "Contact · LoRa direct") }
            return String(localized: "Contact · \(Int(hops)) hops")
        case let .group(gid):
            let n = model.groups.first { $0.gid == gid }?.members.count ?? 0
            return String(localized: "Group · \(n) members · flood")
        // Positions never go to a plain MeshCore chat (§15).
        case .mesh, .channel, .room:
            return ""
        }
    }
}

// MARK: - per-contact precision

/// Who sees our position, at what precision, for how long.
struct ShareSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var hours = 1.0
    @State private var error: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    SectionLabel("Duration")
                    Picker("Duration", selection: $hours) {
                        Text("1 h").tag(1.0)
                        Text("8 h").tag(8.0)
                        Text("24 h").tag(24.0)
                    }
                    .pickerStyle(.segmented)
                    Text("Sharing stops on its own at the end; the others see your marker disappear.")
                        .font(.system(.caption)).foregroundStyle(Theme.tx2)
                    if !model.contacts.isEmpty {
                        SectionLabel("Contacts").padding(.top, 8)
                        CardList {
                            ForEach(Array(model.contacts.enumerated()), id: \.element.key) { i, c in
                                if i > 0 { CardDivider(inset: 60) }
                                row(.contact(key: c.key), name: c.name, key: c.key)
                            }
                        }
                    }
                    if !model.groups.isEmpty {
                        SectionLabel("Groups").padding(.top, 8)
                        CardList {
                            ForEach(Array(model.groups.enumerated()), id: \.element.gid) { i, g in
                                if i > 0 { CardDivider(inset: 60) }
                                row(.group(gid: g.gid), name: g.name, key: g.gid)
                            }
                        }
                        Text("A group gets at most one position every 5 minutes.")
                            .font(.system(.caption)).foregroundStyle(Theme.tx2)
                    }
                    if model.contacts.isEmpty && model.groups.isEmpty {
                        Text("No contacts yet: connect your MeshCore node, or try the demo network in Settings.")
                            .foregroundStyle(Theme.tx2)
                    }
                    Text("A rounded position is the centre of a fixed grid cell, never random noise: averaging many of them reveals nothing more.")
                        .font(.system(.caption))
                        .foregroundStyle(Theme.tx2)
                        .padding(.top, 8)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Precision per contact")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .alert("Could not share", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }

    private func row(_ chat: ChatID, name: String, key: Data) -> some View {
        let current = model.share(of: chat)?.precision
        return HStack(spacing: 12) {
            Avatar(name: name, key: key, size: 32)
            Text(name).font(.system(.callout))
            Spacer()
            Menu {
                Button("Not shared") { set(chat, nil) }
                ForEach([SharePrecision.exact, .m100, .km1, .km10], id: \.self) { p in
                    Button(PrecisionText.name(p)) { set(chat, p) }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(current.map(PrecisionText.name) ?? String(localized: "Not shared"))
                    Image(systemName: "chevron.up.chevron.down").font(.system(.caption))
                }
                .font(.system(.subheadline, weight: .semibold))
                .foregroundStyle(current == nil ? Theme.tx2 : Theme.tx)
                .frame(minHeight: 44)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
    }

    private func set(_ chat: ChatID, _ p: SharePrecision?) {
        error = model.setShare(chat, precision: p, hours: hours)
    }
}

extension Array {
    /// The element at `i`, or nil past the ends.
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}

/// A node's route on the map (mockup `Map`): to whom, how many hops, through which repeaters,
/// when it was traced; the legs whose chip found no room on the map; the legend, or why nothing
/// is drawn; a way to close it.
struct RouteMapCard: View {
    let key: Data
    let drawn: Bool
    let unlabelled: [MapRoute.Line]
    @Environment(AppModel.self) private var model

    var body: some View {
        let node = model.node(key)
        let route = model.nodeRoute(key)
        let hops: String = switch node?.hops {
        case nil: String(localized: "path unknown")
        case 0?: String(localized: "direct")
        case let h?: String(localized: "\(Int(h)) hops")
        }
        let line = [hops, route?.via.map { String(localized: "via \($0)") },
                    route?.tracedAt.map { String(localized: "traced \(model.heard($0))") }].compactMap { $0 }
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(String(localized: "Route to \(node?.name ?? String(key.hex.prefix(8)))"))
                        .font(.system(.headline))
                        .foregroundStyle(Theme.tx)
                        .accessibilityAddTraits(.isHeader)
                    Text(verbatim: line.joined(separator: " · "))
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                Button { model.mapRouteTo = nil } label: {
                    Image(systemName: "xmark")
                        .font(.system(.footnote, weight: .bold))
                        .foregroundStyle(Theme.tx)
                        .frame(width: 36, height: 36)
                        .background(Theme.s2, in: Circle())
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close the route")
            }
            // A leg too short on screen for its chip: said here, in its colour.
            ForEach(Array(unlabelled.enumerated()), id: \.offset) { _, leg in
                // Its words wrap at larger text; read as one static text over the whole row (the
                // audit found it too small to touch, and cut).
                HStack(spacing: 8) {
                    Image(uiImage: RouteLegend.swatch(leg.color, dashed: leg.dashed)).accessibilityHidden(true)
                    Text(verbatim: leg.said.components(separatedBy: ", ").dropLast().joined(separator: ", "))
                        .font(.system(.caption))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(verbatim: leg.chip)
                        .font(.system(.caption, weight: .bold).monospacedDigit())
                        .foregroundStyle(Color(leg.chipColor))
                        .fixedSize()
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(leg.said)
                .accessibilityAddTraits(.isStaticText)
            }
            if drawn {
                RouteLegend()
            } else {
                Text("No positions to draw this route")
                    .font(.system(.footnote))
                    .foregroundStyle(Theme.tx2)
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
        .padding(.vertical, 12)
        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 18))
        .shadow(color: .black.opacity(0.18), radius: 8, y: 2)
        .padding(.horizontal, 16)
        .padding(.bottom, 8)
    }
}

/// How a route's legs read: good, fair, weak, not measured; the last signal per leg.
struct RouteLegend: View {
    struct Item {
        let label: String
        let color: UIColor
        let dashed: Bool
    }

    static var items: [Item] {
        [Item(label: String(localized: "good"), color: UIColor(Theme.ok), dashed: false),
         Item(label: String(localized: "fair"), color: UIColor(Theme.warn), dashed: false),
         Item(label: String(localized: "weak"), color: UIColor(Theme.sos), dashed: false),
         Item(label: String(localized: "not measured"), color: UIColor(Theme.line), dashed: true)]
    }

    /// A short line in a leg's colour, dashed when nothing measured it.
    static func swatch(_ color: UIColor, dashed: Bool) -> UIImage {
        let size = CGSize(width: 18, height: 6)
        let image = UIGraphicsImageRenderer(size: size).image { _ in
            let path = UIBezierPath()
            path.move(to: CGPoint(x: 2, y: 3))
            path.addLine(to: CGPoint(x: 16, y: 3))
            path.lineWidth = 4
            path.lineCapStyle = dashed ? .butt : .round
            if dashed { path.setLineDash([4, 2], count: 2, phase: 0) }
            color.setStroke()
            path.stroke()
        }
        return image.withRenderingMode(.alwaysOriginal)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            // On more lines with larger text.
            FlowLayout(spacing: 12, lineSpacing: 4) {
                ForEach(Self.items, id: \.label) { item in
                    HStack(spacing: 5) {
                        Image(uiImage: Self.swatch(item.color, dashed: item.dashed)).accessibilityHidden(true)
                        Text(verbatim: item.label).font(.system(.caption)).foregroundStyle(Theme.tx2)
                    }
                    .fixedSize()
                }
            }
            Text("the last signal per leg")
                .font(.system(.caption))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }
}

/// Its views side by side, onto the next line when the room runs out.
struct FlowLayout: Layout {
    var spacing: CGFloat = 8
    var lineSpacing: CGFloat = 4

    private func lines(_ width: CGFloat, _ subviews: Subviews) -> [(CGPoint, CGSize)] {
        var x: CGFloat = 0, y: CGFloat = 0, lineHeight: CGFloat = 0
        return subviews.map { v in
            let s = v.sizeThatFits(.unspecified)
            if x > 0 && x + s.width > width {
                x = 0
                y += lineHeight + lineSpacing
                lineHeight = 0
            }
            defer {
                x += s.width + spacing
                lineHeight = max(lineHeight, s.height)
            }
            return (CGPoint(x: x, y: y), s)
        }
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? .infinity
        let placed = lines(width, subviews)
        let w = placed.map { $0.0.x + $0.1.width }.max() ?? 0
        let h = placed.map { $0.0.y + $0.1.height }.max() ?? 0
        return CGSize(width: width.isFinite ? width : w, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (v, (p, s)) in zip(subviews, lines(bounds.width, subviews)) {
            v.place(at: CGPoint(x: bounds.minX + p.x, y: bounds.minY + p.y), proposal: ProposedViewSize(s))
        }
    }
}
