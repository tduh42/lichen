import SwiftUI

// Mesh tools (protocol.md §15.8, §16; ADR-0041 to ADR-0044): the nodes the radio knows, a
// node's path and trace, room servers and repeaters, saved radios.

/// The Nodes tab: what the radio knows or heard, the most recently heard first (§16.1).
struct NodesView: View {
    @Environment(AppModel.self) private var model
    @State private var filter = Filter.all
    @State private var query = ""

    enum Filter: CaseIterable, Identifiable {
        case all, repeaters, rooms, people
        var id: Self { self }
        var label: String {
            switch self {
            case .all: String(localized: "All")
            case .repeaters: String(localized: "Repeaters")
            case .rooms: String(localized: "Rooms")
            case .people: String(localized: "People")
            }
        }
    }

    private var shown: [NodeInfo] {
        model.nodes.filter { n in
            let kind: Bool = switch filter {
            case .all: true
            case .repeaters: n.kind == .repeater
            case .rooms: n.kind == .roomServer
            case .people: n.kind == .companion || n.kind == .unknown
            }
            return kind && (query.isEmpty || n.name.localizedCaseInsensitiveContains(query))
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                Picker("Show", selection: $filter) {
                    ForEach(Filter.allCases) { Text($0.label).tag($0) }
                }
                .pickerStyle(.segmented)
                if model.nodes.isEmpty {
                    Text(model.isConnected ? String(localized: "The radio has not reported any node yet.")
                                           : String(localized: "Connect your radio to see the nodes it knows."))
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .padding(.top, 20)
                        .frame(maxWidth: .infinity)
                } else {
                    CardList {
                        ForEach(Array(shown.enumerated()), id: \.element.key) { i, n in
                            if i > 0 { CardDivider() }
                            NavigationLink {
                                NodeDetailView(key: n.key)
                            } label: {
                                NodeRow(node: n)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    Text("Repeaters, room servers and people’s radios, as your radio knows them. Those it heard without adding are marked; positions come from the nodes’ own adverts.")
                        .font(.system(.caption))
                        .foregroundStyle(Theme.tx2)
                        .padding(.horizontal, 4)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(16)
        }
        .screen()
        .searchable(text: $query, prompt: Text("Name"))
        .navigationTitle("Nodes")
        .navigationBarTitleDisplayMode(.large)
    }
}

/// A node in a list: its type, name, hops and when it was last heard.
struct NodeRow: View {
    let node: NodeInfo
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: node.kind.symbol)
                .foregroundStyle(node.kind == .repeater ? Theme.lora : Theme.tx2)
                .frame(width: 36, height: 36)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 9))
            VStack(alignment: .leading, spacing: 2) {
                Text(node.name).font(.system(.callout, weight: .medium)).foregroundStyle(Theme.tx)
                Text(detail).font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            Spacer(minLength: 8)
            if !node.onRadio {
                Badge(text: String(localized: "heard"), background: Theme.s2, foreground: Theme.tx2)
            }
            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .contentShape(Rectangle())
    }

    private var detail: String {
        [node.kind.label, NodeRow.hops(node.hops), model.heard(node.lastHeardS)].joined(separator: " · ")
    }

    static func hops(_ hops: UInt8?) -> String {
        switch hops {
        case nil: String(localized: "flood")
        case 0?: String(localized: "direct")
        case let h?: String(localized: "\(Int(h)) hops")
        }
    }
}

/// One node: its path and trace; a room's login; a repeater's administration (§16).
struct NodeDetailView: View {
    let key: Data
    @Environment(AppModel.self) private var model
    @State private var error: String?

    var body: some View {
        let node = model.node(key)
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                if let node { header(node) }
                SectionLabel("Path").padding(.top, 10)
                PathCard(key: key)
                TraceCard(key: key, error: $error)
                if node?.kind == .roomServer {
                    SectionLabel("Room").padding(.top, 10)
                    Button {
                        model.openRequest = .room(key: key)
                    } label: {
                        CardRow(title: String(localized: "Open the room"),
                                subtitle: String(localized: "Its posts, and yours")) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .card(padding: 0)
                    ServerLoginCard(key: key, room: true)
                }
                if node?.kind == .repeater {
                    SectionLabel("Administration").padding(.top, 10)
                    ServerLoginCard(key: key, room: false)
                    RepeaterCards(key: key, error: $error)
                }
            }
            .padding(16)
        }
        .screen()
        .navigationTitle(node?.name ?? String(localized: "Node"))
        .navigationBarTitleDisplayMode(.inline)
        .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    private func header(_ n: NodeInfo) -> some View {
        CardList {
            CardRow(title: n.kind.label, subtitle: [n.onRadio ? nil : String(localized: "Heard, not in the radio’s contacts"),
                                                    String(localized: "Last heard \(model.heard(n.lastHeardS))")]
                .compactMap { $0 }.joined(separator: " · "))
            if let lat = n.lat, let lon = n.lon {
                CardDivider()
                CardRow(title: String(localized: "Advertised position"),
                        subtitle: String(format: "%.5f, %.5f", lat, lon))
            }
            CardDivider()
            MeshCoreKeyRow(key: n.key)
        }
    }
}

/// The radio's path to a node, hop by hop (§16.2).
struct PathCard: View {
    let key: Data
    @Environment(AppModel.self) private var model

    var body: some View {
        CardList {
            if let path = model.nodePath(key) {
                if path.isEmpty {
                    CardRow(title: String(localized: "Direct"), subtitle: String(localized: "Your radio hears it without a repeater."))
                } else {
                    ForEach(Array(path.enumerated()), id: \.offset) { i, hop in
                        if i > 0 { CardDivider() }
                        CardRow(title: hop.display, subtitle: String(localized: "Hop \(i + 1) · \(hop.hash.uppercased())"))
                    }
                }
            } else {
                CardRow(title: String(localized: "No path known"),
                        subtitle: String(localized: "Your radio floods to reach it until it learns a path."))
            }
        }
    }
}

/// Traces the path there and back, and shows each hop's SNR (§16.3).
struct TraceCard: View {
    let key: Data
    @Binding var error: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        let trace = model.trace(key)
        VStack(alignment: .leading, spacing: 8) {
            Button {
                error = model.startTrace(key)
            } label: {
                Label(trace?.state == .sending ? String(localized: "Tracing…") : String(localized: "Trace the path"),
                      systemImage: "point.topleft.down.to.point.bottomright.curvepath")
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.secondary)
            .disabled(!model.isConnected || trace?.state == .sending)
            if let trace {
                CardList {
                    ForEach(Array(trace.legs.enumerated()), id: \.offset) { i, leg in
                        if i > 0 { CardDivider() }
                        CardRow(title: "\(end(leg.from)) → \(end(leg.to))") {
                            Text(verbatim: leg.snrDb.map { "\(decimal(Double($0), 1)) dB" } ?? "–")
                                .font(.system(.callout, design: .monospaced))
                                .foregroundStyle(color(leg.snrDb))
                        }
                    }
                }
                Text(summary(trace))
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                    .padding(.horizontal, 4)
            }
        }
        .padding(.top, 6)
    }

    private func end(_ hop: PathHop?) -> String {
        hop?.display ?? String(localized: "You")
    }

    private func color(_ snr: Float?) -> Color {
        guard let snr else { return Theme.tx2 }
        return snr >= 0 ? Theme.ok : snr >= -7 ? Theme.warn : Theme.sos
    }

    private func summary(_ t: TraceInfo) -> String {
        switch t.state {
        case .sending: String(localized: "On its way through the repeaters and back.")
        case .done: String(localized: "Back in \(decimal(Double(t.rttMs ?? 0) / 1000, 1)) s · SNR measured by each hop, out and back")
        case .noAnswer: String(localized: "No answer: a hop did not hear it, or the radio was disconnected.")
        case .refused: String(localized: "The radio refused the trace.")
        }
    }
}

/// Logging in to a room server or a repeater (§15.8, §16.5).
struct ServerLoginCard: View {
    let key: Data
    let room: Bool
    @Environment(AppModel.self) private var model
    @State private var password = ""
    @State private var remember = true
    @State private var error: String?

    var body: some View {
        let info = model.server(key)
        let login = info?.login ?? .out
        VStack(alignment: .leading, spacing: 8) {
            CardList {
                CardRow(title: String(localized: "Login"), subtitle: login.label)
                if login.isIn {
                    if info?.remembered == true {
                        CardDivider()
                        CardRow(title: String(localized: "Password kept on this phone"),
                                subtitle: String(localized: "Logs in at each connection, so that new posts come."))
                    }
                    CardDivider()
                    Button(role: .destructive) { model.serverLogout(key) } label: {
                        Text("Log out").frame(maxWidth: .infinity, minHeight: 44)
                    }
                } else {
                    CardDivider()
                    SecureField(room ? String(localized: "Room password") : String(localized: "Admin or guest password"),
                                text: $password)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .padding(.horizontal, 16)
                        .frame(minHeight: 44)
                    if room {
                        CardDivider()
                        CardRow(title: String(localized: "Remember on this phone")) {
                            Toggle("Remember on this phone", isOn: $remember).labelsHidden()
                        }
                    }
                    CardDivider()
                    Button {
                        error = model.serverLogin(key, password: password, remember: room && remember)
                    } label: {
                        Text(login == .pending ? String(localized: "Logging in…") : String(localized: "Log in"))
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    .disabled(!model.isConnected || login == .pending || password.utf8.count > 15)
                }
            }
            Text(room ? String(localized: "A room’s posts are MeshCore messages: the room and everyone logged in read them. Up to 15 characters.")
                      : String(localized: "The admin password is never kept on this phone. Guests can read the status only."))
                .font(.system(.caption))
                .foregroundStyle(Theme.tx2)
                .padding(.horizontal, 4)
                .fixedSize(horizontal: false, vertical: true)
            if let error {
                Text(error).font(.system(.footnote)).foregroundStyle(Theme.danger)
            }
        }
    }
}

/// A repeater's status, neighbours and command line (§16.5).
struct RepeaterCards: View {
    let key: Data
    @Binding var error: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        let info = model.server(key)
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Status").padding(.top, 10)
            CardList {
                if let s = info?.stats {
                    statusRows(s)
                    CardDivider()
                }
                Button { error = model.requestServerStats(key) } label: {
                    Text(info?.statsPending == true ? String(localized: "Asking…") : String(localized: "Read the status"))
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .disabled(!model.isConnected || info?.statsPending == true)
            }
            if info?.noAnswer == true {
                Text("No answer: log in first, or the repeater is out of reach.")
                    .font(.system(.footnote)).foregroundStyle(Theme.warnFg)
            }
            SectionLabel("Neighbours").padding(.top, 10)
            CardList {
                if let ns = info?.neighbours {
                    if ns.isEmpty && info?.neighboursPending != true {
                        CardRow(title: String(localized: "It hears nobody yet"))
                        CardDivider()
                    }
                    ForEach(Array(ns.enumerated()), id: \.offset) { _, n in
                        CardRow(title: n.name ?? n.prefix.uppercased(),
                                subtitle: String(localized: "Heard \(MapScreen.ago(UInt64(Date().timeIntervalSince1970) - UInt64(n.heardAgoS)))")) {
                            Text(verbatim: "\(decimal(Double(n.snrDb), 1)) dB")
                                .font(.system(.callout, design: .monospaced))
                                .foregroundStyle(Theme.tx2)
                        }
                        CardDivider()
                    }
                }
                Button { error = model.requestNeighbours(key) } label: {
                    Text(info?.neighboursPending == true ? String(localized: "Asking…") : String(localized: "Ask for its neighbours"))
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .disabled(!model.isConnected || info?.neighboursPending == true)
            }
            if let info, let ns = info.neighbours, Int(info.neighboursTotal) > ns.count {
                Text("\(ns.count) of \(Int(info.neighboursTotal)), the strongest first.")
                    .font(.system(.caption)).foregroundStyle(Theme.tx2).padding(.horizontal, 4)
            }
            SectionLabel("Command line").padding(.top, 10)
            NavigationLink {
                ConsoleView(key: key)
            } label: {
                CardRow(title: String(localized: "Console"),
                        subtitle: String(localized: "Commands for its admin, such as ver, clock or neighbors")) {
                    Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .card(padding: 0)
        }
    }

    @ViewBuilder
    private func statusRows(_ s: ServerStats) -> some View {
        CardRow(title: String(localized: "Battery"), subtitle: "\(decimal(Double(s.batteryMv) / 1000, 2)) V")
        CardDivider()
        CardRow(title: String(localized: "Up for"), subtitle: Self.duration(s.uptimeS))
        CardDivider()
        CardRow(title: String(localized: "Radio"),
                subtitle: String(localized: "Noise floor \(Int(s.noiseFloor)) dBm · last RSSI \(Int(s.lastRssi)) dBm · SNR \(decimal(Double(s.lastSnrDb), 1)) dB"))
        CardDivider()
        CardRow(title: String(localized: "Packets"),
                subtitle: String(localized: "\(Int(s.packetsRecv)) received · \(Int(s.packetsSent)) sent · \(Int(s.txQueue)) waiting"))
        CardDivider()
        CardRow(title: String(localized: "Airtime"),
                subtitle: String(localized: "\(Self.duration(s.airTimeS)) transmitting") +
                    (s.rxAirTimeS.map { " · " + String(localized: "\(Self.duration($0)) receiving") } ?? ""))
        CardDivider()
        CardRow(title: String(localized: "Flood and direct"),
                subtitle: String(localized: "Sent \(Int(s.sentFlood)) / \(Int(s.sentDirect)) · received \(Int(s.recvFlood)) / \(Int(s.recvDirect)) · duplicates \(Int(s.floodDups)) / \(Int(s.directDups))"))
        if let errors = s.recvErrors {
            CardDivider()
            CardRow(title: String(localized: "Receive errors"), subtitle: "\(errors)")
        }
        if let posted = s.posted {
            CardDivider()
            CardRow(title: String(localized: "Posts"), subtitle: String(localized: "\(Int(posted)) kept · \(Int(s.postPushes ?? 0)) sent to clients"))
        }
    }

    static func duration(_ s: UInt32) -> String {
        let f = DateComponentsFormatter()
        f.allowedUnits = s >= 86_400 ? [.day, .hour] : [.hour, .minute]
        f.unitsStyle = .abbreviated
        return f.string(from: TimeInterval(s)) ?? "\(s) s"
    }
}

/// A repeater's command line: what was sent and what came back (§16.5).
struct ConsoleView: View {
    let key: Data
    @Environment(AppModel.self) private var model
    @State private var command = ""
    @State private var error: String?

    var body: some View {
        let lines = model.server(key)?.console ?? []
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        Text("Commands go once, unacknowledged; the repeater answers its admin only.")
                            .font(.system(.caption))
                            .foregroundStyle(Theme.tx2)
                            .padding(.bottom, 6)
                        ForEach(Array(lines.enumerated()), id: \.offset) { i, l in
                            Text(l.outgoing ? (l.text.isEmpty ? String(localized: "(refused by the radio)") : "> " + l.text) : l.text)
                                .font(.system(.footnote, design: .monospaced))
                                .foregroundStyle(l.outgoing ? Theme.lora : Theme.tx)
                                .textSelection(.enabled)
                                .id(i)
                        }
                    }
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .onChange(of: lines.count) { _, n in
                    if n > 0 { withAnimation { proxy.scrollTo(n - 1, anchor: .bottom) } }
                }
            }
            if let error {
                Text(error).font(.system(.footnote)).foregroundStyle(Theme.danger).padding(.horizontal, 16)
            }
            HStack(spacing: 8) {
                TextField("Command", text: $command)
                    .font(.system(.body, design: .monospaced))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.send)
                    .onSubmit(send)
                    .padding(.horizontal, 12)
                    .frame(minHeight: 44)
                    .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
                Button(action: send) {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 30))
                }
                .disabled(command.trimmingCharacters(in: .whitespaces).isEmpty || !model.isConnected)
                .accessibilityLabel("Send")
            }
            .padding(12)
            .background(Theme.s1)
        }
        .screen()
        .navigationTitle(model.node(key)?.name ?? String(localized: "Console"))
        .navigationBarTitleDisplayMode(.inline)
    }

    private func send() {
        let c = command.trimmingCharacters(in: .whitespaces)
        guard !c.isEmpty else { return }
        error = model.sendServerCommand(key, c)
        if error == nil { command = "" }
    }
}

/// Settings › Radios: the radios this phone was paired with (ADR-0042).
struct SavedRadiosView: View {
    @Environment(AppModel.self) private var model
    @State private var forgetting: SavedRadio?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                if model.savedRadios.isEmpty {
                    Text("Radios you pair appear here, to switch between them.")
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                } else {
                    CardList {
                        ForEach(Array(model.savedRadios.enumerated()), id: \.element.address) { i, r in
                            if i > 0 { CardDivider() }
                            row(r)
                        }
                    }
                }
                // A radio of the list that did not answer when connected (TCP): said here, where
                // the user asked.
                if let error = model.tcpError {
                    Banner(kind: .warn, title: String(localized: "Could not reach the radio: \(error)"))
                }
                NavigationLink {
                    PairingView()
                } label: {
                    CardRow(title: String(localized: "Pair another radio")) {
                        Image(systemName: "plus").foregroundStyle(Theme.tx2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .card(padding: 0)
                Text("Your conversations, contacts and groups stay on this phone whichever radio you use. Each radio keeps its own MeshCore contacts, channels and settings, and the people you write to see a different node when you switch.")
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                    .padding(.horizontal, 4)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(16)
        }
        .screen()
        .navigationTitle("Radios")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { model.refreshSavedRadios() }
        .confirmationDialog("Forget \(forgetting?.name ?? "")?", isPresented: Binding(get: { forgetting != nil }, set: { if !$0 { forgetting = nil } }),
                            titleVisibility: .visible) {
            Button("Forget", role: .destructive) {
                if let r = forgetting { model.forgetRadio(r) }
                forgetting = nil
            }
        } message: {
            Text("It leaves this list; iOS may still list it as paired in Bluetooth settings.")
        }
    }

    private func row(_ r: SavedRadio) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(r.current ? Theme.lora : Theme.tx2)
                .frame(width: 36, height: 36)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 9))
            VStack(alignment: .leading, spacing: 2) {
                Text(r.name).font(.system(.callout, weight: .medium)).foregroundStyle(Theme.tx)
                Text(subtitle(r)).font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            Spacer(minLength: 8)
            if r.current {
                Badge(text: String(localized: "connected"), background: Theme.okBg, foreground: Theme.okFg)
            }
            Menu {
                if model.canSwitch(to: r) {
                    Button { model.switchRadio(r) } label: { Label("Connect", systemImage: "link") }
                }
                Button(role: .destructive) { forgetting = r } label: { Label("Forget", systemImage: "trash") }
            } label: {
                Image(systemName: "ellipsis.circle").foregroundStyle(Theme.tx2).frame(width: 44, height: 44)
            }
            .accessibilityLabel("Actions for \(r.name)")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    private func subtitle(_ r: SavedRadio) -> String {
        let link: String = switch r.link {
        case .bluetooth: String(localized: "Bluetooth")
        case .usb: String(localized: "USB")
        case .tcp: String(localized: "TCP")
        }
        let used = Date(timeIntervalSince1970: TimeInterval(r.lastUsedMs) / 1000)
            .formatted(date: .abbreviated, time: .shortened)
        return String(localized: "\(link) · last used \(used)")
    }
}

/// A channel's QR code in the stock app's format (protocol.md §15.3).
struct ChannelQRSheet: View {
    let channel: MeshChannel
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var shown = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 16) {
                    Banner(kind: .warn, title: String(localized: "Whoever scans it joins"),
                           message: String(localized: "This code holds the channel’s secret: anyone who scans it, with lichen or a MeshCore app, reads \(channel.name) and can post in it."))
                    if shown, let text = model.channelQr(channel.id), let image = QRCode.image(text) {
                        Image(uiImage: image)
                            .interpolation(.none)
                            .resizable()
                            .scaledToFit()
                            .frame(maxWidth: 260)
                            .padding(12)
                            .background(.white, in: RoundedRectangle(cornerRadius: 12))
                            .accessibilityLabel("QR code of \(channel.name)")
                        Text(channel.name).font(.system(.headline))
                    } else {
                        Button("Show the code") { shown = true }
                            .buttonStyle(.primary)
                    }
                }
                .padding(16)
            }
            .background(Theme.bg)
            .navigationTitle("Share the channel")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }
}

/// Scans a channel's QR code from a MeshCore app and joins it after confirmation (§15.3).
struct ScanChannelSheet: View {
    let open: (ChatID) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var code: ChannelCode?
    @State private var error: String?

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                if let code {
                    CardList {
                        CardRow(title: code.name, subtitle: code.kind.label)
                    }
                    Text(code.joined ? String(localized: "Your radio already follows this channel.")
                                     : String(localized: "Your radio will follow it, muted. Everyone who has this code reads it."))
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                    Button(code.joined ? String(localized: "Open") : String(localized: "Join the channel")) { join(code) }
                        .buttonStyle(.primary)
                    if let error {
                        Text(error).foregroundStyle(Theme.danger)
                    }
                    Spacer()
                } else {
                    QRScanner(prefixes: ["meshcore://channel/"]) { text in
                        if let c = model.parseChannelQr(text) {
                            code = c
                        } else {
                            error = String(localized: "Not a MeshCore channel code.")
                        }
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .frame(maxHeight: 360)
                    Text(error ?? String(localized: "Point the camera at a channel’s code from a MeshCore app or from lichen."))
                        .font(.system(.footnote))
                        .foregroundStyle(error == nil ? Theme.tx2 : Theme.danger)
                    Spacer()
                }
            }
            .padding(16)
            .background(Theme.bg)
            .navigationTitle("Scan a channel")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            #if DEBUG
            // -scanChannel <code>: the camera reads this channel code (the simulator has none).
            .onAppear {
                if code == nil, let text = UserDefaults.standard.string(forKey: "scanChannel") {
                    code = model.parseChannelQr(text)
                    if code == nil { error = String(localized: "Not a MeshCore channel code.") }
                }
            }
            #endif
        }
    }

    private func join(_ code: ChannelCode) {
        switch model.joinChannel(.privateChannel, name: code.name, secret: code.secretHex) {
        case let .success(chat):
            dismiss()
            open(chat)
        case let .failure(e):
            error = e.message
        }
    }
}
