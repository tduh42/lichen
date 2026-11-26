import Foundation

// Mesh tools (protocol.md §15.8, §16; ADR-0041 to ADR-0044): the nodes the radio knows, paths
// and traces, room servers and repeaters, saved radios, channels by QR code.

extension NodeKind {
    var label: String {
        switch self {
        case .unknown: String(localized: "Node")
        case .companion: String(localized: "Companion")
        case .repeater: String(localized: "Repeater")
        case .roomServer: String(localized: "Room server")
        case .sensor: String(localized: "Sensor")
        }
    }

    var symbol: String {
        switch self {
        case .unknown: "circle.dashed"
        case .companion: "person"
        case .repeater: "antenna.radiowaves.left.and.right"
        case .roomServer: "bubble.left.and.bubble.right"
        case .sensor: "thermometer.medium"
        }
    }
}

extension ServerLogin {
    var label: String {
        switch self {
        case .out: String(localized: "Not logged in")
        case .pending: String(localized: "Logging in…")
        case .guest: String(localized: "Logged in as a guest")
        case .readOnly: String(localized: "Logged in: reading only")
        case .readWrite: String(localized: "Logged in")
        case .admin: String(localized: "Logged in as admin")
        case .refused: String(localized: "Refused")
        case .noAnswer: String(localized: "No answer: a wrong password, or out of reach")
        }
    }

    var isIn: Bool {
        switch self {
        case .guest, .readOnly, .readWrite, .admin: true
        default: false
        }
    }
}

extension PathHop {
    /// "hub", "hub or crest", or the hash when no repeater has it.
    var display: String {
        names.isEmpty ? hash.uppercased() : ListFormatter.localizedString(byJoining: names)
    }
}

extension SignalRating {
    /// Good first: the Nodes tab's order.
    var rank: Int {
        switch self {
        case .good: 0
        case .fair: 1
        case .bad: 2
        case .none: 3
        }
    }
}

extension NodeInfo {
    /// Heard directly in the last 24 h (ADR-0049): the Nodes tab's "In reach".
    func inReach(now: UInt64) -> Bool {
        guard signal != .none, let at = signalAtS else { return false }
        return now <= at || now - at <= 86_400
    }

    /// Heard at all, directly or not, in the last `seconds`.
    func heard(within seconds: UInt64, now: UInt64) -> Bool {
        guard let at = lastHeardS else { return false }
        return now <= at || now - at <= seconds
    }

    /// The Nodes tab's two sections: in reach, by rating then SNR; through repeaters, by hops
    /// then the last heard, the nodes with no known path last.
    static func sections(_ nodes: [NodeInfo], now: UInt64) -> (reach: [NodeInfo], through: [NodeInfo]) {
        let reach = nodes.filter { $0.inReach(now: now) }
            .sorted { a, b in
                a.signal.rank != b.signal.rank ? a.signal.rank < b.signal.rank : (a.snrDb ?? -.infinity) > (b.snrDb ?? -.infinity)
            }
        let through = nodes.filter { !$0.inReach(now: now) }
            .sorted { a, b in
                if (a.hops == nil) != (b.hops == nil) { return b.hops == nil }
                if a.hops != b.hops { return (a.hops ?? 0) < (b.hops ?? 0) }
                return (a.lastHeardS ?? 0) > (b.lastHeardS ?? 0)
            }
        return (reach, through)
    }
}

extension RouteStop {
    /// Its names joined (a hash several repeaters share), else its hash.
    var display: String {
        names.isEmpty ? (hash?.uppercased() ?? "?") : names.joined(separator: " / ")
    }
}

extension NodeRoute {
    /// The repeaters it goes through ("hub, crest"), when its path names them.
    var via: String? {
        let names = stops.dropFirst().dropLast().map { $0.names.joined(separator: " / ") }.filter { !$0.isEmpty }
        return names.isEmpty ? nil : names.joined(separator: ", ")
    }

    /// When the last trace measured its legs.
    var tracedAt: UInt64? { legs.filter(\.traced).compactMap(\.atS).max() }
}

/// An SNR as people read it: "7.5 dB", "−3.0 dB" (a true minus), the decimal of the language.
func dbText(_ snr: Float) -> String {
    (snr <= -0.05 ? "−" : "") + decimal(Double(abs(snr)), 1) + " dB"
}

extension AppModel {
    func node(_ key: Data) -> NodeInfo? { nodes.first { $0.key == key } }

    /// The route to `key` (protocol §16.3): our radio, the repeaters of its path, the node; each
    /// leg's last SNR towards us, or none when nothing measured it. Read again when the nodes or
    /// a trace change.
    func nodeRoute(_ key: Data) -> NodeRoute? {
        _ = meshToolsVersion
        _ = nodes
        return core.nodeRoute(node: key)
    }

    /// What our radio heard of `key` directly, the last 7 days (ADR-0049), oldest first.
    func nodeSignalHistory(_ key: Data) -> [SignalReading] {
        _ = meshToolsVersion
        _ = nodes
        return core.nodeSignalHistory(node: key).sorted { $0.atS < $1.atS }
    }

    /// Where `key` shows on the map: its advertised position, else the farthest stop of its
    /// route that advertises one; nil when none does.
    func nodePlace(_ key: Data) -> (lat: Double, lon: Double)? {
        if let n = node(key), let lat = n.lat, let lon = n.lon { return (lat, lon) }
        return nodeRoute(key)?.stops.dropFirst().last { $0.lat != nil && $0.lon != nil }.map { ($0.lat ?? 0, $0.lon ?? 0) }
    }

    /// How many stops of `key`'s route the map can place: ours at the phone, the others where
    /// they advertise. Fewer than two: no route to draw.
    func routePlaces(_ key: Data) -> Int {
        guard let route = nodeRoute(key) else { return 0 }
        return (myFix != nil ? 1 : 0) + route.stops.dropFirst().filter { $0.lat != nil && $0.lon != nil }.count
    }

    /// The Map tab, in Positions mode, with `key`'s route (a node's page's Show on the map): the
    /// camera takes the route in, or centres on the node when there is none to draw.
    func showNodeOnMap(_ key: Data) {
        guard let (lat, lon) = nodePlace(key) else { return }
        mapRouteTo = key
        if routePlaces(key) < 2 {
            mapFocus = MapPin(lat: lat, lon: lon, version: max(mapFocus?.version ?? 0, mapPin?.version ?? 0) + 1)
        }
        openMap()
    }

    /// "1.2 km NE", "300 m S" from the phone, when both positions are known.
    func distanceBearing(lat: Double?, lon: Double?) -> String? {
        guard let lat, let lon, let d = distanceTo(lat: lat, lon: lon) else { return nil }
        let points = String(localized: "N,NE,E,SE,S,SW,W,NW").split(separator: ",").map(String.init)
        guard points.count == 8 else { return distanceText(d.meters) }
        let point = points[Int(((d.bearing + 22.5) / 45).rounded(.down)) % 8]
        return "\(distanceText(d.meters)) \(point)"
    }

    /// "5 min ago", from a Unix time in seconds.
    func heard(_ unixS: UInt64?) -> String {
        guard let unixS else { return String(localized: "never heard") }
        return MapScreen.ago(unixS)
    }

    /// The radio's path to `key`: `nil` when it floods, empty when it hears it directly.
    func nodePath(_ key: Data) -> [PathHop]? {
        _ = meshToolsVersion
        return (try? core.nodePath(node: key)) ?? nil
    }

    func trace(_ key: Data) -> TraceInfo? {
        _ = meshToolsVersion
        return core.trace(node: key)
    }

    func server(_ key: Data) -> ServerInfo? {
        _ = meshToolsVersion
        return try? core.server(node: key)
    }

    /// Runs a mesh request; an error message to show, or nil.
    private func meshCall(_ f: () throws -> Void) -> String? {
        do {
            try f()
            meshToolsVersion += 1
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    func startTrace(_ key: Data) -> String? {
        meshCall { try core.startTrace(node: key, nowMs: now()) }
    }

    func serverLogin(_ key: Data, password: String, remember: Bool) -> String? {
        meshCall { try core.serverLogin(node: key, password: password, remember: remember, nowMs: now()) }
    }

    func serverLogout(_ key: Data) {
        _ = meshCall { try core.serverLogout(node: key) }
    }

    func requestServerStats(_ key: Data) -> String? {
        meshCall { try core.requestServerStats(node: key, nowMs: now()) }
    }

    func requestNeighbours(_ key: Data) -> String? {
        meshCall { try core.requestNeighbours(node: key, nowMs: now()) }
    }

    func sendServerCommand(_ key: Data, _ command: String) -> String? {
        let sentAt = UInt32(Date().timeIntervalSince1970)
        return meshCall { try core.sendServerCommand(node: key, command: command, sentAt: sentAt, nowMs: now()) }
    }

    // MARK: saved radios (ADR-0042)

    /// The radio answered: save it with how to reach it again (Bluetooth, or TCP).
    func radioConnected() {
        if !isDemo {
            if let t = tcp.target {
                let host = t.host.contains(":") ? "[\(t.host)]" : t.host
                try? core.rememberRadio(address: "\(host):\(t.port)", link: .tcp, name: nodeName)
            } else if let id = companion.currentId {
                try? core.rememberRadio(address: id.uuidString, link: .bluetooth, name: nodeName)
            }
        }
        savedRadios = core.savedRadios()
    }

    func refreshSavedRadios() { savedRadios = core.savedRadios() }

    /// Whether `radio` can be reached from this phone (a USB radio is Android's).
    func canSwitch(to radio: SavedRadio) -> Bool {
        !radio.current && radio.link != .usb
    }

    /// Leaves the radio connected now for `radio`.
    func switchRadio(_ radio: SavedRadio) {
        switch radio.link {
        case .bluetooth:
            guard let id = UUID(uuidString: radio.address) else { return }
            disconnectRadio()
            companion.connect(id)
        case .tcp:
            disconnectRadio()
            connectTcp(radio.address)
        case .usb:
            break
        }
    }

    func forgetRadio(_ radio: SavedRadio) {
        if radio.current { disconnectRadio() }
        try? core.forgetRadio(address: radio.address)
        savedRadios = core.savedRadios()
    }

    /// The radio's battery is low enough to warn (≤ 15 %).
    var radioBatteryLow: Bool { isConnected && radioSettings?.batteryLow == true }

    // MARK: channels by QR code (protocol.md §15.3)

    func channelQr(_ id: Data) -> String? { try? core.channelQr(id: id) }

    func parseChannelQr(_ text: String) -> ChannelCode? { try? core.parseChannelQr(text: text) }
}
