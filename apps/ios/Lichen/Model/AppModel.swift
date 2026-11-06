import Foundation
import Observation

/// A conversation: with a contact, or in a group.
enum ChatID: Hashable {
    case contact(Data)
    case group(Data)
}

struct ChatMessage: Identifiable, Equatable {
    enum Status: Equatable {
        case received
        case sending(sent: Int, total: Int)
        case delivered
        case failed(String)
        /// Group messages are flooded, never acknowledged.
        case flooded
    }

    let id = UUID()
    let outgoing: Bool
    /// Author, for group messages from others.
    let author: Data?
    let text: String
    let date: Date
    var status: Status
    /// Fragments and airtime estimated when it was sent.
    var fragments: Int = 1
    var airtimeMs: Int = 0
}

/// The app side of the lichen core: runs it (frames in and out, timer), keeps the UI state.
@MainActor @Observable
final class AppModel {
    var companionState: Companion.State = .off
    var found: [Companion.Found] = []
    var nodeName = ""
    var nodeKey: Data?
    var radio: Radio?
    var budget = Budget(usedMs: 0, limitMs: 360_000)
    var identityReady = false
    var keyExportRefused = false
    var contacts: [ContactInfo] = []
    var groups: [GroupInfo] = []
    var chats: [ChatID: [ChatMessage]] = [:]
    var unread: [ChatID: Int] = [:]
    var secured: Set<Data> = []
    var lastSnr: [Data: Float] = [:]
    /// Running on the simulated network instead of a real node.
    var isDemo = false
    /// A chat to open (debug launch argument `-open <name>`, for screenshots).
    var debugOpen: ChatID?

    @ObservationIgnored private(set) var core: LichenCore
    @ObservationIgnored let companion = Companion()
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var demo: DemoNetwork?
    @ObservationIgnored private var demoTimer: Timer?
    @ObservationIgnored private var debugSay: String?
    @ObservationIgnored private var outgoing: [UInt64: (ChatID, UUID)] = [:]
    @ObservationIgnored private let started = ProcessInfo.processInfo.systemUptime

    private static let nodeKeyDefault = "lichen.nodeKey"

    init() {
        let lastNode = UserDefaults.standard.data(forKey: Self.nodeKeyDefault)
        core = Self.makeCore(identity: lastNode.flatMap { Keychain.load(node: $0.hex) })
        companion.onState = { [weak self] s in MainActor.assumeIsolated { self?.companionChanged(s) } }
        companion.onFound = { [weak self] f in MainActor.assumeIsolated { self?.found = f } }
        companion.onFrame = { [weak self] d in MainActor.assumeIsolated { self?.frame(d) } }
        #if DEBUG
        let args = UserDefaults.standard
        debugSay = args.string(forKey: "say")
        if args.bool(forKey: "demo") { startDemo() }
        #endif
    }

    private static func makeCore(identity: Data?) -> LichenCore {
        // The seed is 32 bytes and the identity 64 or absent: construction cannot fail.
        try! LichenCore(appName: "lichen-ios", rngSeed: Keychain.randomSeed(), identity: identity)
    }

    // MARK: demo network

    /// Runs the app on a simulated MeshCore network (two simulated users, a group), in real time.
    func startDemo() {
        resetState()
        core = Self.makeCore(identity: nil)
        let net = DemoNetwork()
        demo = net
        isDemo = true
        companionState = .connected("ThinkNode M5")
        core.connect(nowMs: now())
        demoTimer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.demoStep() }
        }
        pump()
    }

    func stopDemo() {
        demoTimer?.invalidate()
        demo = nil
        isDemo = false
        resetState()
        core = Self.makeCore(identity: nil)
        companionState = companion.state
    }

    private func demoStep() {
        guard let demo else { return }
        let t = now()
        for f in demo.read(nowMs: t) {
            core.onCompanionFrame(frame: f, nowMs: t)
        }
        pump()
    }

    private func resetState() {
        nodeName = ""
        nodeKey = nil
        radio = nil
        identityReady = false
        keyExportRefused = false
        contacts = []
        groups = []
        chats = [:]
        unread = [:]
        secured = []
        lastSnr = [:]
        outgoing = [:]
    }

    var isConnected: Bool {
        if case .connected = companionState { return true }
        return false
    }

    // MARK: runner

    private func now() -> UInt64 {
        UInt64((ProcessInfo.processInfo.systemUptime - started) * 1000)
    }

    private func companionChanged(_ s: Companion.State) {
        if isDemo { return }
        companionState = s
        if case .connected = s {
            core.connect(nowMs: now())
            pump()
        }
    }

    private func frame(_ data: Data) {
        core.onCompanionFrame(frame: data, nowMs: now())
        pump()
    }

    /// Writes due frames, reads events, and sets the timer for the next wake-up.
    func pump() {
        let t = now()
        core.tick(nowMs: t)
        while let f = core.pollCompanion(nowMs: t) {
            if let demo { demo.write(frame: f, nowMs: t) } else { companion.write(f) }
        }
        while let e = core.pollEvent() {
            handle(e)
        }
        // Sender keys arrive without an event: refresh the groups when they changed.
        let g = core.groups()
        if g != groups { groups = g }
        budget = core.budget(nowMs: t)
        timer?.invalidate()
        if let next = core.nextWakeup() {
            let delay = max(0.02, Double(Int64(next) - Int64(t)) / 1000)
            timer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated { self?.pump() }
            }
        }
    }

    private func handle(_ e: LichenEvent) {
        switch e {
        case let .connected(key, name, radio):
            nodeKey = key
            nodeName = name
            self.radio = radio
            UserDefaults.standard.set(key, forKey: Self.nodeKeyDefault)
        case .identityReady:
            identityReady = true
            if !isDemo, let k = nodeKey, let prv = core.privateKey() {
                Keychain.save(prv, node: k.hex)
            }
        case .keyExportRefused:
            keyExportRefused = true
        case .contactsUpdated:
            contacts = core.contacts()
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let c = contacts.first(where: { $0.name == name }) {
                debugOpen = .contact(c.key)
            }
            #endif
        case let .sessionEstablished(peer):
            secured.insert(peer)
        case let .received(from, message, snr, _):
            lastSnr[from] = snr
            append(.contact(from), ChatMessage(outgoing: false, author: nil, text: message.display, date: message.date, status: .received))
            #if DEBUG
            if let say = debugSay {
                debugSay = nil
                _ = send(say, to: .contact(from))
            }
            #endif
        case let .progress(id, sent, total):
            update(id) { $0.status = .sending(sent: Int(sent), total: Int(total)) }
        case let .delivered(id):
            update(id) { $0.status = .delivered }
            outgoing[id] = nil
        case let .failed(id, reason):
            update(id) { $0.status = .failed(reason.display) }
            outgoing[id] = nil
        case .groupJoined, .groupUpdated, .groupLeft:
            groups = core.groups()
            contacts = core.contacts()
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let g = groups.first(where: { $0.name == name }) {
                debugOpen = .group(g.gid)
            }
            #endif
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let g = groups.first(where: { $0.name == name }) {
                debugOpen = .group(g.gid)
            }
            #endif
        case let .groupMessage(gid, from, message, snr, _):
            lastSnr[from] = snr
            append(.group(gid), ChatMessage(outgoing: false, author: from, text: message.display, date: message.date, status: .received))
        }
    }

    private func append(_ chat: ChatID, _ m: ChatMessage) {
        chats[chat, default: []].append(m)
        if !m.outgoing { unread[chat, default: 0] += 1 }
    }

    private func update(_ id: UInt64, _ change: (inout ChatMessage) -> Void) {
        guard let (chat, mid) = outgoing[id], let i = chats[chat]?.firstIndex(where: { $0.id == mid }) else { return }
        change(&chats[chat]![i])
    }

    // MARK: actions

    func markRead(_ chat: ChatID) {
        unread[chat] = nil
    }

    func estimate(_ chat: ChatID, text: String) -> Estimate? {
        guard case let .contact(key) = chat, !text.isEmpty else { return nil }
        return try? core.estimateText(to: key, textLen: UInt32(text.utf8.count))
    }

    /// Sends `text`; returns an error message to show, or nil.
    func send(_ text: String, to chat: ChatID) -> String? {
        let sentAt = UInt32(Date().timeIntervalSince1970)
        do {
            switch chat {
            case let .contact(key):
                let est = estimate(chat, text: text)
                let id = try core.sendText(to: key, text: text, sentAt: sentAt, nowMs: now())
                var m = ChatMessage(outgoing: true, author: nil, text: text, date: Date(), status: .sending(sent: 0, total: Int(est?.fragments ?? 1)))
                m.fragments = Int(est?.fragments ?? 1)
                m.airtimeMs = Int(est?.airtimeMs ?? 0)
                outgoing[id] = (chat, m.id)
                append(chat, m)
            case let .group(gid):
                _ = try core.sendGroupText(gid: gid, text: text, sentAt: sentAt, nowMs: now())
                append(chat, ChatMessage(outgoing: true, author: nil, text: text, date: Date(), status: .flooded))
            }
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    func createGroup(name: String, members: [Data]) -> String? {
        do {
            _ = try core.createGroup(name: name, members: members, nowMs: now())
            groups = core.groups()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not create the group.")
        }
    }

    // MARK: lookups for the views

    func name(of key: Data) -> String {
        if key == nodeKey { return String(localized: "You") }
        return contacts.first { $0.key == key }?.name ?? key.prefix(4).hex
    }

    func title(of chat: ChatID) -> String {
        switch chat {
        case let .contact(key): return name(of: key)
        case let .group(gid): return groups.first { $0.gid == gid }?.name ?? String(localized: "Group")
        }
    }
}

extension Data {
    var hex: String { map { String(format: "%02x", $0) }.joined() }
}

extension Message {
    var display: String {
        switch self {
        case let .text(_, text): return text
        case let .other(type, body): return String(localized: "Unsupported content (type \(String(type)), \(String(body.count)) bytes)")
        }
    }

    var date: Date {
        if case let .text(sentAt, _) = self, sentAt > 0 {
            return Date(timeIntervalSince1970: TimeInterval(sentAt))
        }
        return Date()
    }
}

extension FailReason {
    var display: String {
        switch self {
        case .noPath: return String(localized: "no path found")
        case .unreachable: return String(localized: "unreachable")
        case .timeout: return String(localized: "no acknowledgement")
        case .refused: return String(localized: "refused")
        case .tooLarge: return String(localized: "too large")
        }
    }
}

extension LichenError {
    var display: String {
        switch self {
        case .UnknownContact: return String(localized: "Unknown contact.")
        case .UnknownGroup: return String(localized: "Unknown group.")
        case .Empty: return String(localized: "The message is empty.")
        case .TooLarge: return String(localized: "The message is too long.")
        case .NoIdentity: return String(localized: "The node's key is not available yet.")
        case .NotAllowed: return String(localized: "Only the group admin can do this.")
        case .InvalidKey: return String(localized: "Invalid key.")
        }
    }
}
