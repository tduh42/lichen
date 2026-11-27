import SwiftUI

/// LoRa presets with a firm source (ADR-0032): MeshCore's FAQ for USA/Canada, the EU/UK narrow
/// preset its communities use (869.618 MHz, as stock radios ship in Europe). Other regions:
/// the values the local community gives, by hand.
enum RadioPreset: String, CaseIterable, Identifiable {
    case euNarrow, usCanada, custom
    var id: Self { self }

    var label: String {
        switch self {
        case .euNarrow: String(localized: "EU/UK (narrow)")
        case .usCanada: String(localized: "USA/Canada")
        case .custom: String(localized: "Custom")
        }
    }

    /// Frequency (kHz), bandwidth (Hz), spreading factor, coding rate.
    var values: (UInt32, UInt32, UInt8, UInt8)? {
        switch self {
        case .euNarrow: (869_618, 62_500, 8, 8)
        case .usCanada: (910_525, 62_500, 7, 5)
        case .custom: nil
        }
    }

    static func matching(_ r: RadioSettings) -> RadioPreset {
        allCases.first { p in
            p.values.map { $0 == (r.freqKhz, r.bwHz, r.sf, r.cr) } ?? false
        } ?? .custom
    }
}

/// The radio's own settings (protocol §1.1, ADR-0032): read from the radio at each connection
/// and after each change; the radio keeps them, lichen stores none.
///
/// Nothing to compare, on purpose: pushed while the app starts (`-screen radio`, as the UI
/// tests open it), the navigation stack built this view over and over, and SwiftUI found it
/// changed every time: the whole screen's body, hundreds of times in about half the launches,
/// thousands on a busy Mac, the screen blank for 20 s to minutes. This view is the same every
/// time: `RadioSettingsContent` is built again only when what it reads changes.
struct RadioSettingsView: View {
    var body: some View {
        RadioSettingsContent()
            #if DEBUG
            .overlay(alignment: .bottomLeading) { RadioSettingsBuilds() }
            #endif
    }
}

#if DEBUG
/// How many times Radio settings' body ran since launch.
@MainActor private var radioSettingsBuilds = 0

/// UI tests (`-countBuilds YES`, RL-5): that count, read twice a second, nearly transparent:
/// a handful of builds, not thousands.
private struct RadioSettingsBuilds: View {
    var body: some View {
        if UserDefaults.standard.bool(forKey: "countBuilds") {
            TimelineView(.periodic(from: .now, by: 0.5)) { _ in
                Text(verbatim: "\(radioSettingsBuilds)")
                    .font(.caption)
                    .opacity(0.01)
                    .accessibilityIdentifier("radio.builds")
            }
        }
    }
}
#endif

private struct RadioSettingsContent: View {
    @Environment(AppModel.self) private var model
    @State private var preset = RadioPreset.custom
    @State private var freqText = ""
    @State private var bwHz: UInt32 = 62_500
    @State private var sf: UInt8 = 8
    @State private var cr: UInt8 = 8
    @State private var gpsInterval: UInt32?
    @State private var pinText = ""
    @State private var pinSaved = false
    @State private var confirmingParams = false
    @State private var confirmingPosition = false
    @State private var confirmingTelemetry: UInt8?
    @State private var confirmingReboot = false
    /// The search's text: rows whose title or section holds it, to jump to.
    @State private var query = ""
    @FocusState private var searching: Bool
    /// The row a search result led to, lit for a moment.
    @State private var highlighted: String?

    static let bandwidths: [UInt32] = [7_800, 10_400, 15_600, 20_800, 31_250, 41_700, 62_500, 125_000, 250_000, 500_000]
    static let intervals: [UInt32] = [60, 300, 900, 3_600]
    /// The kinds of radio added by themselves when not every one is (the firmware's mask).
    static var kinds: [(bit: UInt8, title: String, icon: String)] {
        [(0x02, String(localized: "People (companions)"), "person.2"),
         (0x04, String(localized: "Repeaters"), "antenna.radiowaves.left.and.right"),
         (0x08, String(localized: "Room servers"), "bubble.left.and.bubble.right"),
         (0x10, String(localized: "Sensors"), "sensor")]
    }

    var body: some View {
        #if DEBUG
        let _ = radioSettingsBuilds += 1
        #endif
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    if let radio = model.radioSettings, radio.read, model.isConnected {
                        // While a setting waits for the radio, the value asked for (B-user-5).
                        let r = model.radioAsked ?? radio
                        RadioHeader(settings: r)
                        searchField(r, proxy)
                        if query.trimmingCharacters(in: .whitespaces).isEmpty {
                            lora(r)
                            power(r)
                            positionAndGps(r)
                            readings(r)
                            if r.autoaddMask != nil { contacts(r) }
                            delivery(r)
                            bluetooth(r)
                            device(r)
                        } else {
                            results(r, proxy)
                        }
                    } else if model.isConnected {
                        // Connected, the radio has not answered yet: no zeros shown as its values (D-9).
                        HStack(spacing: 10) {
                            ProgressView()
                            Text("Reading the radio’s settings…").foregroundStyle(Theme.tx2)
                        }
                        .padding(.top, 24)
                    } else {
                        Text("Connect a radio to see its settings.")
                            .foregroundStyle(Theme.tx2)
                            .padding(.top, 24)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 24)
            }
            .scrollDismissesKeyboard(.immediately)
        }
        .background(Theme.bg)
        .navigationTitle("Radio settings")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            model.refreshRadio()
            load()
        }
        .onChange(of: model.radioSettings) { load() }
        .alert("Not changed", isPresented: Binding(get: { model.radioError != nil }, set: { if !$0 { model.radioError = nil } })) {
            Button("OK") { model.radioError = nil }
        } message: {
            Text(model.radioError ?? "")
        }
    }

    /// The editable copies follow what the radio reports.
    private func load() {
        guard let r = model.radioSettings else { return }
        preset = .matching(r)
        freqText = mhz(r.freqKhz)
        bwHz = r.bwHz
        sf = r.sf
        cr = r.cr
        if let s = r.gpsIntervalS { gpsInterval = s }
    }

    // MARK: Search

    /// A row the search finds: its title and its section, in the app's language.
    struct Entry: Identifiable {
        let id: String
        let title: String
        let section: String
        let icon: String
        var tint = IconTile.Tint.neutral
    }

    /// The rows shown for these settings, in their order on screen.
    private func entries(_ r: RadioSettings) -> [Entry] {
        let lora = String(localized: "LoRa")
        let power = String(localized: "Transmit power")
        let position = String(localized: "Position and GPS")
        let readings = String(localized: "Readings")
        let contacts = String(localized: "New contacts")
        let delivery = String(localized: "Delivery")
        let bluetooth = String(localized: "Bluetooth")
        let radio = String(localized: "This radio")
        var e = [Entry(id: "lora", title: String(localized: "Now"), section: lora, icon: "antenna.radiowaves.left.and.right", tint: .lora),
                 Entry(id: "preset", title: String(localized: "Preset"), section: lora, icon: "list.bullet")]
        if preset == .custom {
            e += [Entry(id: "frequency", title: String(localized: "Frequency (MHz)"), section: lora, icon: "waveform"),
                  Entry(id: "bandwidth", title: String(localized: "Bandwidth"), section: lora, icon: "arrow.left.and.right"),
                  Entry(id: "sf", title: String(localized: "Spreading factor"), section: lora, icon: "dial.medium"),
                  Entry(id: "cr", title: String(localized: "Coding rate"), section: lora, icon: "number")]
        }
        e += [Entry(id: "apply", title: String(localized: "Apply"), section: lora, icon: "checkmark.circle"),
              Entry(id: "power", title: power, section: power, icon: "bolt")]
        if r.gps != nil {
            e.append(Entry(id: "gps", title: String(localized: "The radio’s GPS"), section: position, icon: "scope"))
            if r.gps == true { e.append(Entry(id: "gps_interval", title: String(localized: "A fix every"), section: position, icon: "timer")) }
        }
        e.append(Entry(id: "position", title: String(localized: "Position in adverts"), section: position, icon: "mappin.and.ellipse"))
        if r.positionInAdverts {
            e.append(Entry(id: "position_advertised", title: String(localized: "Position advertised"), section: position, icon: "mappin"))
            if usesPhone(r), model.myFix != nil {
                e.append(Entry(id: "use_phone", title: String(localized: "Use this phone’s position"), section: position, icon: "location"))
            }
        }
        e += [Entry(id: "telemetry_base", title: String(localized: "Telemetry: battery and status"), section: readings, icon: "battery.100"),
              Entry(id: "telemetry_location", title: String(localized: "Telemetry: position"), section: readings, icon: "location.circle"),
              Entry(id: "telemetry_environment", title: String(localized: "Telemetry: sensors"), section: readings, icon: "thermometer.medium")]
        if r.autoaddMask != nil {
            e.append(Entry(id: "manual_add", title: String(localized: "Add every radio heard"), section: contacts, icon: "person.badge.plus"))
            if r.manualAdd {
                e += Self.kinds.map { Entry(id: "autoadd_\($0.bit)", title: $0.title, section: contacts, icon: $0.icon) }
            }
            e += [Entry(id: "autoadd_1", title: String(localized: "Replace the oldest when full"), section: contacts, icon: "arrow.triangle.2.circlepath"),
                  Entry(id: "autoadd_hops", title: String(localized: "From how far"), section: contacts, icon: "arrow.up.left.and.arrow.down.right")]
        }
        e += [Entry(id: "multi_acks", title: String(localized: "Extra acknowledgements"), section: delivery, icon: "checkmark.bubble"),
              Entry(id: "pin", title: String(localized: "PIN"), section: bluetooth, icon: "lock"),
              Entry(id: "save_pin", title: String(localized: "Save the PIN"), section: bluetooth, icon: "square.and.arrow.down"),
              Entry(id: "restart", title: String(localized: "Restart the radio"), section: bluetooth, icon: "arrow.clockwise")]
        if r.storageUsedKb != nil, (r.storageTotalKb ?? 0) > 0 {
            e.append(Entry(id: "storage", title: String(localized: "Storage"), section: radio, icon: "internaldrive"))
        }
        if model.nodeKey != nil {
            e += [Entry(id: "key", title: String(localized: "MeshCore public key"), section: radio, icon: "key"),
                  Entry(id: "share_key", title: String(localized: "Share the key"), section: radio, icon: "square.and.arrow.up")]
        }
        return e
    }

    /// The rows whose title or section holds every word typed, in the app's language, without
    /// minding case or accents.
    private func matches(_ r: RadioSettings) -> [Entry] {
        let words = query.split(whereSeparator: \.isWhitespace).map(String.init)
        return entries(r).filter { e in
            words.allSatisfy { w in
                [e.title, e.section].contains { $0.range(of: w, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
            }
        }
    }

    private func searchField(_ r: RadioSettings, _ proxy: ScrollViewProxy) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(Theme.tx2).accessibilityHidden(true)
            // On more lines with larger text rather than cut; Search (Return) puts the keyboard
            // away, the rows found staying.
            TextField("Search radio settings", text: $query, axis: .vertical)
                .lineLimit(1...3)
                .focused($searching)
                .submitLabel(.search)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .onSubmit { searching = false }
                .onChange(of: query) { _, q in
                    if q.contains("\n") {
                        query = q.replacingOccurrences(of: "\n", with: "")
                        searching = false
                    }
                }
            if !query.isEmpty {
                Button { query = "" } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(Theme.tx2)
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear the search")
            }
        }
        .padding(.leading, 12)
        .padding(.trailing, query.isEmpty ? 12 : 0)
        .frame(minHeight: 44)
        .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
        .padding(.top, 4)
    }

    /// The rows found: a tap leads to the row itself, lit for a moment.
    @ViewBuilder private func results(_ r: RadioSettings, _ proxy: ScrollViewProxy) -> some View {
        let found = matches(r)
        if found.isEmpty {
            Text("No setting matches")
                .font(.footnote)
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
                .padding(.top, 10)
        } else {
            CardList {
                ForEach(Array(found.enumerated()), id: \.element.id) { i, e in
                    if i > 0 { CardDivider.tiled }
                    Button { go(to: e, proxy) } label: {
                        CardRow(title: e.title, subtitle: e.section == e.title ? nil : e.section, icon: e.icon, tint: e.tint) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2).accessibilityHidden(true)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.top, 10)
        }
    }

    /// Back to every row, scrolled to this one, which is lit for a moment.
    private func go(to entry: Entry, _ proxy: ScrollViewProxy) {
        searching = false
        query = ""
        Task { @MainActor in
            // The sections come back first.
            try? await Task.sleep(for: .milliseconds(100))
            withAnimation(.easeInOut(duration: 0.3)) { proxy.scrollTo(entry.id, anchor: .center) }
            highlighted = entry.id
            try? await Task.sleep(for: .seconds(1.6))
            if highlighted == entry.id { withAnimation(.easeOut(duration: 0.6)) { highlighted = nil } }
        }
    }

    // MARK: Sections

    /// What the header leaves out: its storage, its MeshCore key and code (its model, firmware
    /// and battery are in the header).
    @ViewBuilder private func device(_ r: RadioSettings) -> some View {
        let storage: (UInt32, UInt32)? = {
            if let used = r.storageUsedKb, let total = r.storageTotalKb, total > 0 { return (used, total) }
            return nil
        }()
        VStack(alignment: .leading, spacing: 8) {
            if storage != nil || model.nodeKey != nil {
                SectionLabel("This radio").padding(.top, 10)
                CardList {
                    if let storage {
                        CardRow(title: String(localized: "Storage"),
                                subtitle: String(localized: "\(Int(storage.0)) of \(Int(storage.1)) KB used"), icon: "internaldrive")
                            .setting("storage", lit: highlighted)
                        if model.nodeKey != nil { CardDivider.tiled }
                    }
                    // Its MeshCore public key, as the stock app shows it (F-1).
                    if let key = model.nodeKey {
                        MeshCoreKeyRow(key: key, icon: "key")
                            .setting("key", lit: highlighted)
                        CardDivider.tiled
                        ShareLink(item: key.hex) {
                            CardRow(title: String(localized: "Share the key"), icon: "square.and.arrow.up")
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .setting("share_key", lit: highlighted)
                    }
                }
            }
            // The stock app's contact code for this radio (F-1, §15.3): a MeshCore user adds it.
            if let key = model.nodeKey,
               let code = try? meshcoreContactCode(name: r.name.isEmpty ? model.nodeName : r.name, key: key, kind: .companion),
               let image = QRCode.image(code) {
                SectionLabel("MeshCore code").padding(.top, 10)
                VStack(spacing: 10) {
                    Image(uiImage: image)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .padding(12)
                        .background(.white, in: RoundedRectangle(cornerRadius: 14))
                        .frame(maxWidth: 220)
                        .accessibilityLabel("MeshCore code of this radio")
                    Text("Someone with the stock MeshCore app can scan it to add this radio.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity)
                .card()
            }
        }
    }

    private func lora(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("LoRa").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Now"), subtitle: summary(r.freqKhz, r.bwHz, r.sf, r.cr),
                        icon: "antenna.radiowaves.left.and.right", tint: .lora)
                    .setting("lora", lit: highlighted)
                CardDivider.tiled
                CardRow(title: String(localized: "Preset"), icon: "list.bullet") {
                    Picker("Preset", selection: $preset) {
                        ForEach(RadioPreset.allCases) { Text($0.label).tag($0) }
                    }
                    .labelsHidden()
                }
                .setting("preset", lit: highlighted)
                if preset == .custom {
                    CardDivider.tiled
                    CardRow(title: String(localized: "Frequency (MHz)"), icon: "waveform") {
                        // As wide as its figures, larger text included.
                        TextField("869.618", text: $freqText)
                            .keyboardType(.decimalPad)
                            .multilineTextAlignment(.trailing)
                            .fixedSize(horizontal: true, vertical: false)
                            .frame(minWidth: 110, alignment: .trailing)
                    }
                    .setting("frequency", lit: highlighted)
                    CardDivider.tiled
                    CardRow(title: String(localized: "Bandwidth"), icon: "arrow.left.and.right") {
                        Picker("Bandwidth", selection: $bwHz) {
                            ForEach(Self.bandwidths, id: \.self) { Text(bandwidth($0)).tag($0) }
                        }
                        .labelsHidden()
                    }
                    .setting("bandwidth", lit: highlighted)
                    CardDivider.tiled
                    CardRow(title: String(localized: "Spreading factor"), icon: "dial.medium") {
                        Picker("Spreading factor", selection: $sf) {
                            ForEach(Array(UInt8(5)...UInt8(12)), id: \.self) { Text(verbatim: "SF\($0)").tag($0) }
                        }
                        .labelsHidden()
                    }
                    .setting("sf", lit: highlighted)
                    CardDivider.tiled
                    CardRow(title: String(localized: "Coding rate"), icon: "number") {
                        Picker("Coding rate", selection: $cr) {
                            ForEach(Array(UInt8(5)...UInt8(8)), id: \.self) { Text(verbatim: "4/\($0)").tag($0) }
                        }
                        .labelsHidden()
                    }
                    .setting("cr", lit: highlighted)
                }
                CardDivider.tiled
                Button { confirmingParams = true } label: {
                    CardRow(title: String(localized: "Apply"), icon: "checkmark.circle")
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(chosen.map { $0 == (r.freqKhz, r.bwHz, r.sf, r.cr) } ?? true)
                .setting("apply", lit: highlighted)
                RadioRowStatus(row: "lora")
            }
            Text("A radio on other settings no longer hears the radios of its mesh: choose your region’s preset, or the values your local community uses.")
                .font(.footnote)
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
        }
        .alert("Change the LoRa settings?", isPresented: $confirmingParams) {
            Button("Apply", role: .destructive) {
                guard let (f, b, s, c) = chosen else { return }
                model.radioSet(.params, row: "lora", asked: { $0.freqKhz = f; $0.bwHz = b; $0.sf = s; $0.cr = c }) {
                    try model.core.setRadioParams(freqKhz: f, bwHz: b, sf: s, cr: c)
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The radio will only hear radios on \(chosen.map { summary($0.0, $0.1, $0.2, $0.3) } ?? ""). Now: \(summary(r.freqKhz, r.bwHz, r.sf, r.cr)).")
        }
    }

    private func power(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Transmit power").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "\(Int(r.txDbm)) dBm"),
                        subtitle: String(localized: "Up to \(Int(r.maxTxDbm)) dBm on this radio. Less reaches less far and saves its battery."),
                        icon: "bolt") {
                    Stepper("Transmit power",
                            value: Binding(get: { Int(r.txDbm) },
                                           set: { v in
                                               model.radioSet(.txPower, row: "power", asked: { $0.txDbm = Int8(v) }) {
                                                   try model.core.setTxPower(dbm: Int8(v))
                                               }
                                           }),
                            in: -9...Int(r.maxTxDbm))
                        .labelsHidden()
                }
                .setting("power", lit: highlighted)
                RadioRowStatus(row: "power")
            }
        }
    }

    /// The radio's GPS and the position its adverts carry, in one card: the GPS first.
    private func positionAndGps(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Position and GPS").padding(.top, 10)
            CardList {
                if r.gps != nil {
                    CardRow(title: String(localized: "The radio’s GPS"),
                            subtitle: String(localized: "Keeps the radio’s position current. It draws power."), icon: "scope") {
                        Toggle("The radio’s GPS",
                               isOn: Binding(get: { r.gps == true },
                                             set: { on in
                                                 model.radioSet(.gps, row: "gps", asked: { $0.gps = on }) { try model.core.setGps(on: on) }
                                             }))
                            .labelsHidden()
                    }
                    .setting("gps", lit: highlighted)
                    RadioRowStatus(row: "gps")
                    if r.gps == true {
                        CardDivider.tiled
                        CardRow(title: String(localized: "A fix every"), icon: "timer") {
                            Picker("A fix every",
                                   selection: Binding(get: { gpsInterval },
                                                      set: { s in
                                                          gpsInterval = s
                                                          if let s {
                                                              model.radioSet(.gpsInterval, row: "gps_interval", asked: { $0.gpsIntervalS = s }) {
                                                                  try model.core.setGpsInterval(seconds: s)
                                                              }
                                                          }
                                                      })) {
                                Text("Not set").tag(UInt32?.none)
                                ForEach(Self.intervals, id: \.self) { s in
                                    Text(Duration.seconds(Int64(s)).formatted(.units(allowed: [.hours, .minutes], width: .wide))).tag(UInt32?.some(s))
                                }
                            }
                            .labelsHidden()
                        }
                        .setting("gps_interval", lit: highlighted)
                        RadioRowStatus(row: "gps_interval")
                    }
                    CardDivider.tiled
                }
                CardRow(title: String(localized: "Position in adverts"),
                        subtitle: r.positionInAdverts
                            ? String(localized: "Every advert shows the radio’s position to the whole mesh, unencrypted.")
                            : String(localized: "Off: adverts carry no position."),
                        icon: "mappin.and.ellipse") {
                    Toggle("Position in adverts",
                           isOn: Binding(get: { r.positionInAdverts },
                                         set: { on in if on { confirmingPosition = true } else { setPrefs(r, row: "position", position: false) } }))
                        .labelsHidden()
                }
                .setting("position", lit: highlighted)
                RadioRowStatus(row: "position")
                if r.positionInAdverts {
                    CardDivider.tiled
                    CardRow(title: String(localized: "Position advertised"), subtitle: position(r), icon: "mappin")
                        .setting("position_advertised", lit: highlighted)
                    // Why there is none: the GPS is on but has no fix (it needs open sky).
                    if r.gps == true && r.latE6 == 0 && r.lonE6 == 0 {
                        Text("The radio’s GPS has no fix yet: it needs open sky. Until then, it can advertise this phone’s position.")
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.leading, 58)
                            .padding(.trailing, 16)
                            .padding(.bottom, 11)
                    }
                    if usesPhone(r), let fix = model.myFix {
                        CardDivider.tiled
                        Button {
                            let lat = Int32((fix.lat * 1e6).rounded())
                            let lon = Int32((fix.lon * 1e6).rounded())
                            model.radioSet(.advertPosition, row: "advert_position", asked: { $0.latE6 = lat; $0.lonE6 = lon }) {
                                try model.core.setAdvertPosition(latE6: lat, lonE6: lon)
                            }
                        } label: {
                            CardRow(title: String(localized: "Use this phone’s position"), icon: "location")
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .setting("use_phone", lit: highlighted)
                        RadioRowStatus(row: "advert_position")
                    }
                }
            }
        }
        // The phone's location runs while its position can be given to the radio, as on the map.
        .onAppear { if usesPhone(r) { model.needsLocation("radio", true) } }
        .onChange(of: usesPhone(r)) { _, on in model.needsLocation("radio", on) }
        .onDisappear { model.needsLocation("radio", false) }
        .alert("Show your position to the whole mesh?", isPresented: $confirmingPosition) {
            Button("Show it", role: .destructive) { setPrefs(r, row: "position", position: true) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Every advert of your radio will carry its position, unencrypted: anyone with a MeshCore app nearby sees where it is.")
        }
    }

    /// A radio advertising its position takes this phone's when it has none of its own: without
    /// GPS, or with its GPS on but no fix yet, indoors (B-user-2). A radio whose GPS has a
    /// position keeps it: its own fix is better.
    private func usesPhone(_ r: RadioSettings) -> Bool {
        r.positionInAdverts && (r.gps != true || (r.latE6 == 0 && r.lonE6 == 0))
    }

    /// Who may ask the radio for its readings (telemetry).
    private func readings(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Readings").padding(.top, 10)
            CardList {
                telemetryRow(String(localized: "Telemetry: battery and status"), r.telemetry.base, icon: "battery.100") { v in
                    setPrefs(r, row: "telemetry_base", telemetry: Telemetry(base: v, location: r.telemetry.location, environment: r.telemetry.environment))
                }
                .setting("telemetry_base", lit: highlighted)
                RadioRowStatus(row: "telemetry_base")
                CardDivider.tiled
                telemetryRow(String(localized: "Telemetry: position"), r.telemetry.location, icon: "location.circle") { v in
                    if v == 0 {
                        setPrefs(r, row: "telemetry_location", telemetry: Telemetry(base: r.telemetry.base, location: 0, environment: r.telemetry.environment))
                    } else {
                        confirmingTelemetry = v
                    }
                }
                .setting("telemetry_location", lit: highlighted)
                RadioRowStatus(row: "telemetry_location")
                CardDivider.tiled
                telemetryRow(String(localized: "Telemetry: sensors"), r.telemetry.environment, icon: "thermometer.medium") { v in
                    setPrefs(r, row: "telemetry_environment", telemetry: Telemetry(base: r.telemetry.base, location: r.telemetry.location, environment: v))
                }
                .setting("telemetry_environment", lit: highlighted)
                RadioRowStatus(row: "telemetry_environment")
            }
            Text("Who may ask your radio for them. Contacts are chosen in the MeshCore app. lichen’s own positions always travel encrypted, only to the people you choose.")
                .font(.footnote)
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
        }
        .alert("Give your position by telemetry?", isPresented: Binding(get: { confirmingTelemetry != nil }, set: { if !$0 { confirmingTelemetry = nil } })) {
            Button("Allow", role: .destructive) {
                if let v = confirmingTelemetry {
                    setPrefs(r, row: "telemetry_location", telemetry: Telemetry(base: r.telemetry.base, location: v, environment: r.telemetry.environment))
                }
                confirmingTelemetry = nil
            }
            Button("Cancel", role: .cancel) { confirmingTelemetry = nil }
        } message: {
            Text("Those allowed can ask your radio where it is, at any time, unencrypted.")
        }
    }

    private func telemetryRow(_ title: String, _ value: UInt8, icon: String, set: @escaping (UInt8) -> Void) -> some View {
        CardRow(title: title, icon: icon) {
            Picker(title, selection: Binding(get: { value }, set: set)) {
                Text("Nobody").tag(UInt8(0))
                Text("Chosen contacts").tag(UInt8(1))
                Text("Everyone").tag(UInt8(2))
            }
            .labelsHidden()
        }
    }

    private func contacts(_ r: RadioSettings) -> some View {
        let mask = r.autoaddMask ?? 0
        let hops = r.autoaddMaxHops ?? 0
        let setMask: (UInt8, Bool) -> Void = { bit, on in
            let m = on ? mask | bit : mask & ~bit
            model.radioSet(.autoadd, row: "autoadd_\(bit)", asked: { $0.autoaddMask = m; $0.autoaddMaxHops = hops }) {
                try model.core.setAutoadd(mask: m, maxHops: hops)
            }
        }
        return VStack(alignment: .leading, spacing: 8) {
            SectionLabel("New contacts").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Add every radio heard"),
                        subtitle: r.manualAdd ? String(localized: "Only the kinds below become contacts by themselves.")
                                              : String(localized: "Every advert heard becomes a contact."),
                        icon: "person.badge.plus") {
                    Toggle("Add every radio heard",
                           isOn: Binding(get: { !r.manualAdd }, set: { on in setPrefs(r, row: "manual_add", manualAdd: !on) }))
                        .labelsHidden()
                }
                .setting("manual_add", lit: highlighted)
                RadioRowStatus(row: "manual_add")
                if r.manualAdd {
                    ForEach(Self.kinds, id: \.bit) { kind in
                        CardDivider.tiled
                        CardRow(title: kind.title, icon: kind.icon) {
                            Toggle(kind.title, isOn: Binding(get: { mask & kind.bit != 0 }, set: { setMask(kind.bit, $0) }))
                                .labelsHidden()
                        }
                        .setting("autoadd_\(kind.bit)", lit: highlighted)
                        RadioRowStatus(row: "autoadd_\(kind.bit)")
                    }
                }
                CardDivider.tiled
                CardRow(title: String(localized: "Replace the oldest when full"), icon: "arrow.triangle.2.circlepath") {
                    Toggle("Replace the oldest when full",
                           isOn: Binding(get: { mask & 0x01 != 0 }, set: { setMask(0x01, $0) }))
                        .labelsHidden()
                }
                .setting("autoadd_1", lit: highlighted)
                RadioRowStatus(row: "autoadd_1")
                CardDivider.tiled
                CardRow(title: String(localized: "From how far"), icon: "arrow.up.left.and.arrow.down.right") {
                    Picker("From how far",
                           selection: Binding(get: { hops },
                                              set: { h in
                                                  model.radioSet(.autoadd, row: "autoadd_hops", asked: { $0.autoaddMaxHops = h }) {
                                                      try model.core.setAutoadd(mask: mask, maxHops: h)
                                                  }
                                              })) {
                        ForEach(hopChoices(hops), id: \.self) { Text(hopLabel($0)).tag($0) }
                    }
                    .labelsHidden()
                }
                .setting("autoadd_hops", lit: highlighted)
                RadioRowStatus(row: "autoadd_hops")
            }
        }
    }

    private func delivery(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Delivery").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Extra acknowledgements: \(Int(r.multiAcks))"),
                        subtitle: String(localized: "Sends each acknowledgement again: more reliable, more airtime."),
                        icon: "checkmark.bubble") {
                    Stepper("Extra acknowledgements",
                            value: Binding(get: { Int(r.multiAcks) }, set: { setPrefs(r, row: "multi_acks", multiAcks: UInt8($0)) }),
                            in: 0...2)
                        .labelsHidden()
                }
                .setting("multi_acks", lit: highlighted)
                RadioRowStatus(row: "multi_acks")
            }
        }
    }

    private func bluetooth(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Bluetooth").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "PIN"),
                        subtitle: pinSaved ? String(localized: "Saved: used from the radio’s next restart. Pair the phone again with it.")
                                           : String(localized: "Six digits; empty for the radio’s default."),
                        icon: "lock") {
                    TextField("123456", text: $pinText)
                        .keyboardType(.numberPad)
                        .multilineTextAlignment(.trailing)
                        .fixedSize(horizontal: true, vertical: false)
                        .frame(minWidth: 90, alignment: .trailing)
                }
                .setting("pin", lit: highlighted)
                CardDivider.tiled
                Button {
                    let pin = UInt32(pinText) ?? 0
                    model.radioSet(.pin, row: "pin", asked: { _ in }) { try model.core.setBlePin(pin: pin) }
                    pinSaved = model.radioError == nil
                } label: {
                    CardRow(title: String(localized: "Save the PIN"), icon: "square.and.arrow.down")
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(!(pinText.isEmpty || (pinText.count == 6 && pinText.allSatisfy(\.isNumber))))
                .setting("save_pin", lit: highlighted)
                RadioRowStatus(row: "pin")
                CardDivider.tiled
                Button { confirmingReboot = true } label: {
                    CardRow(title: String(localized: "Restart the radio"), icon: "arrow.clockwise")
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .setting("restart", lit: highlighted)
            }
        }
        .alert("Restart the radio?", isPresented: $confirmingReboot) {
            Button("Restart", role: .destructive) { model.rebootRadio() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The connection drops and comes back in a few seconds.")
        }
    }

    // MARK: Helpers

    /// The LoRa values chosen on screen, or nil for an unreadable frequency.
    private var chosen: (UInt32, UInt32, UInt8, UInt8)? {
        if let v = preset.values { return v }
        let text = freqText.replacingOccurrences(of: ",", with: ".")
        guard let f = Double(text), f > 0 else { return nil }
        return (UInt32((f * 1000).rounded()), bwHz, sf, cr)
    }

    private func setPrefs(_ r: RadioSettings, row: String, manualAdd: Bool? = nil, telemetry: Telemetry? = nil,
                          position: Bool? = nil, multiAcks: UInt8? = nil) {
        let manualAdd = manualAdd ?? r.manualAdd
        let telemetry = telemetry ?? r.telemetry
        let position = position ?? r.positionInAdverts
        let multiAcks = multiAcks ?? r.multiAcks
        model.radioSet(.preferences, row: row, asked: {
            $0.manualAdd = manualAdd
            $0.telemetry = telemetry
            $0.positionInAdverts = position
            $0.multiAcks = multiAcks
        }) {
            try model.core.setRadioPreferences(manualAdd: manualAdd, telemetry: telemetry,
                                               positionInAdverts: position, multiAcks: multiAcks)
        }
    }

    /// The frequency in MHz, in the app's language ("869,618" in French): the field reads it
    /// back with a comma or a point.
    private func mhz(_ khz: UInt32) -> String {
        decimal(Double(khz) / 1000, 3)
    }

    private func summary(_ khz: UInt32, _ hz: UInt32, _ sf: UInt8, _ cr: UInt8) -> String {
        "\(mhz(khz)) MHz · \(bandwidth(hz)) · SF\(sf) · CR 4/\(cr)"
    }

    private func position(_ r: RadioSettings) -> String {
        if r.latE6 == 0 && r.lonE6 == 0 { return String(localized: "None yet") }
        return String(format: "%.5f, %.5f", Double(r.latE6) / 1e6, Double(r.lonE6) / 1e6)
    }

    /// 0 any distance, 1 direct only, N up to N − 1 hops (the firmware's rule).
    private func hopChoices(_ current: UInt8) -> [UInt8] {
        let base: [UInt8] = [0, 1, 2, 3, 4, 6]
        return base.contains(current) ? base : (base + [current]).sorted()
    }

    private func hopLabel(_ h: UInt8) -> String {
        switch h {
        case 0: String(localized: "Any distance")
        case 1: String(localized: "Direct only")
        default: String(localized: "Up to \(Int(h) - 1) hops")
        }
    }
}

extension View {
    /// A settings row a search leads to: its id to scroll to, lit while it is the one found.
    func setting(_ id: String, lit: String?) -> some View {
        self.background(lit == id ? Theme.line : Color.clear, in: RoundedRectangle(cornerRadius: 14))
            .id(id)
    }
}

/// The radio at the top of its settings: name and model, the link and for how long, the
/// battery, the airtime left this hour (a bar in `lora`) and the firmware. Only what the radio
/// and the core report: a figure the radio did not give is left out.
struct RadioHeader: View {
    let settings: RadioSettings
    @Environment(AppModel.self) private var model
    @ScaledMetric(relativeTo: .subheadline) private var ring: CGFloat = 58
    @Environment(\.dynamicTypeSize) private var textSize

    var body: some View {
        let r = settings
        let name = r.name.isEmpty ? model.nodeName : r.name
        // The ring beside the name, or under it at the accessibility text sizes: the same views.
        let top = textSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12)) : AnyLayout(HStackLayout(spacing: 12))
        VStack(alignment: .leading, spacing: 12) {
            top {
                IconTile(symbol: "antenna.radiowaves.left.and.right", tint: .lora, side: 48)
                VStack(alignment: .leading, spacing: 2) {
                    Text(name.isEmpty ? String(localized: "MeshCore radio") : name)
                        .font(.system(.title3, weight: .bold))
                        .foregroundStyle(Theme.tx)
                    if !r.manufacturer.isEmpty && r.manufacturer != name {
                        Text(r.manufacturer).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    }
                    TimelineView(.periodic(from: .now, by: 30)) { context in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Circle().fill(Theme.ok).frame(width: 7, height: 7).accessibilityHidden(true)
                            Text(line(at: context.date))
                                .font(.system(.footnote))
                                .foregroundStyle(Theme.tx2)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    RadioReadingBar().padding(.top, 2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
                if let mv = r.batteryMv, let percent = r.batteryPercent {
                    battery(Int(percent), volts: decimal(Double(mv) / 1000, 2))
                }
            }
            Rectangle().fill(Theme.line).frame(height: 1)
            HStack(alignment: .top, spacing: 12) {
                figure("\(airtimeLeft) s", String(localized: "airtime left"))
                if !r.firmware.isEmpty { figure(r.firmware, String(localized: "firmware")) }
            }
            // The share of this hour's airtime left.
            Capsule().fill(Theme.s2)
                .frame(height: 5)
                .overlay(alignment: .leading) {
                    GeometryReader { g in
                        Capsule().fill(Theme.lora).frame(width: g.size.width * fraction)
                    }
                }
                .accessibilityHidden(true)
        }
        .card(padding: 16)
    }

    /// "Bluetooth · connected 2 h 14 min"; while a connection still reads the radio, what it
    /// reads ("Bluetooth · Reading the radio · channels").
    private func line(at now: Date) -> String {
        if let reading = radioReadingText(model) { return "\(model.linkName) · \(reading)" }
        guard let since = model.connectedSince else { return model.linkName }
        let minutes = max(0, Int(now.timeIntervalSince(since) / 60))
        guard minutes > 0 else { return "\(model.linkName) · \(String(localized: "connected just now"))" }
        // "14 min", "2 h", "2 h 14 min": the same in French.
        let h = minutes / 60, m = minutes % 60
        let duration = h == 0 ? "\(m) min" : m == 0 ? "\(h) h" : "\(h) h \(m) min"
        return "\(model.linkName) · \(String(localized: "connected \(duration)"))"
    }

    private var airtimeLeft: UInt64 {
        (model.budget.limitMs - min(model.budget.usedMs, model.budget.limitMs)) / 1000
    }

    private var fraction: CGFloat {
        guard model.budget.limitMs > 0 else { return 0 }
        return CGFloat(airtimeLeft * 1000) / CGFloat(model.budget.limitMs)
    }

    /// A tabular figure over what it is.
    private func figure(_ value: String, _ label: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(value).font(.system(.subheadline, weight: .semibold).monospacedDigit()).foregroundStyle(Theme.tx)
            Text(label).font(.system(.caption)).foregroundStyle(Theme.tx2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    /// The charge in a ring: `ok`, `warn` at 30 % and below, `sos` at 15 % and below. The ring
    /// grows with its figures (larger text).
    private func battery(_ percent: Int, volts: String) -> some View {
        let colour = percent <= 15 ? Theme.sos : percent <= 30 ? Theme.warn : Theme.ok
        return Square {
            VStack(spacing: 0) {
                Text(verbatim: "\(percent) %").font(.system(.subheadline, weight: .bold).monospacedDigit())
                // Caption, not caption2: caption2 stays 11 pt at the smaller text sizes.
                Text(verbatim: "\(volts) V").font(.system(.caption).monospacedDigit()).foregroundStyle(Theme.tx2)
            }
            .foregroundStyle(Theme.tx)
            .fixedSize()
            .padding(9)
        }
        .frame(minWidth: ring, minHeight: ring)
        .background {
            ZStack {
                Circle().stroke(Theme.line, lineWidth: 5)
                Circle()
                    .trim(from: 0, to: CGFloat(min(max(percent, 0), 100)) / 100)
                    .stroke(colour, style: StrokeStyle(lineWidth: 5, lineCap: .round))
                    .rotationEffect(.degrees(-90))
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(String(localized: "Battery \(percent) percent, \(volts) volts"))
    }
}

/// Its content centred in a square as wide as its larger side: a ring's figures, whole.
private struct Square: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let size = subviews.first?.sizeThatFits(.unspecified) ?? .zero
        let side = max(size.width, size.height)
        return CGSize(width: side, height: side)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: CGPoint(x: bounds.midX, y: bounds.midY), anchor: .center, proposal: .unspecified)
    }
}

/// What became of the setting changed from `row` (B-user-5): a spinner while the radio has not
/// answered, then "Saved on the radio" for 2 s, or why the value shown is still the radio's
/// (kept, refused, unanswered) until the next change.
struct RadioRowStatus: View {
    let row: String
    @Environment(AppModel.self) private var model

    var body: some View {
        if let state = model.radioRows[row] {
            HStack(spacing: 8) {
                switch state.outcome {
                case nil:
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityLabel("Waiting for the radio")
                case .saved?:
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.ok).accessibilityHidden(true)
                    Text("Saved on the radio")
                case let outcome?:
                    Image(systemName: "info.circle").foregroundStyle(Theme.warn).accessibilityHidden(true)
                    Text(Self.why(outcome))
                }
            }
            .font(.system(.footnote))
            .foregroundStyle(Theme.tx)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.bottom, 11)
            .accessibilityElement(children: .combine)
            .task(id: state) {
                guard state.outcome == .saved else { return }
                try? await Task.sleep(for: .seconds(model.radioSavedSeconds))
                model.clearRadioRow(row)
            }
        }
    }

    private static func why(_ outcome: SettingOutcome) -> String {
        switch outcome {
        case .notKept: String(localized: "The radio answered but kept its old value.")
        case .refused: String(localized: "The radio refused this setting.")
        case .noAnswer, .saved: String(localized: "The radio did not answer.")
        }
    }
}
