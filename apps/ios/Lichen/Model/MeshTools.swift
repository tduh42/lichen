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

extension AppModel {
    func node(_ key: Data) -> NodeInfo? { nodes.first { $0.key == key } }

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
