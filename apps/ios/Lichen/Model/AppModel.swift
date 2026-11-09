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
    /// Downloads in progress, to cancel them.
    @ObservationIgnored private var fetchTasks: [Data: Task<Void, Never>] = [:]

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
    /// Pinned and muted conversations (local, per identity).
    var prefs: [ChatID: ChatPrefs] = [:]
    /// A message to open its chat on (from a search): taken by that chat's screen.
    var focus: (chat: ChatID, id: Int64)?
    var secured: Set<Data> = []
    var lastSnr: [Data: Float] = [:]
    /// Running on the simulated network instead of a real node.
    var isDemo = false
    /// The demo's simulated radio is switched off (Settings), to see messages wait (§6.1).
    var demoRadioOff = false
    /// A chat to open (debug launch argument `-open <name>`, for screenshots).
    var debugOpen: ChatID?
    /// Invitations we accepted, by admin node: the group's name, until it arrives (§10.5).
    var joining: [Data: String] = [:]
    /// Groups that arrived after we accepted their invitation, by admin node.
    var joinedByInvite: [Data: Data] = [:]
    /// An invitation to scan at launch (debug `-screen join`: Camille's, on the demo).
    var debugInvite: String?
    @ObservationIgnored private var debugInviteMade = false
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
    /// The simulated network runs on a queue of its own, reads and writes in order: catching up
    /// after a pause can take seconds, and must not freeze the screen.
    @ObservationIgnored private let demoQueue = DispatchQueue(label: "lichen.demo", qos: .userInitiated)
    @ObservationIgnored private var demoReading = false
    @ObservationIgnored private var debugSay: String?
    @ObservationIgnored private var debugVoiceSent = false
    @ObservationIgnored private var debugFilled = false
    @ObservationIgnored private var debugAnswered = false
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
        refreshPrefs()
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
        refreshPrefs()
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
        #if DEBUG
        // -offline S: the demo radio drops after S seconds, with a text waiting for Camille and
        // one for the group; -online S: it comes back after S seconds; -expiry S: texts expire
        // after S seconds instead of 24 h.
        let expiry = args.integer(forKey: "expiry")
        if expiry > 0 { core.setOutboxExpiry(ms: UInt64(expiry) * 1000) }
        let off = args.integer(forKey: "offline")
        if off > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(off)) { [weak self] in
                guard let self, self.isDemo else { return }
                self.setDemoRadio(on: false)
                if let c = self.contacts.first(where: { $0.name == "Camille" }) {
                    _ = self.send(String(localized: "Written while my radio is off"), to: .contact(key: c.key))
                }
                if let g = self.groups.first {
                    _ = self.send(String(localized: "On my way, radio off for now"), to: .group(gid: g.gid))
                }
            }
        }
        let on = args.integer(forKey: "online")
        if on > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(on)) { [weak self] in
                guard let self, self.isDemo else { return }
                self.setDemoRadio(on: true)
            }
        }
        #endif
    }

    /// Switches the demo's simulated radio off or on: off, nothing reaches it and the core
    /// pauses, as when Bluetooth drops (§6.1).
    func setDemoRadio(on: Bool) {
        guard isDemo, on == demoRadioOff else { return }
        demoRadioOff = !on
        if on {
            companionState = .connected("ThinkNode M5")
            core.connect(nowMs: now())
        } else {
            companionState = .idle
            core.disconnect(nowMs: now())
        }
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
        refreshPrefs()
        updateLocator()
    }

    private func leaveDemo() {
        demoRadioOff = false
        demoTimer?.invalidate()
        demoFixTimer?.invalidate()
        demoFixTimer = nil
        demo = nil
        isDemo = false
    }

    private func demoStep() {
        guard let demo, !demoReading else { return }
        demoReading = true
        let t = now()
        demoQueue.async { [weak self] in
            let frames = demo.read(nowMs: t)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.demoReading = false
                    guard self.demo === demo else { return }
                    // Radio off: the network goes on, and what it pushes is lost.
                    for f in frames where !self.demoRadioOff {
                        self.core.onCompanionFrame(frame: f, nowMs: t)
                    }
                    self.pump()
                }
            }
        }
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
        prefs = [:]
        focus = nil
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
        let was = isConnected
        companionState = s
        // No radio session with a throwaway identity (`identityProblem`).
        if case .connected = s, identityProblem == nil {
            core.connect(nowMs: now())
            pump()
        } else if was, !isConnected {
            // Lost: texts wait and a transfer pauses until it is back (§6.1).
            core.disconnect(nowMs: now())
            pump()
        }
    }

    private func frame(_ data: Data) {
        core.onCompanionFrame(frame: data, nowMs: now())
        pump()
    }

    /// Our simulated position, read on the demo's queue.
    private func demoFix(_ demo: DemoNetwork) {
        demoQueue.async { [weak self] in
            let fix = demo.myPosition()
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.gotFix(fix) }
            }
        }
    }

    /// Writes due frames, reads events, and sets the timer for the next wake-up.
    func pump() {
        let t = now()
        core.setClock(unixMs: UInt64(Date().timeIntervalSince1970 * 1000), nowMs: t)
        core.tick(nowMs: t)
        var toDemo: [Data] = []
        while let f = core.pollCompanion(nowMs: t) {
            if demo != nil {
                if !demoRadioOff { toDemo.append(f) }
            } else {
                companion.write(f)
            }
        }
        if let demo, !toDemo.isEmpty {
            demoQueue.async { for f in toDemo { demo.write(frame: f, nowMs: t) } }
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
                // -focus <text>: open on the first message found, as a search result does.
                if let q = UserDefaults.standard.string(forKey: "focus"), let m = searchMessages(q).first {
                    focus = (m.chat, m.id)
                }
            }
            if shares.isEmpty, let name = UserDefaults.standard.string(forKey: "share"),
               let c = contacts.first(where: { $0.name == name }) {
                _ = setShare(.contact(key: c.key), precision: .m100, hours: 1.25)
            }
            // -screen invite: a group of your own, alone, to invite people with a QR code.
            if isDemo, debugScreen == "invite", !debugInviteMade,
               let gid = try? core.createGroup(name: String(localized: "Sunday hike"), members: [], nowMs: now()) {
                debugInviteMade = true
                groups = core.groups()
                debugOpen = .group(gid: gid)
            }
            // -screen join: Camille shows an invitation to a group of hers, and you scan it.
            if isDemo, debugScreen == "join", !debugInviteMade,
               contacts.contains(where: { $0.name == "Camille" }) {
                debugInviteMade = true
                debugInvite = demo?.camilleInvite()
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
            // A muted chat stays silent; SOS alerts go through `notifier.sos`, never muted.
            if !isMuted(.contact(key: from)) {
                notifier.message(.contact(key: from), title: name(of: from), body: message.display)
            }
            #if DEBUG
            if let say = debugSay {
                debugSay = nil
                _ = send(say, to: .contact(key: from))
            }
            // -reply <text> / -react <emoji>: answer / react to their first message (§6.2).
            let args = UserDefaults.standard
            if !debugAnswered, args.string(forKey: "reply") != nil || args.string(forKey: "react") != nil {
                debugAnswered = true
                DispatchQueue.main.async { [weak self] in
                    guard let self,
                          let m = (try? self.core.history(chat: .contact(key: from), beforeId: nil, limit: 20))?
                              .last(where: { !$0.outgoing }) else { return }
                    if let r = args.string(forKey: "reply") { _ = self.reply(r, to: m) }
                    if let e = args.string(forKey: "react") { _ = self.react(m, e) }
                }
            }
            #endif
        case .progress, .delivered, .failed, .groupSent:
            break
        case let .reacted(chat, by, emoji, target):
            if let target { upsert(target) }
            // A reaction to one of ours, as messaging apps do; never for a muted chat.
            if let target, target.outgoing, !emoji.isEmpty, !isMuted(chat) {
                let who = name(of: by)
                let what = target.message.display
                notifier.message(chat, title: title(of: chat),
                                 body: String(localized: "\(who) reacted \(emoji) to “\(what)”"))
            }
        case let .groupJoined(gid, _, admin):
            if joining.removeValue(forKey: admin) != nil { joinedByInvite[admin] = gid }
            groupsChanged()
        case .groupUpdated, .groupLeft:
            groupsChanged()
        case let .memberJoined(gid, node):
            groupsChanged()
            let chat = ChatID.group(gid: gid)
            if !isMuted(chat) {
                notifier.message(chat, title: title(of: chat),
                                 body: String(localized: "\(name(of: node)) joined with your invitation"))
            }
        case let .groupMessage(gid, from, message, snr, _):
            lastSnr[from] = snr
            let chat = ChatID.group(gid: gid)
            if !isMuted(chat) {
                notifier.message(chat, title: title(of: chat), body: "\(name(of: from)): \(message.display)")
            }
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
        } else if let i = list.firstIndex(where: { $0.id > m.id }) {
            // In history order, even when one is sent while an earlier one is still reported.
            list.insert(m, at: i)
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

    /// Loads earlier pages until message `id` is there (a search result), 20 pages at most.
    func loadUntil(_ chat: ChatID, _ id: Int64) {
        if chats[chat] == nil { load(chat) }
        var pages = 0
        while !(chats[chat]?.contains { $0.id == id } ?? true), olderAvailable[chat] == true, pages < 20 {
            loadOlder(chat)
            pages += 1
        }
    }

    // MARK: chat essentials (pins, mutes, search)

    func refreshPrefs() {
        prefs = Dictionary(core.chatPrefs().map { ($0.chat, $0) }, uniquingKeysWith: { a, _ in a })
    }

    func isPinned(_ chat: ChatID) -> Bool { prefs[chat]?.pinnedAtMs != nil }

    /// Muted now: its messages post no notification (SOS alerts still do).
    func isMuted(_ chat: ChatID) -> Bool {
        guard let until = prefs[chat]?.mutedUntilMs else { return false }
        return until > Int64(Date().timeIntervalSince1970 * 1000)
    }

    func setPinned(_ chat: ChatID, _ pinned: Bool) {
        try? core.setPinned(chat: chat, pinned: pinned)
        refreshPrefs()
    }

    /// Mutes `chat` for `hours`, or until the user turns it back on (`nil`).
    func mute(_ chat: ChatID, hours: Double?) {
        let until = hours.map { Int64((Date().timeIntervalSince1970 + $0 * 3600) * 1000) } ?? Int64.max
        try? core.setMuted(chat: chat, untilMs: until)
        refreshPrefs()
    }

    func unmute(_ chat: ChatID) {
        try? core.setMuted(chat: chat, untilMs: nil)
        refreshPrefs()
    }

    /// Text messages containing `query` (case and accents aside), newest first.
    func searchMessages(_ query: String) -> [StoredMessage] {
        core.searchMessages(query: query, limit: 50)
    }

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

    /// A small file over LoRa (§13.2, ADR-0022), compressed when smaller (ADR-0023): up to 8 KB
    /// on the air, to a contact.
    func sendFile(_ data: Data, name: String, to chat: ChatID) -> String? {
        guard case let .contact(key) = chat else { return LichenError.MediaInGroup.display }
        return sendMedia(chat) { try core.sendFile(to: key, name: name, data: data, sentAt: $0, nowMs: $1) }
    }

    /// How a small file would go over LoRa to a contact: its size on the air (compressed when
    /// smaller), fragments and airtime; nil for a group, or when it does not fit.
    func loraFile(_ chat: ChatID, name: String, data: Data) -> LoraFile? {
        guard case let .contact(key) = chat else { return nil }
        return (try? core.loraFile(to: key, name: name, data: data)) ?? nil
    }

    /// A copy of a file under its own name, to open or share it (QuickLook and other apps go
    /// by the name's extension).
    func openable(name: String, data: Data? = nil, file: URL? = nil) -> URL? {
        guard let dir = Self.folder("open") else { return nil }
        let safe = fitFileName(name: name, maxBytes: 120)
        let url = dir.appendingPathComponent(safe.isEmpty ? "file" : safe)
        try? FileManager.default.removeItem(at: url)
        if let data {
            return (try? data.write(to: url)) != nil ? url : nil
        }
        if let file {
            return (try? FileManager.default.copyItem(at: file, to: url)) != nil ? url : nil
        }
        return nil
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

    /// Sends a failed text or voice message again: the same message, so that someone who
    /// already has it does not get it twice (§6.1).
    func retry(_ m: StoredMessage) -> String? {
        do {
            upsert(try core.retryMessage(messageId: m.id, nowMs: now()))
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    // MARK: replies and reactions (protocol.md §6.2)

    /// Answers `target` with `text` (a text that waits for the radio like any other).
    func reply(_ text: String, to target: StoredMessage) -> String? {
        do {
            let m = try core.sendReply(chat: target.chat, toMessageId: target.id, text: text,
                                       sentAt: UInt32(Date().timeIntervalSince1970), nowMs: now())
            upsert(m)
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// Reacts to `message` with `emoji` (empty: removes ours).
    func react(_ message: StoredMessage, _ emoji: String) -> String? {
        do {
            upsert(try core.sendReaction(messageId: message.id, emoji: emoji, nowMs: now()))
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// What a reaction costs us in `chat`.
    func reactionCost(_ chat: ChatID) -> Estimate? {
        try? core.estimateReaction(chat: chat)
    }

    /// Cancels a text that is still waiting and none of which went out: it disappears.
    func cancel(_ m: StoredMessage) -> String? {
        do {
            try core.cancelMessage(messageId: m.id)
            chats[m.chat]?.removeAll { $0.id == m.id }
            conversations = core.conversations()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not cancel.")
        }
    }

    /// Airtime left this hour, milliseconds.
    var airtimeLeftMs: UInt64 { budget.limitMs - min(budget.usedMs, budget.limitMs) }

    private func groupsChanged() {
        groups = core.groups()
        contacts = core.contacts().filter(\.person)
        #if DEBUG
        if debugOpen == nil, let name = UserDefaults.standard.string(forKey: "open"),
           let g = groups.first(where: { $0.name == name }) {
            debugOpen = .group(gid: g.gid)
        }
        #endif
    }

    // MARK: invitations by QR code (protocol.md §10.5)

    /// An invitation to a group we administer, for its QR code: 10 minutes, any number of
    /// people, until revoked.
    func createInvite(_ gid: Data) -> Result<InviteCode, LichenError> {
        do {
            let code = try core.createInvite(gid: gid, nowMs: now())
            #if DEBUG
            // -inviteDemo YES: Léa, next to you on the demo network, scans it.
            if isDemo, UserDefaults.standard.bool(forKey: "inviteDemo") { demo?.guestScans(text: code.text) }
            #endif
            return .success(code)
        } catch let e as LichenError {
            return .failure(e)
        } catch {
            return .failure(.BadCode)
        }
    }

    /// Renames a group we administer; its members see the change.
    func renameGroup(_ gid: Data, to name: String) -> String? {
        groupCall { try core.renameGroup(gid: gid, name: name, nowMs: now()) }
    }

    /// The invitation's screen closed: its code admits no one any more.
    func revokeInvite(_ token: Data) { core.revokeInvite(token: token) }

    func parseInvite(_ text: String) -> Result<InviteInfo, LichenError> {
        do {
            return .success(try core.parseInvite(text: text, nowMs: now()))
        } catch let e as LichenError {
            return .failure(e)
        } catch {
            return .failure(.BadCode)
        }
    }

    /// Asks the admin of a scanned invitation to add us (they become a verified contact); the
    /// group arrives as `groupJoined`.
    func acceptInvite(_ text: String, _ info: InviteInfo) -> String? {
        do {
            _ = try core.acceptInvite(text: text, nowMs: now())
            joining[info.admin] = info.groupName
            contacts = core.contacts().filter(\.person)
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not join the group.")
        }
    }

    /// Invites contacts into a group we administer (ADR-0017).
    func addMembers(_ members: [Data], to gid: Data) -> String? {
        groupCall { try core.addMembers(gid: gid, members: members, nowMs: now()) }
    }

    /// Leaves a group (closes it when we are its admin); the conversation stays.
    func leaveGroup(_ gid: Data) -> String? {
        let e = groupCall { try core.leaveGroup(gid: gid, nowMs: now()) }
        shares = core.shares()
        return e
    }

    private func groupCall(_ f: () throws -> Void) -> String? {
        do {
            try f()
            groups = core.groups()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not change the group.")
        }
    }

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
                demoFix(demo)
                demoFixTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated {
                        guard let self, let demo = self.demo else { return }
                        self.demoFix(demo)
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
    /// Offers `original` over the bridge and announces it over LoRa (§14.1): `kind` 1 a photo,
    /// 3 any file (ADR-0022).
    func sendOverBridge(_ original: Data, name: String, to chat: ChatID, anonymous: Bool, kind: UInt8 = 1) -> String? {
        let providers = bridgeProviders(anonymous: anonymous)
        guard let bridge, !providers.isEmpty else {
            return String(localized: "No Internet connection: it cannot be offered.")
        }
        let name = fitFileName(name: name, maxBytes: 32)
        do {
            let offered = try bridge.offer(bytes: original)
            _ = keep(original, hash: offered.hash, name: name)
            let m = try core.sendAnnounce(
                chat: chat, mediaId: Data(), kind: kind, hash: offered.hash, size: offered.size,
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
        fetchTasks[hash] = Task {
            defer {
                fetching[hash] = nil
                fetchTasks[hash] = nil
            }
            var failure = String(localized: "Download failed.")
            for p in route {
                fetching[hash] = p.transport
                do {
                    if p.transport == 2 {
                        let (state, cache) = Self.torDirs
                        _ = try await bridge.startTor(stateDir: state, cacheDir: cache, publish: false, limitS: 180)
                    }
                    let bytes = try await bridge.fetch(transport: p.transport, provider: p.key, hash: hash, size: UInt64(size))
                    // Cancelled meanwhile: dropped.
                    if Task.isCancelled { return }
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
            if !Task.isCancelled { fetchErrors[hash] = failure }
        }
    }

    /// Stops a download: the original is offered again, without an error.
    func cancelFetch(_ m: StoredMessage) {
        guard case let .announce(_, _, _, hash, _, _, _) = m.message else { return }
        fetchTasks[hash]?.cancel()
        fetchTasks[hash] = nil
        fetching[hash] = nil
        fetchErrors[hash] = nil
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

    /// Accepted and waiting (for the radio, a path, a session…), §6.1.
    var isWaiting: Bool {
        if case .waiting = status { return true }
        return false
    }

    /// When the sender wrote it, if well before it arrived (a message that waited, §6.1).
    var writtenEarlier: Date? {
        let written = message.date
        return !outgoing && date.timeIntervalSince(written) > 600 ? written : nil
    }

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
        case let .announce(_, _, kind, _, _, _, name) where kind == 3: return String(localized: "File: \(name)")
        case let .announce(_, _, _, _, _, _, name): return String(localized: "Photo: \(name)")
        case let .file(_, name, _): return String(localized: "File: \(name)")
        case let .other(type, _) where type == 0x03: return String(localized: "Position")
        case .notice:
            return noticeText(me: UserDefaults.standard.data(forKey: "lichen.nodeKey")) { _, name in name }
        case let .other(type, body): return String(localized: "Unsupported content (type \(String(type)), \(String(body.count)) bytes)")
        }
    }

    /// A membership notice as a sentence (ADR-0021): "You" for ourselves (`me`, our node), the
    /// others named by `name(node, name then)`.
    func noticeText(me: Data?, name: (Data, String) -> String) -> String {
        guard case let .notice(kind, actor, actorName, subjects, text) = self else { return "" }
        let mine = actor == me
        let who = name(actor, actorName)
        let names = ListFormatter.localizedString(byJoining: subjects.map { name($0.node, $0.name) })
        switch kind {
        case 1: return mine ? String(localized: "You created the group") : String(localized: "\(who) created the group")
        case 2: return String(localized: "\(who) added you")
        case 3: return String(localized: "\(names) joined")
        case 4: return String(localized: "\(names) left")
        case 5: return mine ? String(localized: "You removed \(names)") : String(localized: "\(who) removed \(names)")
        case 6: return String(localized: "No longer in the group: \(names)")
        case 7: return String(localized: "\(who) removed you")
        case 8: return mine ? String(localized: "You closed the group") : String(localized: "\(who) closed the group")
        case 9: return mine ? String(localized: "You renamed the group “\(text)”") : String(localized: "\(who) renamed the group “\(text)”")
        case 10: return String(localized: "You left the group")
        default: return String(localized: "The group changed")
        }
    }

    var date: Date {
        switch self {
        case let .text(sentAt, _), let .voice(sentAt, _, _, _, _), let .image(sentAt, _, _, _, _, _, _),
             let .announce(sentAt, _, _, _, _, _, _), let .file(sentAt, _, _):
            if sentAt > 0 { return Date(timeIntervalSince1970: TimeInterval(sentAt)) }
        case .notice, .other:
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
        case .expired: return String(localized: "expired")
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
        case .NotCancellable: return String(localized: "Part of it already went out: it can no longer be cancelled.")
        case .NotReferable: return String(localized: "This message is from before replies existed: it cannot be answered.")
        case .NoClock: return String(localized: "The phone's clock is not set yet.")
        case .Expired: return String(localized: "This invitation has expired: ask for a new one.")
        }
    }
}
