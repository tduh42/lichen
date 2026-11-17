import MapLibre
import SwiftUI

/// "Map": positions shared with us, ours, SOS, and our shares (mockup `carte`).
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

    var body: some View {
        ZStack(alignment: .top) {
            MapGate {
                MapLibreView(items: items, style: style.resolved(dark: dark), recenter: recenter) { bounds, zoom in
                    camera = (bounds, zoom)
                }
            }
            .ignoresSafeArea(edges: .top)
            topBar
                .padding(.horizontal, 16)
                .padding(.top, 8)
            if let p = offline.progress {
                DownloadBanner(done: p.done, total: p.total)
                    .padding(.top, 64)
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            SharePanel(editing: $editing)
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
            if !items.isEmpty { recenter += 1 }
            #if DEBUG
            if model.debugScreen == "sos" { showingSos = true }
            #endif
        }
        .onDisappear {
            model.needsLocation("map", false)
            refresh?.invalidate()
        }
        .onChange(of: items.filter { $0.kind != .node }.map(\.id)) { _, ids in
            let new = Set(ids).subtracting(seen)
            if !new.isEmpty {
                seen.formUnion(new)
                recenter += 1
            }
        }
    }

    private var dark: Bool { scheme == .dark }

    private var topBar: some View {
        HStack {
            Button { showingSos = true } label: {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                    Text(model.sos ? "SOS on" : "SOS")
                }
                .font(.system(.subheadline, weight: .bold))
                .padding(.horizontal, 18)
                .frame(height: 44)
                .background(Theme.sosBtn, in: Capsule())
                .foregroundStyle(.white)
                .overlay(Capsule().stroke(.white.opacity(0.9), lineWidth: model.sos ? 3 : 0))
            }
            .accessibilityLabel(model.sos ? "SOS on: open" : "SOS")
            Spacer()
            Menu {
                Picker("Map style", selection: $style) {
                    ForEach(MapStyle.allCases) { Text($0.label).tag($0) }
                }
                Toggle(isOn: $showNodes) {
                    Label("MeshCore nodes", systemImage: "antenna.radiowaves.left.and.right")
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
            } label: {
                MapIcon(systemName: "map")
            }
            .accessibilityLabel("Map layers")
            Button { recenter += 1 } label: {
                MapIcon(systemName: "scope", tint: Theme.me)
            }
            .accessibilityLabel("Show everyone")
        }
    }

    // MARK: items

    private var items: [MapItem] {
        let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        var out: [MapItem] = []
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
            for n in model.nodes {
                guard let lat = n.lat, let lon = n.lon else { continue }
                out.append(MapItem(
                    id: "n:" + n.key.hex, kind: .node, lat: lat, lon: lon, radius: 0,
                    initials: "", label: n.name,
                    color: UIColor(n.kind == .repeater ? Theme.lora : Color(hex: 0xD6D6D0)).resolvedColor(with: traits),
                    dark: dark, symbol: n.kind.symbol
                ))
            }
        }
        if let me = model.myFix {
            out.append(MapItem(
                id: "me", kind: .me, lat: me.lat, lon: me.lon, radius: me.accuracyM,
                initials: "", label: "", color: UIColor(Theme.me).resolvedColor(with: traits), dark: dark
            ))
        }
        return out
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
            Capsule()
                .fill(Theme.line)
                .frame(width: 36, height: 5)
                .frame(maxWidth: .infinity, minHeight: 12)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation(.snappy) { expanded.toggle() } }
                .accessibilityAddTraits(.isButton)
                .accessibilityLabel(expanded ? "Collapse" : "Expand")
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Location sharing")
                        .font(.system(.title3, weight: .bold))
                    Text(status)
                        .font(.system(.footnote))
                        .foregroundStyle(model.sos ? Theme.sosFg : Theme.tx2)
                }
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
                            Spacer()
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
                Text("Sent only when you stray more than 50 m from your track. Encryption doesn't hide your radio position.")
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
                    Image(systemName: "chevron.up.chevron.down").font(.system(.caption2))
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
