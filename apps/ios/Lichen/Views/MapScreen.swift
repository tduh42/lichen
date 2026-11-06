import MapLibre
import SwiftUI

/// "Map": positions shared with us, ours, SOS, and our shares (mockup ios-carte).
struct MapScreen: View {
    @Environment(AppModel.self) private var model
    @State private var offline = OfflineMaps()
    @AppStorage("map.style") private var style = MapStyle.liberty
    @State private var recenter = 0
    /// Who was on the map already: someone new reframes the map around everyone.
    @State private var seen: Set<String> = []
    @State private var camera: (MLNCoordinateBounds, Double)?
    @State private var confirmSos = false
    @State private var editing = false
    @State private var refresh: Timer?

    var body: some View {
        ZStack(alignment: .top) {
            MapLibreView(items: items, style: style, recenter: recenter) { bounds, zoom in
                camera = (bounds, zoom)
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
        .confirmationDialog("Send an SOS?", isPresented: $confirmSos, titleVisibility: .visible) {
            Button("Send SOS", role: .destructive) { model.setSos(true) }
        } message: {
            Text("Your exact position goes to every contact you share with and every group, every 2 minutes, until you stop it.")
        }
        .onAppear {
            model.mapVisible = true
            model.refreshPositions()
            refresh = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in
                MainActor.assumeIsolated { model.refreshPositions() }
            }
            seen = Set(items.map(\.id))
            if !items.isEmpty { recenter += 1 }
        }
        .onDisappear {
            model.mapVisible = false
            refresh?.invalidate()
        }
        .onChange(of: items.map(\.id)) { _, ids in
            let new = Set(ids).subtracting(seen)
            if !new.isEmpty {
                seen.formUnion(new)
                recenter += 1
            }
        }
    }

    private var topBar: some View {
        HStack {
            Button {
                if model.sos { model.setSos(false) } else { confirmSos = true }
            } label: {
                Label(model.sos ? "Stop SOS" : "SOS", systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 15, weight: .bold))
                    .labelStyle(SosLabelStyle())
                    .padding(.horizontal, 18)
                    .frame(height: 44)
                    .background(Theme.danger, in: Capsule())
                    .foregroundStyle(.white)
                    .overlay(Capsule().stroke(.white, lineWidth: model.sos ? 3 : 0))
                    .shadow(color: .black.opacity(0.15), radius: 4, y: 2)
            }
            .accessibilityLabel(model.sos ? "Stop SOS" : "Send SOS")
            Spacer()
            Menu {
                Picker("Map style", selection: $style) {
                    ForEach(MapStyle.allCases) { Text($0.label).tag($0) }
                }
                Section {
                    Button {
                        if let (b, z) = camera { offline.download(b, zoom: z, style: style) }
                    } label: {
                        Label("Download this area", systemImage: "arrow.down.circle")
                    }
                    .disabled(camera == nil || offline.progress != nil)
                    if offline.areas > 0 {
                        Button(role: .destructive) { offline.deleteAll() } label: {
                            Label("Delete offline maps (\(offline.areas), \(ByteCountFormatter.string(fromByteCount: Int64(offline.bytes), countStyle: .file)))",
                                  systemImage: "trash")
                        }
                    }
                } footer: {
                    Text("Downloaded areas work without a network.")
                }
            } label: {
                RoundIcon(systemName: "square.3.layers.3d")
            }
            .accessibilityLabel("Map layers")
            Button { recenter += 1 } label: {
                RoundIcon(systemName: "location.fill", tint: Color(hex: 0x1E40AF))
            }
            .accessibilityLabel("Show everyone")
        }
    }

    // MARK: items

    private var items: [MapItem] {
        var out: [MapItem] = []
        for p in model.positions {
            let name = model.name(of: p.from)
            let (_, fg) = Theme.avatar(for: p.from)
            out.append(MapItem(
                id: p.from.hex,
                kind: p.sos ? .sos : .peer,
                lat: p.lat,
                lon: p.lon,
                radius: p.radiusM,
                initials: initials(name),
                label: label(p, name: name),
                color: p.sos ? UIColor(Theme.danger) : UIColor(fg)
            ))
        }
        if let me = model.myFix {
            out.append(MapItem(
                id: "me", kind: .me, lat: me.lat, lon: me.lon, radius: me.accuracyM,
                initials: "", label: "", color: UIColor(Color(hex: 0x1E40AF))
            ))
        }
        return out
    }

    private func initials(_ name: String) -> String {
        let letters = name.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined()
        return letters.isEmpty ? "?" : letters.uppercased()
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

/// Icon then title, tight.
private struct SosLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 8) {
            configuration.icon
            configuration.title
        }
    }
}

private struct RoundIcon: View {
    let systemName: String
    var tint: Color = .primary

    var body: some View {
        Image(systemName: systemName)
            .font(.system(size: 18, weight: .medium))
            .foregroundStyle(tint)
            .frame(width: 44, height: 44)
            .background(Color(.systemBackground), in: Circle())
            .shadow(color: .black.opacity(0.15), radius: 4, y: 2)
    }
}

private struct DownloadBanner: View {
    let done: UInt64
    let total: UInt64

    var body: some View {
        HStack(spacing: 8) {
            ProgressView(value: Double(done), total: Double(max(total, 1)))
                .frame(width: 80)
            Text("Downloading the map: \(Int(done)) / \(Int(total))")
                .font(.caption.weight(.medium))
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.regularMaterial, in: Capsule())
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

/// Bottom card: our shares, their precision, stop (mockup ios-carte).
struct SharePanel: View {
    @Environment(AppModel.self) private var model
    @Binding var editing: Bool
    @State private var expanded = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Capsule()
                .fill(Color(.systemGray4))
                .frame(width: 36, height: 5)
                .frame(maxWidth: .infinity)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation(.snappy) { expanded.toggle() } }
                .accessibilityLabel(expanded ? "Collapse" : "Expand")
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Location sharing")
                        .font(.system(size: 20, weight: .bold))
                    Text(status)
                        .font(.footnote)
                        .foregroundStyle(model.sos ? Theme.danger : Theme.secondaryText)
                }
                Spacer()
                Toggle("On", isOn: Binding(
                    get: { !model.shares.isEmpty },
                    set: { on in if on { editing = true } else { model.stopAllShares() } }
                ))
                .labelsHidden()
                .tint(Theme.ok)
            }
            if expanded {
                if model.locationDenied && !model.isDemo {
                    Button {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(url)
                        }
                    } label: {
                        Label("Location access is off: allow it in Settings.", systemImage: "location.slash")
                            .font(.footnote)
                    }
                }
                if !model.shares.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(model.shares, id: \.chat) { s in
                            ShareRow(share: s)
                                .contentShape(Rectangle())
                                .onTapGesture { editing = true }
                            if s.chat != model.shares.last?.chat {
                                Divider().padding(.leading, 58)
                            }
                        }
                    }
                    .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
                }
                HStack(spacing: 8) {
                    Button { editing = true } label: {
                        Text("Precision per contact")
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
                    }
                    .foregroundStyle(.primary)
                    Button { model.stopAllShares() } label: {
                        Text("Stop sharing")
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .background(Color(hex: 0xFEE2E2), in: RoundedRectangle(cornerRadius: 12))
                    }
                    .foregroundStyle(Color(hex: 0x991B1B))
                    .disabled(model.shares.isEmpty)
                    .opacity(model.shares.isEmpty ? 0.45 : 1)
                }
                .font(.system(size: 15, weight: .semibold))
                Text("Sent only when you stray more than 50 m from your track. Encryption doesn't hide your radio position.")
                    .font(.caption)
                    .foregroundStyle(Theme.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .background(
            Color(.systemBackground),
            in: UnevenRoundedRectangle(topLeadingRadius: 20, topTrailingRadius: 20)
        )
        .shadow(color: .black.opacity(0.08), radius: 8, y: -2)
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
                Text(model.title(of: share.chat))
                    .font(.system(size: 15, weight: .semibold))
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(Theme.secondaryText)
            }
            Spacer()
            Text(PrecisionText.name(share.precision))
                .font(.system(size: 13, weight: .semibold))
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var avatarKey: Data {
        switch share.chat {
        case let .contact(key): key
        case let .group(gid): gid
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
            Form {
                Section {
                    Picker("Duration", selection: $hours) {
                        Text("1 h").tag(1.0)
                        Text("8 h").tag(8.0)
                        Text("24 h").tag(24.0)
                    }
                    .pickerStyle(.segmented)
                } header: {
                    Text("Duration")
                } footer: {
                    Text("Sharing stops on its own at the end; the others see your marker disappear.")
                }
                if !model.contacts.isEmpty {
                    Section("Contacts") {
                        ForEach(model.contacts, id: \.key) { c in
                            row(.contact(key: c.key), name: c.name, key: c.key)
                        }
                    }
                }
                if !model.groups.isEmpty {
                    Section {
                        ForEach(model.groups, id: \.gid) { g in
                            row(.group(gid: g.gid), name: g.name, key: g.gid)
                        }
                    } header: {
                        Text("Groups")
                    } footer: {
                        Text("A group gets at most one position every 5 minutes.")
                    }
                }
                if model.contacts.isEmpty && model.groups.isEmpty {
                    Text("No contacts yet: connect your MeshCore node, or try the demo network in Settings.")
                        .foregroundStyle(Theme.secondaryText)
                }
                Section {
                    Text("A rounded position is the centre of a fixed grid cell, never random noise: averaging many of them reveals nothing more.")
                        .font(.footnote)
                        .foregroundStyle(Theme.secondaryText)
                }
            }
            .navigationTitle("Precision per contact")
            .navigationBarTitleDisplayMode(.inline)
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
            Text(name)
            Spacer()
            Menu {
                Button("Not shared") { set(chat, nil) }
                ForEach([SharePrecision.exact, .m100, .km1, .km10], id: \.self) { p in
                    Button(PrecisionText.name(p)) { set(chat, p) }
                }
            } label: {
                Text(current.map(PrecisionText.name) ?? String(localized: "Not shared"))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(current == nil ? Theme.secondaryText : Theme.accent)
            }
        }
    }

    private func set(_ chat: ChatID, _ p: SharePrecision?) {
        error = model.setShare(chat, precision: p, hours: hours)
    }
}
