import Charts
import SwiftUI

// Mesh tools (protocol.md §15.8, §16; ADR-0041 to ADR-0044): the nodes the radio knows, a
// node's path and trace, room servers and repeaters, saved radios.

/// The Nodes tab (mockup `Main`): who the radio hears directly and how well, then those it
/// reaches through repeaters (§16.1, ADR-0049).
struct NodesView: View {
    @Environment(AppModel.self) private var model
    @State private var filter = NodeFilter.all
    @State private var query = ""

    static let twoHours = NodeFilter.twoHours

    var body: some View {
        // Now, read again every 30 s: the "heard" times and the 2 h and 24 h limits move on.
        TimelineView(.periodic(from: .now, by: 30)) { context in
            content(now: UInt64(context.date.timeIntervalSince1970))
        }
        .screen()
        .navigationTitle("Nodes")
        .navigationBarTitleDisplayMode(.large)
        // Each node's distance and bearing from the phone.
        .onAppear { model.needsLocation("nodes", true) }
        .onDisappear { model.needsLocation("nodes", false) }
    }

    private func content(now: UInt64) -> some View {
        let shown = model.nodes.filter { n in
            filter.keeps(n, now: now) && (query.isEmpty || n.name.localizedCaseInsensitiveContains(query))
        }
        let sections = NodeInfo.sections(shown, now: now)
        let heard = model.nodes.filter { $0.heard(within: Self.twoHours, now: now) }.count
        return ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                Text(verbatim: String(localized: "\(model.nodes.count) known") + " · "
                     + String(localized: "\(heard) heard in the last 2 h"))
                    .font(.system(.footnote))
                    .foregroundStyle(Theme.tx2)
                    .padding(.horizontal, 4)
                chips(now: now)
                search
                if model.nodes.isEmpty {
                    Text(model.isConnected ? String(localized: "The radio has not reported any node yet.")
                                           : String(localized: "Connect your radio to see the nodes it knows."))
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .padding(.top, 20)
                        .frame(maxWidth: .infinity)
                } else {
                    if !sections.reach.isEmpty {
                        SectionLabel("In reach · heard directly by your radio").padding(.top, 6)
                        list(sections.reach, inReach: true, now: now)
                    }
                    if !sections.through.isEmpty {
                        SectionLabel("Through repeaters").padding(.top, 6)
                        list(sections.through, inReach: false, now: now)
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
    }

    /// A node by its name: on more lines with larger text rather than cut.
    private var search: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(Theme.tx2).accessibilityHidden(true)
            TextField("Name", text: $query, axis: .vertical)
                .lineLimit(1...3)
                .submitLabel(.search)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .onChange(of: query) { _, q in
                    if q.contains("\n") { query = q.replacingOccurrences(of: "\n", with: "") }
                }
        }
        .padding(.horizontal, 12)
        .frame(minHeight: 44)
        .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
    }

    /// The filters, each with its count (Heard in 2 h without).
    private func chips(now: UInt64) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(NodeFilter.allCases) { f in
                    let on = filter == f
                    let label = f.chip(model.nodes, now: now)
                    Button { filter = f } label: {
                        Text(verbatim: label)
                            .font(.system(.subheadline, weight: .semibold))
                            .foregroundStyle(on ? Theme.actFg : Theme.tx)
                            .fixedSize()
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(on ? Theme.actBg : Theme.s1, in: Capsule())
                            .overlay(Capsule().stroke(on ? Color.clear : Theme.line, lineWidth: 1))
                            .frame(minHeight: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(on ? .isSelected : [])
                }
            }
        }
    }

    private func list(_ nodes: [NodeInfo], inReach: Bool, now: UInt64) -> some View {
        CardList {
            ForEach(Array(nodes.enumerated()), id: \.element.key) { i, n in
                if i > 0 { CardDivider(inset: 68) }
                NavigationLink {
                    NodeDetailView(key: n.key)
                } label: {
                    NodeRow(node: n, inReach: inReach, now: now)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("node")
            }
        }
    }
}

/// A person's initials, or a node's kind on its tile (a repeater's in `lora`); larger with the
/// text.
struct NodeLeading: View {
    let node: NodeInfo
    var base: CGFloat = 40
    @ScaledMetric(relativeTo: .callout) private var scale: CGFloat = 1

    var body: some View {
        let size = base * scale
        if node.kind == .companion {
            // Initials in a text style (a fixed size does not follow the text size).
            let (bg, fg) = Theme.avatar(for: node.key)
            Text(initials(node.name))
                .font(.system(base >= 50 ? .headline : base >= 36 ? .footnote : .caption, weight: .semibold))
                .frame(width: size, height: size)
                .background(bg, in: Circle())
                .foregroundStyle(fg)
                .accessibilityHidden(true)
        } else {
            let repeater = node.kind == .repeater
            Image(systemName: node.kind.symbol)
                .font(.system(size: size * 0.45, weight: .medium))
                .foregroundStyle(repeater ? Theme.loraFg : Theme.tx)
                .frame(width: size, height: size)
                .background(repeater ? Theme.loraBg : Theme.s2, in: RoundedRectangle(cornerRadius: size * 0.27))
                .accessibilityHidden(true)
        }
    }
}

/// A node in the list: its name, kind, the repeaters it comes through, where it is, when it was
/// last heard; its signal (in reach) or how many repeaters away.
struct NodeRow: View {
    let node: NodeInfo
    var inReach = false
    var now = UInt64(Date().timeIntervalSince1970)
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 12) {
            NodeLeading(node: node)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(node.name)
                        .font(.system(.callout, weight: .semibold))
                        .foregroundStyle(Theme.tx)
                        .fixedSize(horizontal: false, vertical: true)
                    if node.lichen { LichenBadge() }
                    if !node.onRadio {
                        Badge(text: String(localized: "heard"), background: Theme.s2, foreground: Theme.tx2)
                    }
                }
                subtitle
                    .font(.system(.footnote))
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            if inReach {
                SignalMeter(rating: node.signal, snr: node.snrDb)
            } else {
                HopPill(hops: node.hops)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 11)
        .frame(minHeight: 44)
        .contentShape(Rectangle())
    }

    /// "Repeater · via hub · 1.2 km NE · 2 min ago", the last heard in `ok` within 2 h.
    private var subtitle: Text {
        let route = inReach ? nil : model.nodeRoute(node.key)
        let parts = [node.kindWord, route?.via.map { String(localized: "via \($0)") },
                     model.distanceBearing(lat: node.lat, lon: node.lon)].compactMap { $0 }
        let recent = node.heard(within: NodesView.twoHours, now: now)
        return Text(verbatim: parts.map { $0 + " · " }.joined()).foregroundStyle(Theme.tx2)
            + Text(verbatim: model.heard(node.lastHeardS)).foregroundStyle(recent ? Theme.ok : Theme.tx2)
    }
}

extension NodeInfo {
    /// What a node is, in a word: its kind; "MeshCore app" for someone with the stock app;
    /// nothing for a lichen user (the badge says it).
    var kindWord: String? {
        switch kind {
        case .companion where lichen: nil
        case .companion: String(localized: "MeshCore app")
        default: kind.label
        }
    }
}

/// One node (mockup `NodePage`): who it is and how well it is heard, how it reaches us (its
/// route, a trace), its signal over a day or a week; a room's login, a repeater's
/// administration (§16, ADR-0049).
struct NodeDetailView: View {
    let key: Data
    @Environment(AppModel.self) private var model
    @State private var error: String?
    /// Readings' Log in: the login card's password field takes the keyboard.
    @State private var loginRequest = 0

    var body: some View {
        let node = model.node(key)
        TimelineView(.periodic(from: .now, by: 30)) { context in
            let now = UInt64(context.date.timeIntervalSince1970)
            ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    if let node { header(node, now: now) }
                    SectionLabel("How it reaches you").padding(.top, 10)
                    RouteCard(key: key, error: $error)
                    if let node { SignalSection(node: node, now: now).padding(.top, 10) }
                    // What it sends: never for our own radio (Radio settings has its battery).
                    if let node, node.key != model.nodeKey {
                        ReadingsCard(node: node, now: now) {
                            withAnimation { proxy.scrollTo("login", anchor: .center) }
                            loginRequest += 1
                        }
                        .padding(.top, 10)
                    }
                    if let node { details(node).padding(.top, 10) }
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
                        ServerLoginCard(key: key, room: true, focusRequest: loginRequest).id("login")
                    }
                    if node?.kind == .repeater {
                        SectionLabel("Administration").padding(.top, 10)
                        ServerLoginCard(key: key, room: false, focusRequest: loginRequest).id("login")
                        RepeaterCards(key: key, error: $error)
                    }
                }
                .padding(16)
            }
            }
        }
        .screen()
        .navigationTitle(node?.name ?? String(localized: "Node"))
        .navigationBarTitleDisplayMode(.inline)
        // Its distance and bearing from the phone.
        .onAppear { model.needsLocation("node", true) }
        .onDisappear { model.needsLocation("node", false) }
        .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    /// The node on top: its tile, name, kind and distance, when last heard; its signal (heard
    /// directly) or how many repeaters away.
    private func header(_ n: NodeInfo, now: UInt64) -> some View {
        HStack(spacing: 14) {
            NodeLeading(node: n, base: 56)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(n.name).font(.system(.title3, weight: .bold)).foregroundStyle(Theme.tx)
                    if n.lichen { LichenBadge() }
                }
                let what = [n.kindWord, model.distanceBearing(lat: n.lat, lon: n.lon)].compactMap { $0 }
                if !what.isEmpty {
                    Text(verbatim: what.joined(separator: " · ")).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                }
                Text(String(localized: "Heard \(model.heard(n.lastHeardS))"))
                    .font(.system(.footnote, weight: .semibold))
                    .foregroundStyle(n.heard(within: NodesView.twoHours, now: now) ? Theme.ok : Theme.tx2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if n.signal != .none {
                SignalMeter(rating: n.signal, snr: n.snrDb, large: true)
            } else {
                HopPill(hops: n.hops)
            }
        }
        .padding(.horizontal, 2)
        .card(padding: 14)
    }

    /// Heard without being added, its advertised position, its MeshCore key.
    private func details(_ n: NodeInfo) -> some View {
        CardList {
            if !n.onRadio {
                CardRow(title: String(localized: "Heard, not in the radio’s contacts"))
                CardDivider()
            }
            if let lat = n.lat, let lon = n.lon {
                CardRow(title: String(localized: "Advertised position"),
                        subtitle: String(format: "%.5f, %.5f", lat, lon))
                CardDivider()
            }
            MeshCoreKeyRow(key: n.key)
        }
    }
}

/// How a node reaches us (§16.3): its route stop by stop, each leg's last signal; the last
/// trace, Trace again, Show on the map.
struct RouteCard: View {
    let key: Data
    @Binding var error: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        let route = model.nodeRoute(key)
        let trace = model.trace(key)
        let tracedAt = route?.tracedAt
        VStack(alignment: .leading, spacing: 12) {
            if let route, route.stops.count >= 2 {
                RouteRibbon(route: route)
            } else {
                Text("No path known").font(.system(.subheadline)).foregroundStyle(Theme.tx)
            }
            Text(status(trace, tracedAt: tracedAt))
                .font(.system(.footnote))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Button {
                    error = model.startTrace(key)
                } label: {
                    Label(trace?.state == .sending ? String(localized: "Tracing…")
                          : trace != nil || tracedAt != nil ? String(localized: "Trace again")
                          : String(localized: "Trace the path"),
                          systemImage: "arrow.clockwise")
                        .padding(.horizontal, 8)
                }
                .buttonStyle(.primary)
                .disabled(!model.isConnected || trace?.state == .sending)
                // Where the node, or a repeater on the way, advertises a position.
                if model.nodePlace(key) != nil {
                    Button {
                        model.showNodeOnMap(key)
                    } label: {
                        Label("Show on the map", systemImage: "map")
                    }
                    .buttonStyle(.secondary)
                    .accessibilityIdentifier("showOnMap")
                }
            }
        }
        .card(padding: 16)
    }

    /// When the legs were traced, and today's trace's figures; or what the trace is doing.
    private func status(_ trace: TraceInfo?, tracedAt: UInt64?) -> String {
        switch trace?.state {
        case .sending?: return String(localized: "On its way through the repeaters and back.")
        case .noAnswer?: return String(localized: "No answer: a hop did not hear it, or the radio was disconnected.")
        case .refused?: return String(localized: "The radio refused the trace.")
        case .done?:
            let when = model.heard(tracedAt ?? UInt64(Date().timeIntervalSince1970))
            let back = decimal(Double(trace?.rttMs ?? 0) / 1000, 1)
            let airtime = decimal(Double(trace?.airtimeMs ?? 0) / 1000, 1)
            return String(localized: "Traced \(when) · back in \(back) s · \(airtime) s of airtime")
        case nil:
            return tracedAt.map { String(localized: "Traced \(model.heard($0))") } ?? String(localized: "Not traced yet")
        }
    }
}

/// You → each repeater → the node, a leg between each two: its colour by rating and its SNR,
/// or a dashed `line` "not measured". Scrolls sideways when the stops do not fit.
struct RouteRibbon: View {
    let route: NodeRoute
    @Environment(AppModel.self) private var model
    @ScaledMetric(relativeTo: .caption) private var stopWidth: CGFloat = 62
    /// The first stop, us, wider: the radio's model under "You" on one line ("ThinkNode M5").
    @ScaledMetric(relativeTo: .caption) private var firstWidth: CGFloat = 92
    @ScaledMetric(relativeTo: .caption) private var minLeg: CGFloat = 56
    @State private var width: CGFloat = 0

    var body: some View {
        let you = String(localized: "You")
        let names = route.stops.enumerated().map { i, s in i == 0 ? you : s.display }
        let last = route.stops.count - 1
        // The legs share the room the stops leave, never narrower than `minLeg`.
        let legWidth = last > 0 ? max(minLeg, (width - firstWidth - stopWidth * CGFloat(last)) / CGFloat(last)) : minLeg
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 0) {
                ForEach(Array(route.stops.enumerated()), id: \.offset) { i, stop in
                    RouteStopView(stop: stop, first: i == 0, last: i == last, name: names[i])
                        .frame(width: i == 0 ? firstWidth : stopWidth)
                    if i < last {
                        RouteLegView(leg: i < route.legs.count ? route.legs[i] : nil, from: names[i], to: names[i + 1])
                            .frame(width: legWidth)
                    }
                }
            }
        }
        .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }
}

/// A stop: our radio (its initials on a dark disc, "You" and its name), a repeater (on its
/// `lora` tile), or the node (its own tile or avatar, ringed).
struct RouteStopView: View {
    let stop: RouteStop
    let first: Bool
    let last: Bool
    let name: String
    @Environment(AppModel.self) private var model
    @ScaledMetric(relativeTo: .caption) private var side: CGFloat = 40

    var body: some View {
        VStack(spacing: 5) {
            ZStack {
                if first {
                    Text(initials(model.nodeName.isEmpty ? name : model.nodeName))
                        .font(.system(.caption, weight: .bold))
                        .foregroundStyle(Theme.actFg)
                        .frame(width: side * 0.85, height: side * 0.85)
                        .background(Theme.actBg, in: Circle())
                } else if last, let key = stop.key, let node = model.node(key) {
                    NodeLeading(node: node, base: 30)
                        .padding(3)
                        .overlay {
                            if node.kind == .companion {
                                Circle().stroke(Theme.tx2, lineWidth: 2)
                            } else {
                                RoundedRectangle(cornerRadius: side * 0.27).stroke(Theme.tx2, lineWidth: 2)
                            }
                        }
                } else {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: side * 0.4, weight: .medium))
                        .foregroundStyle(Theme.loraFg)
                        .frame(width: side * 0.85, height: side * 0.85)
                        .background(Theme.loraBg, in: RoundedRectangle(cornerRadius: side * 0.22))
                }
            }
            .frame(width: side, height: side)
            .accessibilityHidden(true)
            Text(verbatim: name)
                .font(.system(.caption, weight: .semibold))
                .foregroundStyle(Theme.tx)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            if first && !model.nodeName.isEmpty {
                // On one line in its wider stop (`RouteRibbon.firstWidth`, which grows with the
                // text); on two rather than cut when longer.
                Text(verbatim: model.nodeName)
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// A leg: a line in its rating's colour with its SNR under it, or a dashed `line` "not
/// measured"; read as "hub to crest, −3.0 dB".
struct RouteLegView: View {
    let leg: RouteLeg?
    let from: String
    let to: String
    @ScaledMetric(relativeTo: .caption) private var top: CGFloat = 18

    var body: some View {
        let snr = leg?.snrDb
        let rating = leg?.signal ?? .none
        let label = snr.map(dbText) ?? String(localized: "not measured")
        VStack(spacing: 4) {
            if snr != nil {
                Capsule().fill(rating != .none ? rating.color : Theme.tx2).frame(height: 4)
            } else {
                // Nothing measured it: dashed, in `line`.
                HorizontalLine()
                    .stroke(Theme.line, style: StrokeStyle(lineWidth: 4, lineCap: .round, dash: [8, 5]))
                    .frame(height: 4)
            }
            Text(verbatim: label)
                .font(.system(.caption, weight: snr != nil ? .semibold : .regular).monospacedDigit())
                .foregroundStyle(snr != nil && rating != .none ? rating.color : Theme.tx2)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.top, top)
        .padding(.horizontal, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(String(localized: "\(from) to \(to), \(label)"))
    }
}

/// A horizontal line through the middle of its frame.
struct HorizontalLine: Shape {
    func path(in rect: CGRect) -> Path {
        var p = Path()
        p.move(to: CGPoint(x: rect.minX, y: rect.midY))
        p.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
        return p
    }
}

/// A node's signal over the last 24 h or 7 days (ADR-0049); for a node heard only through
/// repeaters, why there is none.
struct SignalSection: View {
    let node: NodeInfo
    let now: UInt64
    @Environment(AppModel.self) private var model
    @State private var week = false

    var body: some View {
        let history = model.nodeSignalHistory(node.key)
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                SectionLabel(week ? "Signal, last 7 days" : "Signal, last 24 h")
                if !history.isEmpty {
                    Picker(selection: $week) {
                        Text("24 h").tag(false)
                        Text("7 days").tag(true)
                    } label: {
                        EmptyView()
                    }
                    .pickerStyle(.segmented)
                    .fixedSize()
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                if history.isEmpty {
                    Text("Heard only through repeaters: no signal of its own. The route above shows how it reaches you.")
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                } else {
                    let window: UInt64 = week ? 7 * 86_400 : 86_400
                    let points = history.filter { now <= $0.atS || now - $0.atS <= window }
                    if points.isEmpty {
                        Text("No reading in this time.").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                    } else {
                        SignalChart(points: points, now: now, window: window, limits: model.signalLimits)
                    }
                    HStack(alignment: .firstTextBaseline) {
                        Text(week ? "7 days ago" : "24 h ago")
                        Spacer(minLength: 8)
                        Text(String(localized: "now \(dbText(node.snrDb ?? history[history.count - 1].snrDb))"))
                            .monospacedDigit()
                    }
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .card(padding: 14)
        }
    }
}

/// The readings in their window: a dot each, coloured by its rating, joined by a thin line; dB
/// up the side. Behind them, faint bands at the preset's limits (ADR-0049): Good above the
/// first, Fair down to the second, weak below; the dB range takes both in. None before a radio.
struct SignalChart: View {
    let points: [SignalReading]
    let now: UInt64
    let window: UInt64
    /// The SNRs from which a reading is Good, and Fair, under the radio's preset (ADR-0049): faint
    /// bands behind the dots. None before a radio.
    let limits: SignalLimits?
    @ScaledMetric(relativeTo: .caption) private var labelRoom: CGFloat = 16

    private static let height: CGFloat = 92

    var body: some View {
        let good = limits.map { Double($0.goodSnrDb) }
        let fair = limits.map { Double($0.fairSnrDb) }
        let values = points.map { Double($0.snrDb) } + [good, fair].compactMap { $0 }
        // At least 6 dB tall, a decibel of room above and below; both limits inside, with room
        // above the Good one for its label, however low the readings.
        let lo = min((values.min() ?? 0) - 1, (values.max() ?? 0) - 6).rounded(.down)
        let hi = max((values.max() ?? 0) + 1, lo + 6, (good ?? -.infinity) + 3).rounded(.up)
        let start = Date(timeIntervalSince1970: TimeInterval(now > window ? now - window : 0))
        let end = Date(timeIntervalSince1970: TimeInterval(now))
        Chart {
            // The bands first, faint: the dots stay the main thing.
            if let good, let fair {
                RectangleMark(yStart: .value("dB", good), yEnd: .value("dB", hi))
                    .foregroundStyle(Theme.okBg.opacity(Self.band))
                RectangleMark(yStart: .value("dB", fair), yEnd: .value("dB", good))
                    .foregroundStyle(Theme.warnBg.opacity(Self.band))
                RectangleMark(yStart: .value("dB", lo), yEnd: .value("dB", fair))
                    .foregroundStyle(Theme.sosBg.opacity(Self.band))
                // Each limit named at its left end, just above it.
                ForEach([(String(localized: "Good"), good), (String(localized: "Fair"), fair)], id: \.0) { label, db in
                    RuleMark(y: .value("dB", db))
                        .foregroundStyle(.clear)
                        .annotation(position: .top, alignment: .leading, spacing: 1) {
                            Text(label).font(.system(.caption)).foregroundStyle(Theme.tx2).padding(.leading, 4)
                        }
                }
            }
            ForEach(Array(points.enumerated()), id: \.offset) { _, p in
                LineMark(x: .value("Time", Date(timeIntervalSince1970: TimeInterval(p.atS))),
                         y: .value("dB", Double(p.snrDb)))
                    .foregroundStyle(Theme.tx2.opacity(0.5))
                    .lineStyle(StrokeStyle(lineWidth: 1.5))
            }
            ForEach(Array(points.enumerated()), id: \.offset) { _, p in
                PointMark(x: .value("Time", Date(timeIntervalSince1970: TimeInterval(p.atS))),
                          y: .value("dB", Double(p.snrDb)))
                    .foregroundStyle(p.signal == .none ? Theme.tx2 : p.signal.color)
                    .symbolSize(36)
            }
        }
        .chartXScale(domain: start...max(end, start.addingTimeInterval(1)))
        .chartYScale(domain: lo...hi)
        .chartXAxis(.hidden)
        .chartYAxis {
            AxisMarks(position: .leading, values: ticks(lo, hi, good, fair)) { value in
                AxisGridLine().foregroundStyle(Theme.line)
                AxisValueLabel {
                    if let db = value.as(Double.self) {
                        Text(verbatim: dbText(Float(db)).replacingOccurrences(of: " dB", with: ""))
                            .font(.system(.caption).monospacedDigit())
                            .foregroundStyle(Theme.tx2)
                    }
                }
            }
        }
        .frame(height: Self.height)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(said)
        // For the UI tests (never read aloud): the limits drawn and the dB range.
        .accessibilityIdentifier(limits.map { "signalChart \($0.goodSnrDb) \($0.fairSnrDb) \(lo) \(hi)" } ?? "signalChart")
    }

    private static let band = 0.6

    /// "Signal chart: 12 readings, now 7.5 dB, good above 0.0 dB, fair above −5.0 dB".
    private var said: String {
        let base = String(localized: "Signal chart: \(points.count) readings, now \(dbText(points[points.count - 1].snrDb))")
        guard let limits else { return base }
        return base + ", " + String(localized: "good above \(dbText(limits.goodSnrDb)), fair above \(dbText(limits.fairSnrDb))")
    }

    /// dB up the side: at the limits when there are some, then the top and the bottom where they
    /// leave room for their figures; else the top, the middle and the bottom.
    private func ticks(_ lo: Double, _ hi: Double, _ good: Double?, _ fair: Double?) -> [Double] {
        guard let good, let fair else { return [lo, (lo + hi) / 2, hi] }
        let room = (hi - lo) * Double(labelRoom) / Double(Self.height)
        var ticks: [Double] = []
        for db in [good, fair, hi, lo] where ticks.allSatisfy({ abs($0 - db) >= room }) { ticks.append(db) }
        return ticks
    }
}

/// "Readings it sends" (mockup `NodePage`, §16.7, ADR-0050): its battery, its temperature and
/// whatever else it sent, when it sent them, and Ask again; or why there is nothing yet. Only
/// the buttons ask: opening the page never does. A repeater or a room answers only those logged
/// in: Log in takes the user to its login card.
struct ReadingsCard: View {
    let node: NodeInfo
    let now: UInt64
    let toLogin: () -> Void
    @Environment(AppModel.self) private var model

    private var server: Bool { node.kind == .repeater || node.kind == .roomServer }

    var body: some View {
        let readings = model.nodeReadings(node.key)
        let state = model.nodeReadingsState(node.key)
        let refusal = model.readingsRefusals[node.key]
        let last = readings.last
        let loggedIn = !server || (model.server(node.key)?.login ?? .out).isIn
        let refused: Bool = {
            if state == .refused { return true }
            if case .UnknownContact? = refusal { return true }
            return false
        }()
        let loginLine = node.kind == .roomServer
            ? String(localized: "A room server answers only those logged in, as a guest at least.")
            : String(localized: "A repeater answers only those logged in, as a guest at least.")
        // What became of the last request, until the next one.
        let outcome: String? = refused ? String(localized: "Your radio can’t ask it: it isn’t one of its contacts.")
            : refusal.map(\.display)
            ?? (state == .answeredEmpty ? String(localized: "It answered without a reading lichen reads.")
                : state == .noAnswer ? (server ? String(localized: "No answer. It may be out of reach.")
                                               : String(localized: "No answer. Someone with the MeshCore app answers only if they allow it."))
                : nil)
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("Readings it sends")
            CardList {
                if let last {
                    if let mv = last.batteryMv {
                        let volts = decimal(Double(mv) / 1000, 2)
                        ReadingRow(label: String(localized: "Battery"),
                                   value: last.batteryPercent.map { String(localized: "\(volts) V · about \(Int($0)) %") } ?? "\(volts) V")
                        CardDivider()
                    }
                    if let t = last.temperatureC {
                        ReadingRow(label: String(localized: "Temperature"), value: "\(Self.decimals(t, 1)) °C")
                        CardDivider()
                    }
                    ForEach(Array(last.values.enumerated()), id: \.offset) { _, v in
                        let label = Self.label(v.kind)
                        ReadingRow(label: v.channel == 1 ? label : String(localized: "\(label) · sensor \(Int(v.channel))"),
                                   value: Self.value(v))
                        CardDivider()
                    }
                    // Its battery over the week, once there are two to join.
                    let battery = readings.filter { $0.batteryMv != nil && (now <= $0.atS || now - $0.atS <= 7 * 86_400) }
                    if battery.count >= 2 {
                        BatteryChart(points: battery, now: now)
                        CardDivider()
                    }
                    HStack(spacing: 12) {
                        Text(String(localized: "Sent \(model.heard(last.atS))"))
                            .font(.system(.subheadline))
                            .foregroundStyle(Theme.tx2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .fixedSize(horizontal: false, vertical: true)
                        action(state: state, refused: refused, loggedIn: loggedIn, again: true)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 6)
                    .frame(minHeight: 44)
                    if let note = outcome ?? (loggedIn ? nil : loginLine) {
                        Text(note)
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 16)
                            .padding(.bottom, 12)
                    }
                } else {
                    // Nothing yet: what to do, or why it cannot be done.
                    HStack(spacing: 12) {
                        Text(outcome ?? (loggedIn ? String(localized: "Ask it for its battery and temperature.") : loginLine))
                            .font(.system(.subheadline))
                            .foregroundStyle(outcome != nil ? Theme.tx2 : Theme.tx)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .fixedSize(horizontal: false, vertical: true)
                        action(state: state, refused: refused, loggedIn: loggedIn, again: false)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 44)
                }
            }
        }
    }

    /// Ask, Ask again, Log in; "Asking…" while it waits; nothing when the radio cannot ask.
    @ViewBuilder
    private func action(state: ReadingsState, refused: Bool, loggedIn: Bool, again: Bool) -> some View {
        if refused {
            EmptyView()
        } else if state == .asking {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Asking…").font(.system(.subheadline)).foregroundStyle(Theme.tx2)
            }
            .accessibilityElement(children: .combine)
        } else if !loggedIn {
            button(String(localized: "Log in"), toLogin)
        } else {
            button(again ? String(localized: "Ask again") : String(localized: "Ask")) { model.askReadings(node.key) }
        }
    }

    private func button(_ title: String, _ act: @escaping () -> Void) -> some View {
        Button(action: act) {
            Text(title)
                .font(.system(.subheadline, weight: .semibold))
                .foregroundStyle(Theme.tx)
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
        .disabled(!model.isConnected)
        .accessibilityIdentifier("readingsAction")
    }

    /// "11.5", "4": `max` decimals at most, none when it has none, in the phone's language.
    static func decimals(_ v: Double, _ max: Int) -> String {
        v.formatted(.number.precision(.fractionLength(0...max)).grouping(.never))
    }

    static func label(_ kind: ReadingKind) -> String {
        switch kind {
        case .analogInput: String(localized: "Analog input")
        case .generic: String(localized: "Generic")
        case .illuminance: String(localized: "Illuminance")
        case .temperature: String(localized: "Temperature")
        case .humidity: String(localized: "Humidity")
        case .pressure: String(localized: "Pressure")
        case .voltage: String(localized: "Voltage")
        case .current: String(localized: "Current")
        case .frequency: String(localized: "Frequency")
        case .percentage: String(localized: "Percentage")
        case .altitude: String(localized: "Altitude")
        case .power: String(localized: "Power")
        case .distance: String(localized: "Distance")
        case .latitude: String(localized: "Latitude")
        case .longitude: String(localized: "Longitude")
        }
    }

    /// A value with its unit, as many decimals as it needs: volts two, a position four. A
    /// position is only listed here, never put on the map.
    static func value(_ v: ReadingValue) -> String {
        let number = switch v.kind {
        case .voltage: decimal(v.value, 2)
        case .latitude, .longitude: decimal(v.value, 4)
        case .current: decimals(v.value, 3)
        default: decimals(v.value, 2)
        }
        if v.unit.isEmpty { return number }
        return v.unit == "°" ? number + "°" : "\(number) \(v.unit)"
    }
}

/// A label and its value, read as one: "Battery, 3.92 V, about 69 %".
struct ReadingRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(label)
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            Text(verbatim: value)
                .font(.system(.subheadline, weight: .medium).monospacedDigit())
                .foregroundStyle(Theme.tx)
                .multilineTextAlignment(.trailing)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .frame(minHeight: 44)
        // Read as one line of text, the whole row's frame.
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: "\(label), \(value.replacingOccurrences(of: " · ", with: ", "))"))
        .accessibilityAddTraits(.isStaticText)
    }
}

/// Its battery over 7 days: a dot per answer joined by a thin line, volts up the side, small.
struct BatteryChart: View {
    let points: [NodeReading]
    let now: UInt64

    var body: some View {
        let volts = points.map { Double($0.batteryMv ?? 0) / 1000 }
        let lo = min((volts.min() ?? 0) - 0.02, (volts.max() ?? 0) - 0.1)
        let hi = max((volts.max() ?? 0) + 0.02, lo + 0.1)
        let window: UInt64 = 7 * 86_400
        let start = Date(timeIntervalSince1970: TimeInterval(now > window ? now - window : 0))
        let end = Date(timeIntervalSince1970: TimeInterval(now))
        VStack(alignment: .leading, spacing: 4) {
            Chart {
                ForEach(Array(points.enumerated()), id: \.offset) { i, p in
                    LineMark(x: .value("Time", Date(timeIntervalSince1970: TimeInterval(p.atS))), y: .value("V", volts[i]))
                        .foregroundStyle(Theme.tx2.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1.5))
                }
                ForEach(Array(points.enumerated()), id: \.offset) { i, p in
                    PointMark(x: .value("Time", Date(timeIntervalSince1970: TimeInterval(p.atS))), y: .value("V", volts[i]))
                        .foregroundStyle(Theme.tx)
                        .symbolSize(28)
                }
            }
            .chartXScale(domain: start...max(end, start.addingTimeInterval(1)))
            .chartYScale(domain: lo...hi)
            .chartXAxis(.hidden)
            .chartYAxis {
                AxisMarks(position: .leading, values: [lo, hi]) { value in
                    AxisGridLine().foregroundStyle(Theme.line)
                    AxisValueLabel {
                        if let v = value.as(Double.self) {
                            Text(verbatim: decimal(v, 2)).font(.system(.caption).monospacedDigit()).foregroundStyle(Theme.tx2)
                        }
                    }
                }
            }
            .frame(height: 60)
            Text("7 days").font(.system(.caption)).foregroundStyle(Theme.tx2)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(String(localized: "Battery over 7 days, from \(decimal(volts.first ?? 0, 2)) to \(decimal(volts.last ?? 0, 2)) V"))
    }
}

/// Logging in to a room server or a repeater (§15.8, §16.5).
struct ServerLoginCard: View {
    let key: Data
    let room: Bool
    /// Bumped to give the password field the keyboard (a node's readings' Log in).
    var focusRequest = 0
    @Environment(AppModel.self) private var model
    @FocusState private var passwordFocused: Bool
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
                        // The theme's red: the system's is too pale on a white card.
                        Text("Log out").foregroundStyle(Theme.sos).frame(maxWidth: .infinity, minHeight: 44)
                    }
                } else {
                    CardDivider()
                    SecureField(room ? String(localized: "Room password") : String(localized: "Admin or guest password"),
                                text: $password)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($passwordFocused)
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
                    .accessibilityIdentifier("serverLogin")
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
        .onChange(of: focusRequest) { passwordFocused = true }
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
