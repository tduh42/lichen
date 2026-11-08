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
    /// New identities presented by verified contacts, waiting for the user (node → identity).
    var conflicts: [Data: Data] = [:]
    /// The recovery phrase was written down (Identity screen).
    var phraseBackedUp = UserDefaults.standard.bool(forKey: "lichen.phraseBackedUp") {
        didSet { UserDefaults.standard.set(phraseBackedUp, forKey: "lichen.phraseBackedUp") }
    }
    // Internet bridge (ADR-0015).
    /// iroh reachable (a relay answered).
    var bridgeOnline = false
    /// iroh on (direct peer to peer, or through relays only).
    var irohEnabled = UserDefaults.standard.object(forKey: "lichen.iroh") == nil || UserDefaults.standard.bool(forKey: "lichen.iroh") {
        didSet { UserDefaults.standard.set(irohEnabled, forKey: "lichen.iroh"); applyIroh() }
    }
    /// iroh through relays only: contacts never see our IP address.
    var relayOnly = UserDefaults.standard.bool(forKey: "lichen.relayOnly") {
        didSet { UserDefaults.standard.set(relayOnly, forKey: "lichen.relayOnly"); applyIroh(restart: true) }
    }
    /// Tor on: our onion service offers what we send, and downloads can go over Tor.
    var torEnabled = UserDefaults.standard.bool(forKey: "lichen.tor") {
        didSet { UserDefaults.standard.set(torEnabled, forKey: "lichen.tor"); applyTor() }
    }
    var torStatus = TorStatus.off
    /// Our onion service identity, once published.
    var onionId: Data?
    /// Content fetched or offered over the bridge, by BLAKE3 hash.
    var downloads: [Data: URL] = [:]
    /// Downloads in progress: the transport each one uses (1 iroh, 2 Tor).
    var fetching: [Data: UInt8] = [:]
    var fetchErrors: [Data: String] = [:]

    enum TorStatus: Equatable { case off, connecting, publishing, ready, failed }

    /// The name we give on our contact card.
    var myName = UserDefaults.standard.string(forKey: "lichen.myName") ?? "" {
        didSet { UserDefaults.standard.set(myName, forKey: "lichen.myName") }
    }
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
    /// SOS incidents heard, by sender node: one alert each (`Search.swift`).
    var incidents: [Data: SosIncident] = [:]
    /// The incident whose full-screen alert is up, until dismissed.
    var sosAlert: SenderID?
    /// The person the Search tab shows.
    var searchTarget: Data?
    /// Searches we run (protocol.md §11); `searchVersion` changes with their estimates.
    var searches: [SearchState] = []
    var searchVersion = 0
    /// Members searching for us, and our radio's power while a search holds it lowered.
    var searchedBy: [SearchedBy] = []
    var loweredPower: Int8?
    /// What our SOS is doing.
    var sosStatus: SosStatus?
    /// The compass heading, degrees from true north, while the Search tab is on screen.
    var heading: Double?
    @ObservationIgnored var lastBatteryReport: Date?
    /// Conversations with older messages to load, and those whose history failed to load.
    var olderAvailable: [ChatID: Bool] = [:]
    var historyError: [ChatID: String] = [:]
    /// A screen to show at launch (debug launch argument `-screen <name>`, for screenshots).
    var debugScreen: String?
    /// Image previews whose full variant we asked for.
    var requestedFull: Set<Int64> = []
    let recorder = VoiceRecorder()
    @ObservationIgnored let notifier = Notifier()
    /// Codec2 mode of new voice messages (§13.1): 0 = 700C (compact), 2 = 1600 (standard),
    /// 3 = 2400 (clear).
    var voiceMode = UserDefaults.standard.object(forKey: "lichen.voiceMode") as? Int ?? 2 {
        didSet { UserDefaults.standard.set(voiceMode, forKey: "lichen.voiceMode") }
    }
    let player = VoicePlayer()
    /// Screens that need our position while shown (map, search, SOS alert).
    var locationUsers: Set<String> = [] {
        didSet { if locationUsers != oldValue { updateLocator() } }
    }

    @ObservationIgnored private(set) var core: LichenCore
    /// Why our identity cannot be used: the app shows `IdentityProblemView` instead, with a
    /// throwaway core that stores nothing and never connects.
    var identityProblem: IdentityProblem?
    @ObservationIgnored private var identitySeed: Data?

    enum IdentityProblem: Error, Equatable {
        /// The Keychain cannot be read now: nothing was changed, try again.
        case unavailable
        /// What is stored is not an identity.
        case corrupt
        /// A first identity could not be saved.
        case notSaved
    }
    @ObservationIgnored let companion = Companion()
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var demo: DemoNetwork?
    @ObservationIgnored private var demoTimer: Timer?
    @ObservationIgnored private var debugSay: String?
    @ObservationIgnored private var debugVoiceSent = false
    @ObservationIgnored private var debugFilled = false
    @ObservationIgnored private let started = ProcessInfo.processInfo.systemUptime
    @ObservationIgnored let locator = Locator()
    @ObservationIgnored private var lastFed: UInt64?
    @ObservationIgnored private var demoFixTimer: Timer?
    @ObservationIgnored private var bridge: LichenBridge?

    private static let nodeKeyDefault = "lichen.nodeKey"

    init() {
        switch Self.loadIdentity() {
        case let .success(seed):
            identitySeed = seed
            core = Self.makeCore(seed: seed)
        case let .failure(problem):
            identityProblem = problem
            core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        }
        contacts = core.contacts().filter(\.person)
        groups = core.groups()
        conversations = core.conversations()
        companion.onState = { [weak self] s in MainActor.assumeIsolated { self?.companionChanged(s) } }
        companion.onFound = { [weak self] f in MainActor.assumeIsolated { self?.found = f } }
        companion.onFrame = { [weak self] d in MainActor.assumeIsolated { self?.frame(d) } }
        locator.onFix = { [weak self] l in self?.gotFix(LocationFix(l)) }
        // A tap on a notification opens its conversation.
        notifier.onOpen = { [weak self] chat in
            guard let self else { return }
            // An SOS notification opens its search.
            if case let .contact(key) = chat, let i = incidents[key], i.state() != .ended {
                searchTarget = key
                tab = .search
                return
            }
            tab = .messages
            debugOpen = chat
        }
        locator.onAuthorization = { [weak self] s in self?.locationDenied = s == .denied || s == .restricted }
        locationDenied = [.denied, .restricted].contains(locator.authorization)
        shares = core.shares()
        positions = core.positions(nowMs: now())
        updateLocator()
        loadDownloads()
        startBridge()
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
        // -identityProblem unavailable|corrupt|notSaved: the identity failure screen.
        switch args.string(forKey: "identityProblem") {
        case "unavailable": identityProblem = .unavailable
        case "corrupt": identityProblem = .corrupt
        case "notSaved": identityProblem = .notSaved
        default: break
        }

        #endif
    }

    /// The stored identity, or a new one on a first launch; never a silent replacement of an
    /// identity that cannot be read.
    private static func loadIdentity() -> Result<Data, IdentityProblem> {
        switch Keychain.readIdentity() {
        case let .found(seed):
            return .success(seed)
        case .absent:
            guard let seed = try? Keychain.createIdentitySeed() else { return .failure(.notSaved) }
            return .success(seed)
        case .unavailable:
            return .failure(.unavailable)
        case .corrupt:
            return .failure(.corrupt)
        }
    }

    /// Reads the identity again (`IdentityProblemView`); the app starts with it once it is back.
    func retryIdentity() {
        switch Self.loadIdentity() {
        case let .success(seed): useIdentity(seed)
        case let .failure(problem): identityProblem = problem
        }
    }

    /// A new identity, chosen by the user when the stored one is unreadable for good: contacts
    /// will have to verify it again.
    func startNewIdentity() -> String? {
        do {
            let seed = try Keychain.secureRandom(32)
            try Keychain.replaceIdentitySeed(seed)
            phraseBackedUp = false
            useIdentity(seed)
            return nil
        } catch {
            return String(localized: "The identity could not be saved on this phone. Try again.")
        }
    }

    /// The core restarts with `seed`, our identity from now on.
    private func useIdentity(_ seed: Data) {
        if isDemo {
            leaveDemo()
            companionState = companion.state
        }
        resetState()
        identitySeed = seed
        identityProblem = nil
        core = Self.makeCore(seed: seed)
        contacts = core.contacts().filter(\.person)
        groups = core.groups()
        conversations = core.conversations()
        shares = core.shares()
        positions = core.positions(nowMs: now())
        if isConnected { core.connect(nowMs: now()) }
        restartBridge()
        updateLocator()
        pump()
    }

    private static func makeCore(seed: Data, persistent: Bool = true) -> LichenCore {
        // The seed is 32 bytes and the identity 64 or absent: construction cannot fail.
        try! LichenCore(
            appName: "lichen-ios",
            rngSeed: Keychain.randomSeed(),
            identitySeed: seed,
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
        core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        let net = DemoNetwork()
        demo = net
        isDemo = true
        // Camille offers her photo over Tor too when Tor is on (her own client and onion service).
        if torEnabled, let state = Self.folder("demo-tor-state")?.path, let cache = Self.folder("demo-tor-cache")?.path {
            net.camilleOverTor(stateDir: state, cacheDir: cache)
        }
        #if DEBUG
        // -sosdemo YES: Julien in SOS without GPS once the group is set up (a real search);
        // -sosdemo2 YES: Camille in SOS too, for five minutes (two incidents).
        let args = UserDefaults.standard
        if args.bool(forKey: "sosdemo") || args.bool(forKey: "sosdemo2") { net.julienSos() }
        if args.bool(forKey: "sosdemo2") { net.camilleSos(afterS: 240, forS: 300) }
        #endif
        companionState = .connected("ThinkNode M5")
        core.connect(nowMs: now())
        demoTimer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.demoStep() }
        }
        updateLocator()
        pump()
    }

    func stopDemo() {
        leaveDemo()
        resetState()
        switch Self.loadIdentity() {
        case let .success(seed):
            identitySeed = seed
            core = Self.makeCore(seed: seed)
        case let .failure(problem):
            identityProblem = problem
            core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        }
        companionState = companion.state
        shares = core.shares()
        updateLocator()
    }

    private func leaveDemo() {
        demoTimer?.invalidate()
        demoFixTimer?.invalidate()
        demoFixTimer = nil
        demo = nil
        isDemo = false
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
        conflicts = [:]
        contacts = []
        groups = []
        chats = [:]
        conversations = []
        secured = []
        lastSnr = [:]
        shares = []
        positions = []
        sos = false
        incidents = [:]
        sosAlert = nil
        searchTarget = nil
        searches = []
        searchedBy = []
        loweredPower = nil
        sosStatus = nil
        myFix = nil
        lastFed = nil
    }

    var isConnected: Bool {
        if case .connected = companionState { return true }
        return false
    }

    // MARK: runner

    func now() -> UInt64 {
        UInt64((ProcessInfo.processInfo.systemUptime - started) * 1000)
    }

    private func companionChanged(_ s: Companion.State) {
        if isDemo { return }
        companionState = s
        // No radio session with a throwaway identity (`identityProblem`).
        if case .connected = s, identityProblem == nil {
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
        let st = core.sosStatus(nowMs: t)
        if st != sosStatus { sosStatus = st }
        if !searches.isEmpty || sos { reportBattery() }
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
        case .identityLearned:
            contacts = core.contacts().filter(\.person)
        case let .identityConflict(node, identity):
            conflicts[node] = identity
        case .contactsUpdated:
            contacts = core.contacts().filter(\.person)
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let c = contacts.first(where: { $0.name == name }) {
                debugOpen = .contact(key: c.key)
            }
            // -fill N: N older messages with Camille (paging checks), once.
            if isDemo, !debugFilled, UserDefaults.standard.integer(forKey: "fill") > 0,
               let c = contacts.first(where: { $0.name == "Camille" }) {
                debugFilled = true
                try? core.demoFillHistory(chat: .contact(key: c.key), count: UInt32(UserDefaults.standard.integer(forKey: "fill")))
                conversations = core.conversations()
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
                    guard let self else { return }
                    _ = self.sendVoice(prepareVoice(pcm: pcm, mode: UInt8(self.voiceMode)), to: .contact(key: c.key))
                }
            }
            #endif
        case let .sessionEstablished(peer):
            secured.insert(peer)
        case let .received(from, message, snr, _):
            lastSnr[from] = snr
            notifier.message(.contact(key: from), title: name(of: from), body: message.display)
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
            contacts = core.contacts().filter(\.person)
            #if DEBUG
            if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
               let g = groups.first(where: { $0.name == name }) {
                debugOpen = .group(gid: g.gid)
            }
            #endif
        case let .groupMessage(gid, from, message, snr, _):
            lastSnr[from] = snr
            let chat = ChatID.group(gid: gid)
            notifier.message(chat, title: title(of: chat), body: "\(name(of: from)): \(message.display)")
        case let .messageChanged(m):
            upsert(m)
            conversations = core.conversations()
            #if DEBUG
            // -media YES: ask for full images as their previews arrive, fetch what is announced.
            if UserDefaults.standard.bool(forKey: "media"), !m.outgoing {
                if case .image(_, _, false, _, _, _, _) = m.message, !requestedFull.contains(m.id) {
                    _ = requestFull(m)
                }
                if case .announce = m.message { fetch(m) }
            }
            #endif
        case let .location(position, end):
            positions = core.positions(nowMs: now())
            if position.sos && !end {
                heardSos(from: position.from, group: position.group, position: position, battery: nil)
            } else {
                endSos(position.from)
            }
        case let .beacon(gid, from, sos, battery):
            // An SOS without GPS shows here (protocol.md §11.1).
            if sos {
                heardSos(from: from, group: gid, position: nil, battery: battery)
            } else {
                endSos(from)
            }
        case .sharesChanged:
            shares = core.shares()
            updateLocator()
        case .mediaRequested:
            break
        case let .searched(gid, by):
            // A member asks where we are and we answer (SOS or this group's consent, §11).
            let chat = ChatID.group(gid: gid)
            notifier.message(chat, title: title(of: chat),
                             body: String(localized: "\(name(of: by)) is searching for you"))
            searchedBy = core.searchedBy(nowMs: now())
        case .searchUpdated:
            searchVersion += 1
            refreshSearches()
        case let .txPower(dbm, previous):
            // lc-ffi keeps the power to restore; we show it while it is lowered.
            loweredPower = previous == nil ? nil : dbm
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

    /// Messages per history page.
    static let historyPage: UInt32 = 100

    /// Loads the latest page of `chat` (when its screen opens).
    func load(_ chat: ChatID) {
        do {
            let page = try core.history(chat: chat, beforeId: nil, limit: Self.historyPage)
            chats[chat] = page
            olderAvailable[chat] = page.count == Int(Self.historyPage)
            historyError[chat] = nil
        } catch {
            if chats[chat] == nil { chats[chat] = [] }
            historyError[chat] = String(localized: "The history could not be read.")
        }
    }

    /// The page before the oldest message shown.
    func loadOlder(_ chat: ChatID) {
        guard let first = chats[chat]?.first?.id else { return }
        do {
            let page = try core.history(chat: chat, beforeId: first, limit: Self.historyPage)
            chats[chat] = page + (chats[chat] ?? [])
            olderAvailable[chat] = page.count == Int(Self.historyPage)
            historyError[chat] = nil
        } catch {
            historyError[chat] = String(localized: "The history could not be read.")
        }
    }

    /// The text typed in `chat` and not sent, kept across screens and restarts.
    func draft(_ chat: ChatID) -> String {
        (UserDefaults.standard.dictionary(forKey: Self.draftsDefault) as? [String: String])?[chat.storageKey] ?? ""
    }

    func setDraft(_ chat: ChatID, _ text: String) {
        var all = (UserDefaults.standard.dictionary(forKey: Self.draftsDefault) as? [String: String]) ?? [:]
        all[chat.storageKey] = text.isEmpty ? nil : text
        UserDefaults.standard.set(all, forKey: Self.draftsDefault)
    }

    private static let draftsDefault = "lichen.drafts"

    func markRead(_ chat: ChatID) {
        try? core.markRead(chat: chat)
        conversations = core.conversations()
    }

    /// Deletes a conversation's messages; the contact or the group stays.
    func deleteConversation(_ chat: ChatID) {
        try? core.deleteConversation(chat: chat)
        chats[chat] = nil
        conversations = core.conversations()
    }

    /// A MeshCore advert (flood), so that radios around add ours as a contact.
    func sendAdvert() {
        core.sendAdvert(flood: true, nowMs: now())
        pump()
    }

    /// What sending `text` to `chat` costs us; for a group, our own floods (both copies).
    func estimate(_ chat: ChatID, text: String) -> Estimate? {
        guard !text.isEmpty else { return nil }
        switch chat {
        case let .contact(key): return try? core.estimateText(to: key, textLen: UInt32(text.utf8.count))
        case .group: return core.estimateGroupText(textLen: UInt32(text.utf8.count))
        }
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
    func updateLocator() {
        // A search measures with our position: it must stay fresh, screen off too.
        let sharing = !shares.isEmpty || sos || !searches.isEmpty
        let needed = sharing || !locationUsers.isEmpty
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

    // MARK: Internet bridge (ADR-0015)

    private static func folder(_ name: String) -> URL? {
        guard let base = dataDirectory() else { return nil }
        let dir = URL(fileURLWithPath: base).appendingPathComponent(name, isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// This phone's bridge, keyed from the identity: iroh and Tor as the settings say.
    func startBridge() {
        guard let identitySeed else {
            bridge = nil
            return
        }
        bridge = try? LichenBridge(identitySeed: identitySeed, dir: Self.folder("offers")?.path)
        applyIroh()
        applyTor()
    }

    /// A new identity, new bridge keys.
    private func restartBridge() {
        let old = bridge
        bridge = nil
        bridgeOnline = false
        torStatus = .off
        onionId = nil
        Task {
            await old?.stopIroh()
            await old?.stopTor()
            startBridge()
        }
    }

    /// Starts or stops iroh as the settings say; `bridgeOnline` once a relay answers.
    private func applyIroh(restart: Bool = false) {
        guard let bridge else { return }
        let (on, relayOnly) = (irohEnabled, relayOnly)
        Task {
            if !on || restart {
                await bridge.stopIroh()
                bridgeOnline = false
            }
            guard on else { return }
            if bridge.endpointId() == nil { _ = try? await bridge.startIroh(relayOnly: relayOnly) }
            bridgeOnline = await bridge.online(limitS: 30)
        }
    }

    private static var torDirs: (String, String) {
        (folder("tor-state")?.path ?? "", folder("tor-cache")?.path ?? "")
    }

    /// Connects to Tor and publishes our onion service, or stops Tor, as the settings say.
    private func applyTor() {
        guard let bridge else { return }
        guard torEnabled else {
            if torStatus != .off {
                torStatus = .off
                onionId = nil
                Task { await bridge.stopTor() }
            }
            return
        }
        guard torStatus == .off || torStatus == .failed else { return }
        torStatus = .connecting
        if isDemo, let demo, let state = Self.folder("demo-tor-state")?.path, let cache = Self.folder("demo-tor-cache")?.path {
            demo.camilleOverTor(stateDir: state, cacheDir: cache)
        }
        let (state, cache) = Self.torDirs
        Task {
            do {
                let id = try await bridge.startTor(stateDir: state, cacheDir: cache, publish: true, limitS: 180)
                guard torEnabled else { return }
                onionId = id
                torStatus = .publishing
                let reachable = await bridge.onionReachable(limitS: 300)
                guard torEnabled else { return }
                torStatus = reachable ? .ready : .failed
            } catch {
                if torEnabled { torStatus = .failed }
            }
        }
    }

    private func loadDownloads() {
        guard let dir = Self.folder("downloads"),
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        else { return }
        for f in files {
            if let h = Data(hex: f.deletingPathExtension().lastPathComponent), h.count == 32 { downloads[h] = f }
        }
    }

    private func keep(_ bytes: Data, hash: Data, name: String) -> URL? {
        let ext = (name as NSString).pathExtension
        guard let url = Self.folder("downloads")?.appendingPathComponent(hash.hex)
            .appendingPathExtension(ext.isEmpty ? "bin" : ext)
        else { return nil }
        try? bytes.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        downloads[hash] = url
        return url
    }

    /// The providers we can put in an announcement: iroh unless `anonymous`, Tor when our
    /// onion service is reachable.
    func bridgeProviders(anonymous: Bool) -> [Provider] {
        var out: [Provider] = []
        if !anonymous, bridgeOnline, let id = bridge?.endpointId() { out.append(Provider(transport: 1, key: id)) }
        if torStatus == .ready, let onionId { out.append(Provider(transport: 2, key: onionId)) }
        return out
    }

    /// Offers a photo on the bridge and announces it in one LoRa frame (§14.1); `anonymous`:
    /// over Tor only, so the recipient never learns our IP address.
    func sendOverBridge(_ original: Data, name: String, to chat: ChatID, anonymous: Bool) -> String? {
        let providers = bridgeProviders(anonymous: anonymous)
        guard let bridge, !providers.isEmpty else {
            return String(localized: "No Internet connection: the photo cannot be offered.")
        }
        do {
            let offered = try bridge.offer(bytes: original)
            _ = keep(original, hash: offered.hash, name: name)
            let m = try core.sendAnnounce(
                chat: chat, mediaId: Data(), kind: 1, hash: offered.hash, size: offered.size,
                providers: providers, name: name,
                sentAt: UInt32(Date().timeIntervalSince1970), nowMs: now()
            )
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

    /// The providers of an announcement we would try, in order: iroh (when on), then Tor.
    func fetchRoute(_ providers: [Provider]) -> [Provider] {
        let iroh = irohEnabled && bridgeOnline ? providers.first { $0.transport == 1 } : nil
        let tor = providers.first { $0.transport == 2 }
        return [iroh, tor].compactMap { $0 }
    }

    /// Fetches announced content, verified against its hash: over iroh first, over Tor when
    /// iroh cannot reach the sender (Tor connects on demand).
    func fetch(_ m: StoredMessage) {
        guard case let .announce(_, _, _, hash, size, providers, name) = m.message,
              let bridge, fetching[hash] == nil
        else { return }
        let route = fetchRoute(providers)
        guard let first = route.first else {
            fetchErrors[hash] = String(localized: "No transport reaches the sender: turn iroh or Tor on.")
            return
        }
        fetching[hash] = first.transport
        fetchErrors[hash] = nil
        Task {
            defer { fetching[hash] = nil }
            var failure = String(localized: "Download failed.")
            for p in route {
                fetching[hash] = p.transport
                do {
                    if p.transport == 2 {
                        let (state, cache) = Self.torDirs
                        _ = try await bridge.startTor(stateDir: state, cacheDir: cache, publish: false, limitS: 180)
                    }
                    let bytes = try await bridge.fetch(transport: p.transport, provider: p.key, hash: hash, size: UInt64(size))
                    _ = keep(bytes, hash: hash, name: name)
                    return
                } catch let e as BridgeError {
                    failure = e.display
                    // Only an unreachable sender is worth another transport.
                    if case .Unreachable = e { continue }
                    if case .Timeout = e { continue }
                    break
                } catch {
                    break
                }
            }
            fetchErrors[hash] = failure
        }
    }

    /// The announcement that completes the LoRa preview `mediaId` in `chat`, if any.
    func announcement(for mediaId: Data, in chat: ChatID) -> StoredMessage? {
        chats[chat]?.last { m in
            if case let .announce(_, id, _, _, _, _, _) = m.message { return id == mediaId }
            return false
        }
    }

    /// The LoRa preview an announcement completes, if it is in `chat`.
    func preview(for mediaId: Data, in chat: ChatID) -> StoredMessage? {
        chats[chat]?.first { m in
            if case let .image(_, id, _, _, _, _, _) = m.message { return id == mediaId }
            return false
        }
    }

    // MARK: identity (ADR-0013)

    var fingerprint: [String] { core.fingerprint() }

    func recoveryPhrase() -> [String] { core.recoveryPhrase() }

    /// Our contact card for the QR code; nil until the radio is connected.
    func myCard() -> String? {
        let name = myName.isEmpty ? nodeName : myName
        return try? core.contactCard(name: name)
    }

    /// Adds the contact of a scanned card (verified: scanned in person).
    func addCard(_ text: String) -> Result<ContactInfo, LichenError> {
        do {
            let c = try core.addContactCard(card: text)
            contacts = core.contacts().filter(\.person)
            pump()
            return .success(c)
        } catch let e as LichenError {
            return .failure(e)
        } catch {
            return .failure(.BadCode)
        }
    }

    func contact(_ node: Data) -> ContactInfo? { contacts.first { $0.key == node } }

    /// Names a contact on this phone only (never sent); nil or blank: its node's name again.
    func renameContact(_ node: Data, to name: String?) {
        let trimmed = name?.trimmingCharacters(in: .whitespacesAndNewlines)
        try? core.renameContact(node: node, name: (trimmed?.isEmpty ?? true) ? nil : trimmed)
        contacts = core.contacts().filter(\.person)
        groups = core.groups()
        pump()
    }

    func safetyNumber(with node: Data) -> [String]? { try? core.safetyNumber(node: node) }

    /// The safety number with a new identity a contact presented (before accepting it).
    func safetyNumber(withIdentity identity: Data) -> [String]? { try? core.safetyNumberWith(identity: identity) }

    /// Verifies (or not) contact `node`, whose identity `identity` the user compared; false
    /// when its identity changed meanwhile (nothing verified).
    @discardableResult
    func setVerified(_ node: Data, identity: Data, _ verified: Bool) -> Bool {
        let done = (try? core.setVerified(node: node, identity: identity, verified: verified)) ?? false
        contacts = core.contacts().filter(\.person)
        return done
    }

    /// Accepts the new identity `expected` the user compared, verified; false when another
    /// one arrived meanwhile (compare again).
    func acceptIdentity(_ node: Data, expected: Data) -> Bool {
        let done = (try? core.acceptIdentity(node: node, expected: expected, verify: true)) ?? false
        if done { conflicts[node] = nil } else { conflicts[node] = (try? core.pendingIdentity(node: node)) ?? nil }
        contacts = core.contacts().filter(\.person)
        pump()
        return done
    }

    func rejectIdentity(_ node: Data, expected: Data) {
        try? core.rejectIdentity(node: node, expected: expected)
        conflicts[node] = (try? core.pendingIdentity(node: node)) ?? nil
    }

    /// Restores an identity from its 24 words: the core restarts with it.
    func restore(words: [String]) -> String? {
        do {
            let seed = try seedFromPhrase(words: words)
            // Saved and read back before the app switches to it.
            try Keychain.replaceIdentitySeed(seed)
            phraseBackedUp = true
            useIdentity(seed)
            return nil
        } catch is IdentityWriteError {
            return String(localized: "The identity could not be saved on this phone. Try again.")
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "This is not a lichen code.")
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

    var totalUnread: Int { conversations.reduce(0) { $0 + Int($1.unread) } }
}

extension Data {
    var hex: String { map { String(format: "%02x", $0) }.joined() }

    init?(hex: String) {
        guard hex.count.isMultiple(of: 2) else { return nil }
        var bytes = [UInt8]()
        var i = hex.startIndex
        while i < hex.endIndex {
            let j = hex.index(i, offsetBy: 2)
            guard let b = UInt8(hex[i..<j], radix: 16) else { return nil }
            bytes.append(b)
            i = j
        }
        self.init(bytes)
    }
}

extension BridgeError {
    var display: String {
        switch self {
        case .Unreachable: return String(localized: "Their phone is not reachable: offline, or the app is closed.")
        case .NotFound: return String(localized: "No longer offered by the sender.")
        case .TooLarge: return String(localized: "Larger than announced: dropped.")
        case .Mismatch: return String(localized: "The content did not match its fingerprint: dropped.")
        case .Timeout: return String(localized: "The transfer took too long.")
        case let .Failed(reason): return reason
        }
    }
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
        case let .announce(_, _, _, _, _, _, name): return String(localized: "Photo: \(name)")
        case let .other(type, _) where type == 0x03: return String(localized: "Position")
        case let .other(type, body): return String(localized: "Unsupported content (type \(String(type)), \(String(body.count)) bytes)")
        }
    }

    var date: Date {
        switch self {
        case let .text(sentAt, _), let .voice(sentAt, _, _, _, _), let .image(sentAt, _, _, _, _, _, _),
             let .announce(sentAt, _, _, _, _, _, _):
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
        case .identityChanged: return String(localized: "their identity changed: verify it first")
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
        case .NoIdentity: return String(localized: "Connect your radio first.")
        case .IdentityUnknown: return String(localized: "Their identity is not known yet: exchange a message or scan their code first.")
        case .BadCode: return String(localized: "This is not a lichen code.")
        case .NotAllowed: return String(localized: "Only the group admin can do this.")
        case .InvalidKey: return String(localized: "Invalid key.")
        case let .OverBudget(waitS):
            let minutes = max(1, Int((Double(waitS) / 60).rounded(.up)))
            return String(localized: "Not enough airtime left this hour: it fits in \(minutes) min.")
        case .MediaInGroup:
            return String(localized: "Photos and voice messages go to contacts only: in a group they would have to be flooded.")
        case .BadImage: return String(localized: "This image could not be read.")
        case .NotAPreview: return String(localized: "There is no full version to ask for.")
        case .NotSearching: return String(localized: "No search is running for this person.")
        case let .TooSoon(waitS): return String(localized: "Possible again in \(waitS) s.")
        case .NoPosition: return String(localized: "Your position is not known yet.")
        case .Storage: return String(localized: "The phone's storage could not be read or written.")
        }
    }
}
