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
struct RadioSettingsView: View {
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

    static let bandwidths: [UInt32] = [7_800, 10_400, 15_600, 20_800, 31_250, 41_700, 62_500, 125_000, 250_000, 500_000]
    static let intervals: [UInt32] = [60, 300, 900, 3_600]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                if let r = model.radioSettings, r.read, model.isConnected {
                    device(r)
                    lora(r)
                    power(r)
                    if r.gps != nil { gps(r) }
                    privacy(r)
                    if r.autoaddMask != nil { contacts(r) }
                    delivery(r)
                    bluetooth(r)
                } else if model.isConnected {
                    // Connected, the radio has not answered yet: no zeros shown as its values (D-9).
                    HStack(spacing: 10) {
                        ProgressView()
                        Text("Reading the radio's settings…").foregroundStyle(Theme.tx2)
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
        .background(Theme.bg)
        .navigationTitle("Radio settings")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            model.core.refreshRadio()
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

    // MARK: Sections

    private func device(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Radio").padding(.top, 10)
            CardList {
                CardRow(title: r.manufacturer.isEmpty ? String(localized: "MeshCore radio") : r.manufacturer,
                        subtitle: r.firmware.isEmpty ? nil : String(localized: "Firmware \(r.firmware) · \(r.buildDate)"))
                if let mv = r.batteryMv {
                    CardDivider()
                    CardRow(title: String(localized: "Battery"), subtitle: battery(mv))
                }
                if let used = r.storageUsedKb, let total = r.storageTotalKb, total > 0 {
                    CardDivider()
                    CardRow(title: String(localized: "Storage"),
                            subtitle: String(localized: "\(Int(used)) of \(Int(total)) KB used"))
                }
                // Its MeshCore public key, as the stock app shows it (F-1).
                if let key = model.nodeKey {
                    CardDivider()
                    MeshCoreKeyRow(key: key)
                    CardDivider()
                    ShareLink(item: key.hex) {
                        CardRow(title: String(localized: "Share the key")) {
                            Image(systemName: "square.and.arrow.up").foregroundStyle(Theme.tx2)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
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
                CardRow(title: String(localized: "Now"), subtitle: summary(r.freqKhz, r.bwHz, r.sf, r.cr))
                CardDivider()
                CardRow(title: String(localized: "Preset")) {
                    Picker("Preset", selection: $preset) {
                        ForEach(RadioPreset.allCases) { Text($0.label).tag($0) }
                    }
                    .labelsHidden()
                }
                if preset == .custom {
                    CardDivider()
                    CardRow(title: String(localized: "Frequency (MHz)")) {
                        TextField("869.618", text: $freqText)
                            .keyboardType(.decimalPad)
                            .multilineTextAlignment(.trailing)
                            .frame(width: 110)
                    }
                    CardDivider()
                    CardRow(title: String(localized: "Bandwidth")) {
                        Picker("Bandwidth", selection: $bwHz) {
                            ForEach(Self.bandwidths, id: \.self) { Text(bandwidth($0)).tag($0) }
                        }
                        .labelsHidden()
                    }
                    CardDivider()
                    CardRow(title: String(localized: "Spreading factor")) {
                        Picker("Spreading factor", selection: $sf) {
                            ForEach(Array(UInt8(5)...UInt8(12)), id: \.self) { Text(verbatim: "SF\($0)").tag($0) }
                        }
                        .labelsHidden()
                    }
                    CardDivider()
                    CardRow(title: String(localized: "Coding rate")) {
                        Picker("Coding rate", selection: $cr) {
                            ForEach(Array(UInt8(5)...UInt8(8)), id: \.self) { Text(verbatim: "4/\($0)").tag($0) }
                        }
                        .labelsHidden()
                    }
                }
                CardDivider()
                Button { confirmingParams = true } label: {
                    CardRow(title: String(localized: "Apply")) {
                        Image(systemName: "checkmark.circle").foregroundStyle(Theme.tx2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(chosen.map { $0 == (r.freqKhz, r.bwHz, r.sf, r.cr) } ?? true)
            }
            Text("A radio on other settings no longer hears the radios of its mesh: choose your region's preset, or the values your local community uses.")
                .font(.footnote)
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
        }
        .alert("Change the LoRa settings?", isPresented: $confirmingParams) {
            Button("Apply", role: .destructive) {
                guard let (f, b, s, c) = chosen else { return }
                model.radioSet { try model.core.setRadioParams(freqKhz: f, bwHz: b, sf: s, cr: c) }
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
                        subtitle: String(localized: "Up to \(Int(r.maxTxDbm)) dBm on this radio. Less reaches less far and saves its battery.")) {
                    Stepper("Transmit power",
                            value: Binding(get: { Int(r.txDbm) },
                                           set: { v in model.radioSet { try model.core.setTxPower(dbm: Int8(v)) } }),
                            in: -9...Int(r.maxTxDbm))
                        .labelsHidden()
                }
            }
        }
    }

    private func gps(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("GPS").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "The radio's GPS"),
                        subtitle: String(localized: "Keeps the radio's position current. It draws power.")) {
                    Toggle("The radio's GPS",
                           isOn: Binding(get: { r.gps == true },
                                         set: { on in model.radioSet { try model.core.setGps(on: on) } }))
                        .labelsHidden()
                }
                if r.gps == true {
                    CardDivider()
                    CardRow(title: String(localized: "A fix every")) {
                        Picker("A fix every",
                               selection: Binding(get: { gpsInterval },
                                                  set: { s in
                                                      gpsInterval = s
                                                      if let s { model.radioSet { try model.core.setGpsInterval(seconds: s) } }
                                                  })) {
                            Text("Not set").tag(UInt32?.none)
                            ForEach(Self.intervals, id: \.self) { s in
                                Text(Duration.seconds(Int64(s)).formatted(.units(allowed: [.hours, .minutes], width: .wide))).tag(UInt32?.some(s))
                            }
                        }
                        .labelsHidden()
                    }
                }
            }
        }
    }

    /// A radio advertising its position takes this phone's when it has none of its own: without
    /// GPS, or with its GPS on but no fix yet, indoors (B-user-2). A radio whose GPS has a
    /// position keeps it: its own fix is better.
    private func usesPhone(_ r: RadioSettings) -> Bool {
        r.positionInAdverts && (r.gps != true || (r.latE6 == 0 && r.lonE6 == 0))
    }

    private func privacy(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Position and telemetry").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Position in adverts"),
                        subtitle: r.positionInAdverts
                            ? String(localized: "Every advert shows the radio's position to the whole mesh, unencrypted.")
                            : String(localized: "Off: adverts carry no position.")) {
                    Toggle("Position in adverts",
                           isOn: Binding(get: { r.positionInAdverts },
                                         set: { on in if on { confirmingPosition = true } else { setPrefs(r, position: false) } }))
                        .labelsHidden()
                }
                if r.positionInAdverts {
                    CardDivider()
                    CardRow(title: String(localized: "Position advertised"), subtitle: position(r))
                    // Why there is none: the GPS is on but has no fix (it needs open sky).
                    if r.gps == true && r.latE6 == 0 && r.lonE6 == 0 {
                        Text("The radio's GPS has no fix yet: it needs open sky. Until then, it can advertise this phone's position.")
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 16)
                            .padding(.bottom, 11)
                    }
                    if usesPhone(r), let fix = model.myFix {
                        CardDivider()
                        Button {
                            model.radioSet {
                                try model.core.setAdvertPosition(latE6: Int32((fix.lat * 1e6).rounded()), lonE6: Int32((fix.lon * 1e6).rounded()))
                            }
                        } label: {
                            CardRow(title: String(localized: "Use this phone's position")) {
                                Image(systemName: "location").foregroundStyle(Theme.tx2)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                CardDivider()
                telemetryRow(String(localized: "Telemetry: battery and status"), r.telemetry.base) { v in
                    setPrefs(r, telemetry: Telemetry(base: v, location: r.telemetry.location, environment: r.telemetry.environment))
                }
                CardDivider()
                telemetryRow(String(localized: "Telemetry: position"), r.telemetry.location) { v in
                    if v == 0 {
                        setPrefs(r, telemetry: Telemetry(base: r.telemetry.base, location: 0, environment: r.telemetry.environment))
                    } else {
                        confirmingTelemetry = v
                    }
                }
                CardDivider()
                telemetryRow(String(localized: "Telemetry: sensors"), r.telemetry.environment) { v in
                    setPrefs(r, telemetry: Telemetry(base: r.telemetry.base, location: r.telemetry.location, environment: v))
                }
            }
            Text("Who may ask your radio for them. Contacts are chosen in the MeshCore app. lichen's own positions always travel encrypted, only to the people you choose.")
                .font(.footnote)
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
        }
        // The phone's location runs while its position can be given to the radio, as on the map.
        .onAppear { if usesPhone(r) { model.needsLocation("radio", true) } }
        .onChange(of: usesPhone(r)) { _, on in model.needsLocation("radio", on) }
        .onDisappear { model.needsLocation("radio", false) }
        .alert("Show your position to the whole mesh?", isPresented: $confirmingPosition) {
            Button("Show it", role: .destructive) { setPrefs(r, position: true) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Every advert of your radio will carry its position, unencrypted: anyone with a MeshCore app nearby sees where it is.")
        }
        .alert("Give your position by telemetry?", isPresented: Binding(get: { confirmingTelemetry != nil }, set: { if !$0 { confirmingTelemetry = nil } })) {
            Button("Allow", role: .destructive) {
                if let v = confirmingTelemetry {
                    setPrefs(r, telemetry: Telemetry(base: r.telemetry.base, location: v, environment: r.telemetry.environment))
                }
                confirmingTelemetry = nil
            }
            Button("Cancel", role: .cancel) { confirmingTelemetry = nil }
        } message: {
            Text("Those allowed can ask your radio where it is, at any time, unencrypted.")
        }
    }

    private func telemetryRow(_ title: String, _ value: UInt8, set: @escaping (UInt8) -> Void) -> some View {
        CardRow(title: title) {
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
            model.radioSet { try model.core.setAutoadd(mask: on ? mask | bit : mask & ~bit, maxHops: hops) }
        }
        return VStack(alignment: .leading, spacing: 8) {
            SectionLabel("New contacts").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Add every radio heard"),
                        subtitle: r.manualAdd ? String(localized: "Only the kinds below become contacts by themselves.")
                                              : String(localized: "Every advert heard becomes a contact.")) {
                    Toggle("Add every radio heard",
                           isOn: Binding(get: { !r.manualAdd }, set: { on in setPrefs(r, manualAdd: !on) }))
                        .labelsHidden()
                }
                if r.manualAdd {
                    ForEach([(UInt8(0x02), String(localized: "People (companions)")),
                             (UInt8(0x04), String(localized: "Repeaters")),
                             (UInt8(0x08), String(localized: "Room servers")),
                             (UInt8(0x10), String(localized: "Sensors"))], id: \.0) { bit, title in
                        CardDivider()
                        CardRow(title: title) {
                            Toggle(title, isOn: Binding(get: { mask & bit != 0 }, set: { setMask(bit, $0) }))
                                .labelsHidden()
                        }
                    }
                }
                CardDivider()
                CardRow(title: String(localized: "Replace the oldest when full")) {
                    Toggle("Replace the oldest when full",
                           isOn: Binding(get: { mask & 0x01 != 0 }, set: { setMask(0x01, $0) }))
                        .labelsHidden()
                }
                CardDivider()
                CardRow(title: String(localized: "From how far")) {
                    Picker("From how far",
                           selection: Binding(get: { hops },
                                              set: { h in model.radioSet { try model.core.setAutoadd(mask: mask, maxHops: h) } })) {
                        ForEach(hopChoices(hops), id: \.self) { Text(hopLabel($0)).tag($0) }
                    }
                    .labelsHidden()
                }
            }
        }
    }

    private func delivery(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Delivery").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "Extra acknowledgements: \(Int(r.multiAcks))"),
                        subtitle: String(localized: "Sends each acknowledgement again: more reliable, more airtime.")) {
                    Stepper("Extra acknowledgements",
                            value: Binding(get: { Int(r.multiAcks) }, set: { setPrefs(r, multiAcks: UInt8($0)) }),
                            in: 0...2)
                        .labelsHidden()
                }
            }
        }
    }

    private func bluetooth(_ r: RadioSettings) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Bluetooth").padding(.top, 10)
            CardList {
                CardRow(title: String(localized: "PIN"),
                        subtitle: pinSaved ? String(localized: "Saved: used from the radio's next restart. Pair the phone again with it.")
                                           : String(localized: "Six digits; empty for the radio's default.")) {
                    TextField("123456", text: $pinText)
                        .keyboardType(.numberPad)
                        .multilineTextAlignment(.trailing)
                        .frame(width: 90)
                }
                CardDivider()
                Button {
                    let pin = UInt32(pinText) ?? 0
                    model.radioSet { try model.core.setBlePin(pin: pin) }
                    pinSaved = model.radioError == nil
                } label: {
                    CardRow(title: String(localized: "Save the PIN"))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(!(pinText.isEmpty || (pinText.count == 6 && pinText.allSatisfy(\.isNumber))))
                CardDivider()
                Button { confirmingReboot = true } label: {
                    CardRow(title: String(localized: "Restart the radio")) {
                        Image(systemName: "arrow.clockwise").foregroundStyle(Theme.tx2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .alert("Restart the radio?", isPresented: $confirmingReboot) {
            Button("Restart", role: .destructive) { model.core.rebootRadio() }
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

    private func setPrefs(_ r: RadioSettings, manualAdd: Bool? = nil, telemetry: Telemetry? = nil,
                          position: Bool? = nil, multiAcks: UInt8? = nil) {
        model.radioSet {
            try model.core.setRadioPreferences(manualAdd: manualAdd ?? r.manualAdd,
                                               telemetry: telemetry ?? r.telemetry,
                                               positionInAdverts: position ?? r.positionInAdverts,
                                               multiAcks: multiAcks ?? r.multiAcks)
        }
    }

    private func mhz(_ khz: UInt32) -> String {
        String(format: "%.3f", Double(khz) / 1000)
    }

    private func summary(_ khz: UInt32, _ hz: UInt32, _ sf: UInt8, _ cr: UInt8) -> String {
        "\(mhz(khz)) MHz · \(bandwidth(hz)) · SF\(sf) · CR 4/\(cr)"
    }

    /// Volts, and a rough charge for a lithium cell (3.3 V empty, 4.2 V full).
    private func battery(_ mv: UInt16) -> String {
        let percent = Int((min(max((Double(mv) - 3_300) / 900, 0), 1) * 100).rounded())
        return String(localized: "\(String(format: "%.2f", Double(mv) / 1000)) V · about \(percent) %")
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
