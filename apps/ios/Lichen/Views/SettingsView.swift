import SwiftUI
import UniformTypeIdentifiers

/// Radio, airtime, transports, position, appearance (mockup `reglages`).
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @AppStorage("appearance") private var appearance = Appearance.system
    @State private var advertSent = false
    @State private var naming = false
    @State private var meshName = ""
    @State private var nameError: String?
    @AppStorage("onboarded") private var onboarded = true
    @State private var importing = false
    @State private var importResult: String?
    @State private var exportingBackup = false
    @State private var radioOpen = false

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
                                CardRow(title: String(localized: "Radio settings"), subtitle: preset) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .disabled(!model.isConnected)
                            CardDivider()
                            // Radios to switch between (ADR-0042).
                            NavigationLink {
                                SavedRadiosView()
                            } label: {
                                CardRow(title: String(localized: "Radios"),
                                        subtitle: String(localized: "\(model.savedRadios.count) saved")) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider()
                            // What the radio knows of the mesh (§16).
                            NavigationLink {
                                NodesView()
                            } label: {
                                CardRow(title: String(localized: "Nodes"),
                                        subtitle: model.nodes.isEmpty ? String(localized: "Repeaters, rooms and radios around")
                                                                      : String(localized: "\(model.nodes.count) known")) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider()
                            CardRow(title: String(localized: "Sub-band"), subtitle: subBand)
                            CardDivider()
                            CardRow(title: String(localized: "Relay for others"),
                                    subtitle: String(localized: "Stock companion firmware does not repeat: a repeater node does that."))
                            CardDivider()
                            Button { model.sendAdvert(); advertSent = true } label: {
                                CardRow(title: String(localized: "Send an advert"),
                                        subtitle: advertSent ? String(localized: "Sent: radios around can add yours as a contact.")
                                                             : String(localized: "Lets radios around add yours as a contact")) {
                                    Image(systemName: "dot.radiowaves.left.and.right").foregroundStyle(Theme.lora)
                                }
                            }
                            .buttonStyle(.plain)
                            .disabled(!model.isConnected)
                            .opacity(model.isConnected ? 1 : 0.5)
                            CardDivider()
                            // What MeshCore users and channels see (protocol.md §15.4).
                            Button { meshName = model.nodeName; naming = true } label: {
                                CardRow(title: String(localized: "Name on the mesh"),
                                        subtitle: model.nodeName.isEmpty ? String(localized: "What people with a MeshCore app see")
                                                                         : model.nodeName) {
                                    Image(systemName: "pencil").foregroundStyle(Theme.tx2)
                                }
                            }
                            .buttonStyle(.plain)
                            .disabled(!model.isConnected)
                            .opacity(model.isConnected ? 1 : 0.5)
                        }
                        SectionLabel("Internet transports").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "iroh (direct P2P)"), subtitle: irohLine) {
                                Toggle("iroh (direct P2P)", isOn: $model.irohEnabled).labelsHidden()
                            }
                            CardDivider()
                            CardRow(title: String(localized: "Tor"), subtitle: torLine) {
                                Toggle("Tor", isOn: $model.torEnabled).labelsHidden()
                            }
                            CardDivider()
                            CardRow(title: String(localized: "Relay only"),
                                    subtitle: String(localized: "iroh through relays: contacts never see your IP")) {
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
                                    subtitle: String(localized: "Otherwise tap each one: GIPHY sees your IP address")) {
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
                                    subtitle: String(localized: "Distance from the predicted track")) {
                                Text("50 m").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                            }
                            CardDivider()
                            CardRow(title: String(localized: "Heartbeat when still"),
                                    subtitle: String(localized: "Contacts; groups every 30 min")) {
                                Text("15 min").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                            }
                        }
                        SectionLabel("Appearance").padding(.top, 10)
                        Picker("Appearance", selection: $appearance) {
                            ForEach(Appearance.allCases) { Text($0.label).tag($0) }
                        }
                        .pickerStyle(.segmented)
                        privacy
                        // History from the official MeshCore app (ADR-0033).
                        SectionLabel("History").padding(.top, 10)
                        CardList {
                            Button { importing = true } label: {
                                CardRow(title: String(localized: "Import from the MeshCore app"),
                                        subtitle: String(localized: "The database it exports: your direct messages and channel posts join your MeshCore chats.")) {
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
                                            subtitle: String(localized: "Messages, contacts, groups, settings and media in one file your 24 words open.")) {
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
                                        subtitle: String(localized: "Turn it off to see messages wait for the radio.")) {
                                    Toggle("Simulated radio connected",
                                           isOn: Binding(get: { !model.demoRadioOff }, set: { model.setDemoRadio(on: $0) }))
                                        .labelsHidden()
                                }
                                CardDivider()
                                Button { model.stopDemo() } label: {
                                    CardRow(title: String(localized: "Leave the demo"))
                                }
                                .buttonStyle(.plain)
                            } else {
                                Button { model.startDemo() } label: {
                                    CardRow(title: String(localized: "Try the demo network"),
                                            subtitle: String(localized: "A simulated MeshCore network with two other users and a group, in real time: the app's real core, without a radio.")) {
                                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                    }
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                            CardDivider()
                            Button { onboarded = false } label: {
                                CardRow(title: String(localized: "Show the welcome screens again"))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                        SectionLabel("About").padding(.top, 10)
                        CardList {
                            CardRow(title: String(localized: "Version")) {
                                Text(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "")
                                    .foregroundStyle(Theme.tx2)
                            }
                            CardDivider()
                            CardRow(title: String(localized: "Stickers"),
                                    subtitle: String(localized: "Noto Animated Emoji © Google, CC BY 4.0. Animations by Lottie (Airbnb).")) {
                                EmptyView()
                            }
                            CardDivider()
                            NavigationLink {
                                IdentityView()
                            } label: {
                                CardRow(title: String(localized: "Your identity"), subtitle: identityStatus) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                            }
                            .buttonStyle(.plain)
                            CardDivider()
                            NavigationLink {
                                LicencesView()
                            } label: {
                                CardRow(title: String(localized: "Licences"),
                                        subtitle: String(localized: "GNU GPL 3 or later, and what lichen includes")) {
                                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider()
                            // ADR-0045: no message, key, name or position in it.
                            NavigationLink {
                                ReportView()
                            } label: {
                                CardRow(title: String(localized: "Report a problem"),
                                        subtitle: String(localized: "A log without your messages, to send to the developers")) {
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
            CardRow(title: String(localized: "Lock lichen"), subtitle: unlockMethod) {
                Toggle("Lock lichen", isOn: Binding(get: { lock.enabled }, set: { on in Task { await lock.setEnabled(on) } }))
                    .labelsHidden()
                    .disabled(lock.authenticating)
            }
            CardDivider()
            CardRow(title: String(localized: "Lock after")) {
                Picker("Lock after", selection: $lock.delay) {
                    ForEach(AppLock.delays, id: \.self) { Text(Self.lockDelay($0)).tag($0) }
                }
                .pickerStyle(.menu)
                .labelsHidden()
            }
            .opacity(lock.enabled ? 1 : 0.5)
            .disabled(!lock.enabled)
            CardDivider()
            // People the user blocked (ADR-0034), to unblock.
            NavigationLink { BlockedView() } label: {
                CardRow(title: String(localized: "Blocked")) {
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

    private var radioCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            if model.isConnected {
                HStack(spacing: 12) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .foregroundStyle(Theme.lora)
                        .frame(width: 40, height: 40)
                        .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(model.nodeName.isEmpty ? String(localized: "MeshCore radio") : model.nodeName)
                            .font(.system(.body, weight: .semibold))
                        Text(model.isDemo ? String(localized: "Demo network · simulated companion") :
                                model.link == .tcp ? String(localized: "TCP · MeshCore companion · not encrypted") :
                                String(localized: "BLE · MeshCore companion"))
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                    }
                    Spacer()
                    Badge(text: String(localized: "connected"), background: Theme.okBg, foreground: Theme.okFg)
                }
            } else {
                NavigationLink {
                    PairingView()
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: "antenna.radiowaves.left.and.right")
                            .foregroundStyle(Theme.tx2)
                            .frame(width: 40, height: 40)
                            .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
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
            VStack(alignment: .leading, spacing: 4) {
                Text("Airtime left").font(.system(.footnote)).foregroundStyle(Theme.tx2)
                AirtimeMeter(usedMs: model.budget.usedMs, limitMs: model.budget.limitMs, large: true)
            }
            // Over TCP, also while it reconnects: stops trying.
            if (model.isConnected || model.link == .tcp) && !model.isDemo {
                Button("Disconnect") { model.disconnectRadio() }
                    .buttonStyle(.destructive)
            }
        }
        .card(padding: 16)
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
        return "SF\(r.sf) / \(bandwidth(r.bwHz)) · CR 4/\(r.cr) · \(String(format: "%.3f", Double(r.freqKhz) / 1000)) MHz"
    }

    private var subBand: String {
        let duty = Double(model.budget.limitMs) / 36_000
        let khz = model.radio?.freqKhz ?? 869_618
        let band = (869_400...869_650).contains(khz) ? "869.4–869.65 MHz" : String(format: "%.3f MHz", Double(khz) / 1000)
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
                            Text(b.name).font(.system(.body, weight: .semibold))
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
