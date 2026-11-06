import CoreLocation
import Foundation
import Observation

/// A conversation: `ChatRef` comes from the core (contact or group).
typealias ChatID = ChatRef

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
    /// Loaded histories, by conversation (filled when a chat is opened).
    var chats: [ChatID: [StoredMessage]] = [:]
    /// Conversations with messages, most recent first, with their unread counts.
    var conversations: [Conversation] = []
    var secured: Set<Data> = []
    var lastSnr: [Data: Float] = [:]
    /// Running on the simulated network instead of a real node.
    var isDemo = false
    /// A chat to open (debug launch argument `-open <name>`, for screenshots).
    var debugOpen: ChatID?
    var tab = Tab.messages

    enum Tab: Hashable { case messages, map, search, settings }

    // Location (protocol.md §12).
    var shares: [ShareInfo] = []
    var positions: [PeerPosition] = []
    var sos = false
    var myFix: LocationFix?
    var locationDenied = false
    /// The latest SOS heard, until it ends (the Search tab follows it).
    var incomingSos: PeerPosition?
    /// The full-screen alert for an SOS just heard, until dismissed.
    var sosAlert: PeerPosition?
    /// Every SOS position heard, newest last (the Search tab's measurements).
    var sosLog: [PeerPosition] = []
    /// A screen to show at launch (debug launch argument `-screen <name>`, for screenshots).
    var debugScreen: String?
    /// Image previews whose full variant we asked for.
    var requestedFull: Set<Int64> = []
    let recorder = VoiceRecorder()
    let player = VoicePlayer()
    /// The map is on screen: our position is needed even without a share.
    var mapVisible = false {
        didSet { if mapVisible != oldValue { updateLocator() } }
    }

    @ObservationIgnored private(set) var core: LichenCore
    @ObservationIgnored let companion = Companion()
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var demo: DemoNetwork?
    @ObservationIgnored private var demoTimer: Timer?
    @ObservationIgnored private var debugSay: String?
    @ObservationIgnored private var debugVoiceSent = false
    @ObservationIgnored private let started = ProcessInfo.processInfo.systemUptime
    @ObservationIgnored let locator = Locator()
    @ObservationIgnored private var lastFed: UInt64?
    @ObservationIgnored private var demoFixTimer: Timer?

    private static let nodeKeyDefault = "lichen.nodeKey"

    init() {
        let lastNode = UserDefaults.standard.data(forKey: Self.nodeKeyDefault)
        core = Self.makeCore(identity: lastNode.flatMap { Keychain.load(node: $0.hex) })
        contacts = core.contacts()
        groups = core.groups()
        conversations = core.conversations()
        companion.onState = { [weak self] s in MainActor.assumeIsolated { self?.companionChanged(s) } }
        companion.onFound = { [weak self] f in MainActor.assumeIsolated { self?.found = f } }
        companion.onFrame = { [weak self] d in MainActor.assumeIsolated { self?.frame(d) } }
        locator.onFix = { [weak self] l in self?.gotFix(LocationFix(l)) }
        locator.onAuthorization = { [weak self] s in self?.locationDenied = s == .denied || s == .restricted }
        locationDenied = [.denied, .restricted].contains(locator.authorization)
        shares = core.shares()
        positions = core.positions(nowMs: now())
        updateLocator()
        #if DEBUG
        let args = UserDefaults.standard
        debugSay = args.string(forKey: "say")
        if args.bool(forKey: "demo") { startDemo() }
        switch args.string(forKey: "tab") {
        case "map": tab = .map
        case "search": tab = .search
        case "settings": tab = .settings
        default: break
        }
        debugScreen = args.string(forKey: "screen")

        #endif
    }

    private static func makeCore(identity: Data?, persistent: Bool = true) -> LichenCore {
        // The seed is 32 bytes and the identity 64 or absent: construction cannot fail.
        try! LichenCore(
            appName: "lichen-ios",
            rngSeed: Keychain.randomSeed(),
            identity: identity,
            dataDir: persistent ? dataDirectory() : nil
        )
    }

    /// Per-node databases live in Application Support (file protection until first unlock).
    private static func dataDirectory() -> String? {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        else { return nil }
        let dir = base.appendingPathComponent("lichen", isDirectory: true)
        try? FileManager.default.createDirectory(
            at: dir, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
        )
        return dir.path
    }

    // MARK: demo network

    /// Runs the app on a simulated MeshCore network (two simulated users, a group), in real time.
    func startDemo() {
        resetState()
        core = Self.makeCore(identity: nil, persistent: false)
        let net = DemoNetwork()
        demo = net
        isDemo = true
        companionState = .connected("ThinkNode M5")
        core.connect(nowMs: now())
        demoTimer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.demoStep() }
        }
        updateLocator()
        pump()
    }

    func stopDemo() {
        demoTimer?.invalidate()
        demoFixTimer?.invalidate()
        demoFixTimer = nil
        demo = nil
        isDemo = false
        resetState()
        core = Self.makeCore(identity: nil)
        companionState = companion.state
        shares = core.shares()
        updateLocator()
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
        conversations = []
        secured = []
        lastSnr = [:]
        shares = []
        positions = []
        sos = false
        incomingSos = nil
        sosAlert = nil
        sosLog = []
        myFix = nil
        lastFed = nil
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
        core.setClock(unixMs: UInt64(Date().timeIntervalSince1970 * 1000), nowMs: t)
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
                debugOpen = .contact(key: c.key)
            }
            // -sosdemo YES: an SOS from Julien, for screenshots of the alert and the Search tab.
            if UserDefaults.standard.bool(forKey: "sosdemo"), incomingSos == nil,
               let j = contacts.first(where: { $0.name == "Julien" }) {
                let p = PeerPosition(
                    from: j.key, group: nil,
                    lat: 44.9065, lon: 5.4745, altM: 1_620, radiusM: 6, headingDeg: nil, speedMps: nil,
                    atS: UInt64(Date().timeIntervalSince1970) - 40, sos: true, extrapolated: false
                )
                incomingSos = p
                sosAlert = debugScreen == "search" ? nil : p
                sosLog = [p]
            }
            if shares.isEmpty, let name = UserDefaults.standard.string(forKey: "share"),
               let c = contacts.first(where: { $0.name == name }) {
                _ = setShare(.contact(key: c.key), precision: .m100, hours: 1.25)
            }
            // -media YES: a 4 s voice message to Camille (she answers with hers).
            if UserDefaults.standard.bool(forKey: "media"), !debugVoiceSent,
               let c = contacts.first(where: { $0.name == "Camille" }) {
                debugVoiceSent = true
                let pcm = (0..<32_000).map { i -> Int16 in
                    let t = Double(i) / 8_000
                    return Int16(6_000 * sin(2 * .pi * (150 + 30 * sin(t * 3)) * t) * (0.6 + 0.4 * sin(t * 9)))
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + 20) { [weak self] in
                    _ = self?.sendVoice(prepareVoice(pcm: pcm), to: .contact(key: c.key))
                }
            }
            #endif
        case let .sessionEstablished(peer):
            secured.insert(peer)
        case let .received(from, _, snr, _):
            lastSnr[from] = snr
            #if DEBUG
            if let say = debugSay {
                debugSay = nil
                _ = send(say, to: .contact(key: from))
            }
            #endif
        case .progress, .delivered, .failed:
            break
        case .groupJoined, .groupUpdated, .groupLeft:
            groups = core.groups()
            contacts = core.contacts()
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let g = groups.first(where: { $0.name == name }) {
                debugOpen = .group(gid: g.gid)
            }
            #endif
        case let .groupMessage(_, from, _, snr, _):
            lastSnr[from] = snr
        case let .messageChanged(m):
            upsert(m)
            conversations = core.conversations()
            #if DEBUG
            // -media YES: ask for full images as their previews arrive.
            if UserDefaults.standard.bool(forKey: "media"), !m.outgoing,
               case .image(_, _, false, _, _, _, _) = m.message, !requestedFull.contains(m.id) {
                _ = requestFull(m)
            }
            #endif
        case let .location(position, end):
            positions = core.positions(nowMs: now())
            if position.sos && !end {
                if incomingSos?.from != position.from { sosAlert = position }
                incomingSos = position
                sosLog.append(position)
            } else if incomingSos?.from == position.from {
                incomingSos = nil
                sosAlert = nil
            }
        case .sharesChanged:
            shares = core.shares()
            updateLocator()
        case .mediaRequested:
            break
        }
    }

    private func upsert(_ m: StoredMessage) {
        guard var list = chats[m.chat] else { return }
        if let i = list.firstIndex(where: { $0.id == m.id }) {
            list[i] = m
        } else {
            list.append(m)
        }
        chats[m.chat] = list
    }

    func unread(_ chat: ChatID) -> Int {
        Int(conversations.first { $0.chat == chat }?.unread ?? 0)
    }

    func last(_ chat: ChatID) -> StoredMessage? {
        conversations.first { $0.chat == chat }?.last
    }

    // MARK: actions

    /// Loads the history of `chat` (when its screen opens).
    func load(_ chat: ChatID) {
        chats[chat] = (try? core.history(chat: chat, beforeId: nil, limit: 200)) ?? []
    }

    func markRead(_ chat: ChatID) {
        try? core.markRead(chat: chat)
        conversations = core.conversations()
    }

    func estimate(_ chat: ChatID, text: String) -> Estimate? {
        guard case let .contact(key) = chat, !text.isEmpty else { return nil }
        return try? core.estimateText(to: key, textLen: UInt32(text.utf8.count))
    }

    /// Sends `text`; returns an error message to show, or nil.
    func send(_ text: String, to chat: ChatID) -> String? {
        let sentAt = UInt32(Date().timeIntervalSince1970)
        do {
            let m: StoredMessage
            switch chat {
            case let .contact(key):
                m = try core.sendText(to: key, text: text, sentAt: sentAt, nowMs: now())
            case let .group(gid):
                m = try core.sendGroupText(gid: gid, text: text, sentAt: sentAt, nowMs: now())
            }
            if chats[chat] == nil { load(chat) } else { upsert(m) }
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    // MARK: media (protocol.md §13)

    func estimate(_ chat: ChatID, datagramLen: UInt32) -> Estimate? {
        guard case let .contact(key) = chat else { return nil }
        return try? core.estimateDatagram(to: key, len: datagramLen)
    }

    func sendVoice(_ voice: PreparedVoice, to chat: ChatID) -> String? {
        guard case let .contact(key) = chat else { return LichenError.MediaInGroup.display }
        return sendMedia(chat) { try core.sendVoice(to: key, voice: voice, sentAt: $0, nowMs: $1) }
    }

    func sendImage(_ image: PreparedImage, to chat: ChatID) -> String? {
        guard case let .contact(key) = chat else { return LichenError.MediaInGroup.display }
        return sendMedia(chat) { try core.sendImage(to: key, image: image, sentAt: $0, nowMs: $1) }
    }

    private func sendMedia(_ chat: ChatID, _ send: (UInt32, UInt64) throws -> StoredMessage) -> String? {
        do {
            let m = try send(UInt32(Date().timeIntervalSince1970), now())
            if chats[chat] == nil { load(chat) } else { upsert(m) }
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    func requestFull(_ message: StoredMessage) -> String? {
        do {
            try core.requestFullImage(messageId: message.id, nowMs: now())
            requestedFull.insert(message.id)
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// Sends a failed text or voice message again (a new message).
    func resend(_ m: StoredMessage) -> String? {
        switch m.message {
        case let .text(_, text):
            return send(text, to: m.chat)
        case let .voice(_, mode, frames, durationMs, data):
            return sendVoice(PreparedVoice(mode: mode, frames: frames, durationMs: durationMs, data: data), to: m.chat)
        default:
            return nil
        }
    }

    /// Airtime left this hour, milliseconds.
    var airtimeLeftMs: UInt64 { budget.limitMs - min(budget.usedMs, budget.limitMs) }

    func removeMember(_ member: Data, from gid: Data) -> String? {
        do {
            try core.removeMember(gid: gid, member: member, nowMs: now())
            groups = core.groups()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not change the group.")
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

    // MARK: location

    /// Location is needed while sharing, during SOS, or with the map on screen; in the demo it
    /// comes from the simulated network.
    private func updateLocator() {
        let sharing = !shares.isEmpty || sos
        let needed = sharing || mapVisible
        if let demo {
            locator.stop()
            if needed, demoFixTimer == nil {
                gotFix(demo.myPosition())
                demoFixTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated {
                        guard let self, let demo = self.demo else { return }
                        self.gotFix(demo.myPosition())
                    }
                }
            } else if !needed {
                demoFixTimer?.invalidate()
                demoFixTimer = nil
            }
        } else if needed {
            locator.start(background: sharing)
        } else {
            locator.stop()
        }
    }

    /// A fix from the phone: shown at once, handed to the core every 5 seconds at most.
    private func gotFix(_ fix: LocationFix) {
        myFix = fix
        let t = now()
        if let last = lastFed, t < last + 5_000 { return }
        lastFed = t
        core.updateLocation(fix: fix, nowMs: t)
        pump()
    }

    func refreshPositions() {
        positions = core.positions(nowMs: now())
    }

    func share(of chat: ChatID) -> ShareInfo? {
        shares.first { $0.chat == chat }
    }

    /// Shares our position with `chat` at `precision` for `hours`, or stops (`nil`).
    func setShare(_ chat: ChatID, precision: SharePrecision?, hours: Double) -> String? {
        do {
            if let precision {
                let until = UInt64(Date().timeIntervalSince1970 + hours * 3600)
                try core.startShare(chat: chat, precision: precision, untilS: until, nowMs: now())
            } else {
                try core.stopShare(chat: chat, nowMs: now())
            }
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not change the share.")
        }
        shares = core.shares()
        updateLocator()
        pump()
        return nil
    }

    func stopAllShares() {
        for s in shares {
            try? core.stopShare(chat: s.chat, nowMs: now())
        }
        shares = core.shares()
        updateLocator()
        pump()
    }

    func setSos(_ on: Bool) {
        core.setSos(on: on, nowMs: now())
        sos = core.sos()
        updateLocator()
        pump()
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

    var totalUnread: Int { conversations.reduce(0) { $0 + Int($1.unread) } }
}

extension Data {
    var hex: String { map { String(format: "%02x", $0) }.joined() }
}

extension StoredMessage {
    var date: Date { Date(timeIntervalSince1970: TimeInterval(atMs) / 1000) }

    /// One line for the conversation list ("Camille: …" in groups).
    @MainActor func preview(_ model: AppModel) -> String {
        let text = message.display
        if let author, !outgoing { return "\(model.name(of: author)): \(text)" }
        return text
    }
}

extension Message {
    var display: String {
        switch self {
        case let .text(_, text): return text
        case let .voice(_, _, _, durationMs, _):
            return String(localized: "Voice message (\(clock(TimeInterval(durationMs) / 1000)))")
        case .image: return String(localized: "Photo")
        case let .other(type, _) where type == 0x03: return String(localized: "Position")
        case let .other(type, body): return String(localized: "Unsupported content (type \(String(type)), \(String(body.count)) bytes)")
        }
    }

    var date: Date {
        switch self {
        case let .text(sentAt, _), let .voice(sentAt, _, _, _, _), let .image(sentAt, _, _, _, _, _, _):
            if sentAt > 0 { return Date(timeIntervalSince1970: TimeInterval(sentAt)) }
        case .other:
            break
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
        case .interrupted: return String(localized: "the app stopped before delivery")
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
        case let .OverBudget(waitS):
            let minutes = max(1, Int((Double(waitS) / 60).rounded(.up)))
            return String(localized: "Not enough airtime left this hour: it fits in \(minutes) min.")
        case .MediaInGroup:
            return String(localized: "Photos and voice messages go to contacts only: in a group they would have to be flooded.")
        case .BadImage: return String(localized: "This image could not be read.")
        case .NotAPreview: return String(localized: "There is no full version to ask for.")
        }
    }
}
