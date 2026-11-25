import SwiftUI
import UniformTypeIdentifiers

/// Radio, airtime, transports, position, appearance (mockup `reglages`).
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @AppStorage("appearance") private var appearance = Appearance.system
    @State private var choosingAdvert = false
    @State private var naming = false
    @State private var meshName = ""
    @State private var nameError: String?
    @AppStorage("onboarded") private var onboarded = true
    @State private var importing = false
    @State private var importResult: String?
    @State private var exportingBackup = false
    @State private var radioOpen = false
    @AppStorage(radioDetailsKey) private var radioDetails = false
    @Environment(\.dynamicTypeSize) private var textSize

    /// The advert row's subtitle (B-user-4): what an advert is for, then what the radio answered.
    private var advertStatus: String {
        switch model.advert {
        case nil:
            String(localized: "Tells radios around that yours is here, so they can add it as a contact.")
        case .sending:
            String(localized: "Sending…")
        case let .sent(flood, at):
            flood ? String(localized: "Advert sent at \(at.formatted(date: .omitted, time: .shortened)) (whole mesh)")
                  : String(localized: "Advert sent at \(at.formatted(date: .omitted, time: .shortened)) (nearby)")
        case .notSent:
            String(localized: "The radio did not send it.")
        }
    }

    var body: some View {
        @Bindable var model = model
        NavigationStack {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Settings")
                            .font(.system(.largeTitle, weight: .bold))
                            .tracking(-0.5)
                            .padding(.horizontal, 4)
                            .padding(.top, 8)
                        radioCard
                        SectionLabel("Radio").padding(.top, 10)
                        CardList {
                            // The radio's own settings (ADR-0032).
                            NavigationLink {
                                RadioSettingsView()
                            } label: {
                                CardRow(title: String(localized: "Radio settings"), subtitle: preset, icon: "antenna.radiowaves.left.and.right", tint: .lora) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .disabled(!model.isConnected)
                            CardDivider.tiled
                            // Radios to switch between (ADR-0042).
                            NavigationLink {
                                SavedRadiosView()
                            } label: {
                                CardRow(title: String(localized: "Radios"),
                                        subtitle: String(localized: "\(model.savedRadios.count) saved"), icon: "rectangle.stack") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider.tiled
                            CardRow(title: String(localized: "Sub-band"), subtitle: subBand, icon: "waveform")
                            CardDivider.tiled
                            CardRow(title: String(localized: "Relay for others"),
                                    subtitle: String(localized: "Stock companion firmware does not repeat: a repeater node does that."), icon: "arrow.triangle.branch")
                            CardDivider.tiled
                            // Our advert (B-user-4): nearby or the whole mesh, chosen; then what the
                            // radio answered (without a radio: not sent).
                            Button { choosingAdvert = true } label: {
                                CardRow(title: String(localized: "Send an advert"), subtitle: advertStatus, icon: "dot.radiowaves.left.and.right") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .sheet(isPresented: $choosingAdvert) { AdvertChoiceSheet() }
                            CardDivider.tiled
                            // What MeshCore users and channels see (protocol.md §15.4).
                            Button { meshName = model.nodeName; naming = true } label: {
                                CardRow(title: String(localized: "Name on the mesh"),
                                        subtitle: model.nodeName.isEmpty ? String(localized: "What people with a MeshCore app see")
                                                                         : model.nodeName, icon: "pencil")
                            }
                            .buttonStyle(.plain)
                            .disabled(!model.isConnected)
                            .opacity(model.isConnected ? 1 : 0.5)
                            CardDivider.tiled
                            // Off: fragments, airtime and signal wait behind long press › Info.
                            CardRow(title: String(localized: "Show radio details"),
                                    subtitle: String(localized: "Fragments, airtime and signal under each message, and the radio card on Messages. Otherwise: long press › Info."), icon: "chart.bar") {
                                Toggle("Show radio details", isOn: $radioDetails).labelsHidden()
                            }
                        }
                        SectionLabel("Internet transports").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "iroh (direct P2P)"), subtitle: irohLine, icon: "link", tint: .iroh) {
                                Toggle("iroh (direct P2P)", isOn: $model.irohEnabled).labelsHidden()
                            }
                            CardDivider.tiled
                            CardRow(title: String(localized: "Tor"), subtitle: torLine, icon: "target", tint: .tor) {
                                Toggle("Tor", isOn: $model.torEnabled).labelsHidden()
                            }
                            CardDivider.tiled
                            CardRow(title: String(localized: "Relay only"),
                                    subtitle: String(localized: "iroh through relays: contacts never see your IP"), icon: "arrow.left.arrow.right", tint: .iroh) {
                                Toggle("Relay only", isOn: $model.relayOnly).labelsHidden()
                            }
                            .opacity(model.irohEnabled ? 1 : 0.5)
                            .disabled(!model.irohEnabled)
                        }
                        Text("Photos and files go over the Internet when both phones have it; LoRa only carries a one-packet announcement. iroh first, Tor when iroh cannot reach the sender.")
                            .font(.system(.caption))
                            .foregroundStyle(Theme.tx2)
                            .padding(.horizontal, 4)
                            .fixedSize(horizontal: false, vertical: true)
                        SectionLabel("GIFs").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "Load GIFs automatically"),
                                    subtitle: String(localized: "Otherwise tap each one: GIPHY sees your IP address"), icon: "photo") {
                                Toggle("Load GIFs automatically", isOn: $model.gifAutoLoad).labelsHidden()
                            }
                        }
                        Text("A GIF travels as its GIPHY id, a few bytes; the phone that receives it loads it from GIPHY. With Tor on, GIFs load through Tor, automatically.")
                            .font(.system(.caption))
                            .foregroundStyle(Theme.tx2)
                            .padding(.horizontal, 4)
                            .fixedSize(horizontal: false, vertical: true)
                        SectionLabel("Voice messages").padding(.top, 10)
                        Picker("Voice messages", selection: $model.voiceMode) {
                            Text("Compact").tag(0)
                            Text("Standard").tag(2)
                            Text("Clear").tag(3)
                        }
                        .pickerStyle(.segmented)
                        Text(voiceNote)
                            .font(.system(.caption))
                            .foregroundStyle(Theme.tx2)
                            .padding(.horizontal, 4)
                            .fixedSize(horizontal: false, vertical: true)
                        SectionLabel("Position").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "Send threshold"),
                                    subtitle: String(localized: "Distance from the predicted track"), icon: "location") {
                                Text("50 m").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                            }
                            CardDivider.tiled
                            CardRow(title: String(localized: "Heartbeat when still"),
                                    subtitle: String(localized: "Contacts; groups every 30 min"), icon: "waveform.path.ecg") {
                                Text("15 min").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                            }
                        }
                        SectionLabel("Appearance").padding(.top, 10)
                        Picker("Appearance", selection: $appearance) {
                            ForEach(Appearance.allCases) { Text($0.label).tag($0) }
                        }
                        .pickerStyle(.segmented)
                        privacy
                        // What each conversation takes, and old media to clear (ADR-0034).
                        SectionLabel("Storage").padding(.top, 10)
                        CardList {
                            NavigationLink { StorageView() } label: {
                                CardRow(title: String(localized: "Storage"),
                                        subtitle: String(localized: "Usage per conversation; clear old photos, files and voice messages."), icon: "internaldrive") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                        // History from the official MeshCore app (ADR-0033).
                        SectionLabel("History").padding(.top, 10)
                        CardList {
                            Button {
                                #if DEBUG
                                // -importFile <path>: this export, as if picked in Files (UI tests).
                                if let path = UserDefaults.standard.string(forKey: "importFile") {
                                    importResult = model.importMeshCore(from: URL(fileURLWithPath: path))
                                    return
                                }
                                #endif
                                importing = true
                            } label: {
                                CardRow(title: String(localized: "Import from the MeshCore app"),
                                        subtitle: String(localized: "The database it exports: your direct messages and channel posts join your MeshCore chats."), icon: "square.and.arrow.down") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                        .fileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
                            if case let .success(url) = result { importResult = model.importMeshCore(from: url) }
                        }
                        .alert("Import", isPresented: Binding(get: { importResult != nil }, set: { if !$0 { importResult = nil } })) {
                            Button("OK") { importResult = nil }
                        } message: {
                            Text(importResult ?? "")
                        }
                        // The encrypted backup (ADR-0037).
                        if !model.isDemo {
                            SectionLabel("Backup").padding(.top, 10)
                            CardList {
                                Button { exportingBackup = true } label: {
                                    CardRow(title: String(localized: "Export a backup"),
                                            subtitle: String(localized: "Messages, contacts, groups, settings and media in one file your 24 words open."), icon: "archivebox") {
                                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                    }
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                            .sheet(isPresented: $exportingBackup) { BackupExportSheet() }
                        }
                        SectionLabel("Demo").padding(.top, 10)
                        CardList {
                            if model.isDemo {
                                // Off: messages wait for the radio, as when Bluetooth drops (§6.1).
                                CardRow(title: String(localized: "Simulated radio connected"),
                                        subtitle: String(localized: "Turn it off to see messages wait for the radio."), icon: "power") {
                                    Toggle("Simulated radio connected",
                                           isOn: Binding(get: { !model.demoRadioOff }, set: { model.setDemoRadio(on: $0) }))
                                        .labelsHidden()
                                }
                                CardDivider.tiled
                                Button { model.stopDemo() } label: {
                                    CardRow(title: String(localized: "Leave the demo"), icon: "rectangle.portrait.and.arrow.right")
                                }
                                .buttonStyle(.plain)
                            } else {
                                Button { model.startDemo() } label: {
                                    CardRow(title: String(localized: "Try the demo network"),
                                            subtitle: String(localized: "A simulated MeshCore network with two other users and a group, in real time: the app’s real core, without a radio."), icon: "play.circle") {
                                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                    }
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                            CardDivider.tiled
                            Button { onboarded = false } label: {
                                CardRow(title: String(localized: "Show the welcome screens again"), icon: "hand.wave")
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                        SectionLabel("About").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "Version"), icon: "info.circle") {
                                // "0.10.3 (212)": the release and its build (scripts/version.sh).
                                Text(verbatim: "\(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "") (\(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? ""))")
                                    .foregroundStyle(Theme.tx2)
                            }
                            CardDivider.tiled
                            CardRow(title: String(localized: "Stickers"),
                                    subtitle: String(localized: "Noto Animated Emoji © Google, CC BY 4.0. Animations by Lottie (Airbnb)."), icon: "face.smiling") {
                                EmptyView()
                            }
                            CardDivider.tiled
                            NavigationLink {
                                IdentityView()
                            } label: {
                                CardRow(title: String(localized: "Your identity"), subtitle: identityStatus, icon: "person.badge.key") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                            }
                            .buttonStyle(.plain)
                            CardDivider.tiled
                            NavigationLink {
                                LicencesView()
                            } label: {
                                CardRow(title: String(localized: "Licences"),
                                        subtitle: String(localized: "GNU GPL 3 or later, and what lichen includes"), icon: "doc.text") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider.tiled
                            // ADR-0045: no message, key, name or position in it.
                            NavigationLink {
                                ReportView()
                            } label: {
                                CardRow(title: String(localized: "Report a problem"),
                                        subtitle: String(localized: "A log without your messages, to send to the developers"), icon: "exclamationmark.bubble") {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.bottom, 24)
                }
                .onAppear {
                    #if DEBUG
                    // -screen privacy: the Privacy section, for screenshots.
                    if model.debugScreen == "privacy" { proxy.scrollTo("privacy", anchor: .top) }
                    // -screen radio: the radio's settings (ADR-0032).
                    if model.debugScreen == "radio" { radioOpen = true }
                    #endif
                }
            }
            .navigationDestination(isPresented: $radioOpen) { RadioSettingsView() }
            .screen()
            .toolbar(.hidden, for: .navigationBar)
            .alert("Name on the mesh", isPresented: $naming) {
                TextField("Name", text: $meshName)
                Button("Save") { nameError = model.setMeshName(meshName) }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("People with a MeshCore app see this name, and channel posts start with it. Your radio announces it.")
            }
            .alert("Not changed", isPresented: Binding(get: { nameError != nil }, set: { if !$0 { nameError = nil } })) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(nameError ?? "")
            }
        }
    }

    // MARK: privacy

    /// The app lock (`AppLock`): on after one successful authentication, off by default.
    @ViewBuilder private var privacy: some View {
        @Bindable var lock = model.lock
        SectionLabel("Privacy").padding(.top, 10).id("privacy")
        CardList {
            CardRow(title: String(localized: "Lock lichen"), subtitle: unlockMethod, icon: "lock") {
                Toggle("Lock lichen", isOn: Binding(get: { lock.enabled }, set: { on in Task { await lock.setEnabled(on) } }))
                    .labelsHidden()
                    .disabled(lock.authenticating)
            }
            CardDivider.tiled
            CardRow(title: String(localized: "Lock after"), icon: "timer") {
                Picker("Lock after", selection: $lock.delay) {
                    ForEach(AppLock.delays, id: \.self) { Text(Self.lockDelay($0)).tag($0) }
                }
                .pickerStyle(.menu)
                .labelsHidden()
            }
            .opacity(lock.enabled ? 1 : 0.5)
            .disabled(!lock.enabled)
            CardDivider.tiled
            // People the user blocked (ADR-0034), to unblock.
            NavigationLink { BlockedView() } label: {
                CardRow(title: String(localized: "Blocked"), icon: "hand.raised") {
                    HStack(spacing: 8) {
                        Text(String(model.blocked.count)).foregroundStyle(Theme.tx2)
                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        Text("The radio keeps receiving while lichen is locked.")
            .font(.system(.caption))
            .foregroundStyle(Theme.tx2)
            .padding(.horizontal, 4)
            .fixedSize(horizontal: false, vertical: true)
            .alert("The lock stays off", isPresented: Binding(get: { lock.problem != nil }, set: { if !$0 { lock.problem = nil } })) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(lock.problem ?? "")
            }
    }

    private var unlockMethod: String {
        switch model.lock.biometry {
        case .faceID: String(localized: "With Face ID or your passcode")
        case .touchID: String(localized: "With Touch ID or your passcode")
        default: String(localized: "With your passcode")
        }
    }

    private static func lockDelay(_ seconds: Int) -> String {
        switch seconds {
        case 0: String(localized: "Immediately")
        case 60: String(localized: "1 minute")
        case 900: String(localized: "15 minutes")
        default: String(localized: "1 hour")
        }
    }

    // MARK: radio card

    /// The radio in one line: its name, the link, its battery and the airtime left this hour.
    private var radioCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            if model.isConnected {
                // The badge beside the name, or under it at the accessibility text sizes.
                let card = textSize.isAccessibilitySize
                    ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8)) : AnyLayout(HStackLayout(spacing: 12))
                card {
                    IconTile(symbol: "antenna.radiowaves.left.and.right", tint: .lora, side: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(model.nodeName.isEmpty ? String(localized: "MeshCore radio") : model.nodeName)
                            .font(.system(.body, weight: .semibold))
                        Text(radioLine)
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Badge(text: String(localized: "connected"), background: Theme.okBg, foreground: Theme.okFg)
                }
                .accessibilityElement(children: .combine)
            } else {
                NavigationLink {
                    PairingView()
                } label: {
                    HStack(spacing: 12) {
                        IconTile(symbol: "antenna.radiowaves.left.and.right", side: 44)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Pair a radio").font(.system(.body, weight: .semibold))
                            Text("No radio connected").font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        }
                        Spacer()
                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            // Over TCP, also while it reconnects: stops trying.
            if (model.isConnected || model.link == .tcp) && !model.isDemo {
                Button("Disconnect") { model.disconnectRadio() }
                    .buttonStyle(.destructive)
            }
        }
        .card(padding: 16)
    }

    /// "Bluetooth · 69 % · 357 s of airtime left", a figure never parted from its unit.
    private var radioLine: String {
        let left = Int((model.budget.limitMs - min(model.budget.usedMs, model.budget.limitMs)) / 1000)
        let battery = model.radioSettings?.batteryPercent.map { "\($0)\u{a0}%" }
        let airtime = String(localized: "\(left) s of airtime left").replacingOccurrences(of: "\(left) s", with: "\(left)\u{a0}s")
        return ([model.linkName] + (battery.map { [$0] } ?? []) + [airtime]).joined(separator: " · ")
    }

    private var voiceNote: String {
        switch model.voiceMode {
        case 0: String(localized: "Codec2 700C: 10 s ≈ 6 fragments on one hop. The least airtime, robotic.")
        case 3: String(localized: "Codec2 2400: 10 s ≈ 20 fragments on one hop. The clearest, three times the airtime.")
        default: String(localized: "Codec2 1600: 10 s ≈ 13 fragments on one hop. Clearer, twice the airtime.")
        }
    }

    private var irohLine: String {
        guard model.irohEnabled else { return String(localized: "Off") }
        if !model.bridgeOnline { return String(localized: "Starts when you send or download") }
        return model.relayOnly ? String(localized: "Online, through relays") : String(localized: "Fast; the peer sees your IP")
    }

    private var torLine: String {
        switch model.torStatus {
        case .off: String(localized: "Anonymous, slower")
        case .connecting: String(localized: "Connecting to Tor…")
        case .publishing: String(localized: "Publishing your onion service…")
        case .ready: String(localized: "Your onion service is reachable")
        case .failed: String(localized: "Tor could not connect")
        }
    }

    private var preset: String {
        guard let r = model.radio else { return String(localized: "Read from the radio once connected") }
        return "SF\(r.sf) / \(bandwidth(r.bwHz)) · CR 4/\(r.cr) · \(decimal(Double(r.freqKhz) / 1000, 3)) MHz"
    }

    private var subBand: String {
        let duty = Double(model.budget.limitMs) / 36_000
        let khz = model.radio?.freqKhz ?? 869_618
        let band = (869_400...869_650).contains(khz) ? "\(decimal(869.4, 1))–\(decimal(869.65, 2)) MHz"
                                                    : "\(decimal(Double(khz) / 1000, 3)) MHz"
        return "\(band) · \(String(format: "%.0f", duty)) %"
    }

    private var identityStatus: String {
        let f = model.fingerprint.prefix(2).joined(separator: " ")
        return model.phraseBackedUp
            ? String(localized: "Key \(f)… · backed up")
            : String(localized: "Key \(f)… · not backed up")
    }
}

/// Settings › Privacy › Blocked (ADR-0034): who, since when, and Unblock.
struct BlockedView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                if model.blocked.isEmpty {
                    Text("Nobody is blocked.").foregroundStyle(Theme.tx2)
                }
                ForEach(model.blocked, id: \.node) { b in
                    HStack(spacing: 12) {
                        Avatar(name: b.name, key: b.node)
                        VStack(alignment: .leading, spacing: 2) {
                            // A request's identity (ADR-0046), not the radio it came through.
                            Text(b.request ? String(localized: "Someone using \(b.name)’s radio") : b.name)
                                .font(.system(.body, weight: .semibold))
                            Text(String(localized: "Since \(Date(timeIntervalSince1970: TimeInterval(b.atMs) / 1000).formatted(date: .abbreviated, time: .omitted))"))
                                .font(.system(.footnote))
                                .foregroundStyle(Theme.tx2)
                        }
                        Spacer()
                        Button("Unblock") { model.unblock(b.node) }
                            .buttonStyle(.borderless)
                    }
                    .listRowBackground(Theme.s1)
                }
            } footer: {
                Text("A blocked person is not told. Their messages, introductions and group posts no longer reach this phone; their position and SOS in a group still show. What they send while blocked is lost.")
            }
        }
        .scrollContentBackground(.hidden)
        .background(Theme.bg)
        .navigationTitle("Blocked")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// How far our advert goes, chosen as in the stock app (B-user-4, D-14): nearby (zero hop) or
/// the whole mesh (flood), each with what it reaches; what it carries: the radio's name, and its
/// position when position in adverts is on and the radio has one.
struct AdvertChoiceSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                CardList {
                    option(flood: false, String(localized: "Nearby"), String(localized: "Only radios in direct range hear it."))
                    CardDivider()
                    option(flood: true, String(localized: "Whole mesh"),
                           String(localized: "Repeaters pass it on to every radio they reach."))
                }
                Text(carries)
                    .font(.footnote)
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 4)
                Spacer(minLength: 0)
            }
            .padding(16)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(Theme.bg)
            .navigationTitle("Send an advert")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private var carries: String {
        let r = model.radioSettings
        let name = r.map(\.name).flatMap { $0.isEmpty ? nil : $0 } ?? model.nodeName
        let position = r.map { $0.positionInAdverts && ($0.latE6 != 0 || $0.lonE6 != 0) } ?? false
        return position ? String(localized: "It shows your radio’s name, “\(name)” and its position.")
                        : String(localized: "It shows your radio’s name, “\(name)”.")
    }

    private func option(flood: Bool, _ title: String, _ note: String) -> some View {
        Button {
            dismiss()
            model.sendAdvert(flood: flood)
        } label: {
            CardRow(title: title, subtitle: note) {
                Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
