import CoreLocation
import Foundation
import Observation
import UIKit
import UniformTypeIdentifiers

/// A conversation: `ChatRef` comes from the core (contact, group, or a plain MeshCore chat).
typealias ChatID = ChatRef

extension ChatRef {
    /// A plain MeshCore chat (protocol.md §15): text only, not lichen-encrypted.
    var isPlain: Bool {
        switch self {
        case .mesh, .channel, .room: true
        case .contact, .group: false
        }
    }

    /// Its id: a node key, a group id or a channel id.
    var bytes: Data {
        switch self {
        case let .contact(key), let .mesh(key), let .room(key): key
        case let .group(gid): gid
        case let .channel(id): id
        }
    }
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
    /// New identities presented by verified contacts, waiting for the user (node → identity).
    var conflicts: [Data: Data] = [:]
    /// The recovery phrase was written down (Identity screen).
    var phraseBackedUp = UserDefaults.standard.bool(forKey: "lichen.phraseBackedUp") {
        didSet { UserDefaults.standard.set(phraseBackedUp, forKey: "lichen.phraseBackedUp") }
    }
    /// Contacts (node keys, hex) whose safety numbers the user said to compare later: their
    /// chat no longer asks (B-user-1).
    var compareLater = Set(UserDefaults.standard.stringArray(forKey: "lichen.compareLater") ?? []) {
        didSet { UserDefaults.standard.set(Array(compareLater).sorted(), forKey: "lichen.compareLater") }
    }
    /// The pairing screen, asked for from Messages (no radio paired yet, D-1).
    var showPairing = false
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
    /// Received GIFs load by themselves, without Tor too: GIPHY then sees our IP address
    /// (ADR-0025). With Tor ready they always do, through it.
    var gifAutoLoad = UserDefaults.standard.bool(forKey: "lichen.gifAuto") {
        didSet { UserDefaults.standard.set(gifAutoLoad, forKey: "lichen.gifAuto") }
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
    /// Every contact the core keeps, people or not (a node its radio files as a repeater):
    /// the people go to `contacts`; a conversation with any of them can be renamed.
    var coreContacts: [ContactInfo] = [] {
        didSet { contacts = coreContacts.filter(\.person) }
    }
    /// People: the contacts lichen can write to.
    var contacts: [ContactInfo] = []
    /// MeshCore nodes the radio knows or heard (protocol.md §16.1).
    var nodes: [NodeInfo] = []
    /// Bumped when a trace or a server's answer changes (§16): views read them again.
    var meshToolsVersion = 0
    /// Radios this phone was paired with (ADR-0042).
    var savedRadios: [SavedRadio] = []
    /// What the radio reported of its settings (protocol §1.1), read again after each change.
    var radioSettings: RadioSettings?
    /// Why the radio did not take a setting, for an alert.
    var radioError: String?
    /// Contacts from a link that the radio did not add (§16.6), counted: Add a contact watches it.
    var linkRefusals = 0

    /// A radio setting row's state: waiting for the radio (`outcome` nil), or what became of
    /// the setting changed from it (B-user-5).
    struct RadioRowState: Equatable {
        var outcome: SettingOutcome?
        var at: Date
    }
    /// Each radio setting row the user changed, by its key.
    var radioRows: [String: RadioRowState] = [:]
    /// The radio's settings with the values asked for while they wait, else nil: the screen
    /// shows these until the radio's come back (B-user-5).
    var radioAsked: RadioSettings?
    /// The row each kind of setting was last changed from.
    @ObservationIgnored private var radioRowOf: [RadioSetting: String] = [:]
    /// How long "Saved on the radio" stays on its row: 2 s; debug `-radioSavedFor S` for the UI
    /// tests on a busy machine, whose screen reads can take longer.
    @ObservationIgnored var radioSavedSeconds: Double = {
        #if DEBUG
        let s = UserDefaults.standard.double(forKey: "radioSavedFor")
        if s > 0 { return s }
        #endif
        return 2
    }()

    /// Changes one of the radio's settings (ADR-0032) from `row`: the row waits, showing the
    /// value `asked` for, until the radio's outcome (B-user-5). One that cannot be sent (out of
    /// range, no radio) shows as `radioError`. The settings shown come back from the radio.
    func radioSet(_ setting: RadioSetting, row: String, asked: (inout RadioSettings) -> Void,
                  _ change: () throws -> Void) {
        do {
            try change()
            radioRowOf[setting] = row
            radioRows[row] = RadioRowState(outcome: nil, at: Date())
            if var r = radioAsked ?? radioSettings {
                asked(&r)
                radioAsked = r
            }
            // The core only queues the command: sent now, not when something else wakes the
            // app (B-user-6).
            pump()
        } catch let e as LichenError {
            radioError = e.display
        } catch {
            radioError = error.localizedDescription
        }
    }

    /// Reads the radio's battery and storage again (Radio settings opened).
    func refreshRadio() {
        core.refreshRadio()
        pump()
    }

    /// Restarts the radio: the connection drops and comes back.
    func rebootRadio() {
        core.rebootRadio()
        pump()
    }

    /// A row's outcome, read: it goes.
    func clearRadioRow(_ row: String) {
        if radioRows[row]?.outcome != nil { radioRows[row] = nil }
    }
    /// The radio's MeshCore channels (§15.3).
    var channels: [MeshChannel] = []
    #if DEBUG
    @ObservationIgnored private var debugPlainSaid = false
    #endif
    var groups: [GroupInfo] = []
    /// Groups that list us and wait for an answer (§10.1): someone not verified in person
    /// cannot put us in a group.
    var invitations: [GroupInvitation] = []
    /// Someone using a contact's radio, with an identity it does not have, wants to write
    /// (protocol.md §4.6, ADR-0046): kept apart until the user accepts, blocks or deletes.
    var requests: [IdentityRequest] = []
    /// Contacts whose phone speaks a newer protocol version (ADR-0029).
    var newerPeers: Set<Data> = []
    /// Each open conversation's disappearing-message timer, seconds; 0: off (§6.5).
    var timers: [ChatID: UInt32] = [:]
    /// What the timer menu offers: off, 5 minutes, 1 hour, 8 hours, 1 day, 1 week.
    static let timerChoices: [UInt32] = [0, 300, 3_600, 28_800, 86_400, 604_800]

    /// Messages of `chat` stored from now on disappear after `seconds` (0: they stay). A lichen
    /// conversation gets the same setting; a MeshCore chat keeps it on this phone.
    func setTimer(_ chat: ChatID, _ seconds: UInt32) -> String? {
        do {
            try core.setDisappearing(chat: chat, seconds: seconds)
            timers[chat] = seconds
            // A lichen conversation is sent the timer (B-user-6).
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not change the timer.")
        }
    }

    /// Joins the group of an invitation; it then appears with the others.
    func acceptGroup(_ gid: Data) {
        try? core.acceptGroup(gid: gid, nowMs: now())
        groupsChanged()
        pump()
    }

    /// Forgets an invitation; its admin is not told.
    func declineGroup(_ gid: Data) {
        try? core.declineGroup(gid: gid)
        groupsChanged()
    }

    /// Accepts a request: its identity becomes the contact's (unverified), what it sent moves
    /// into the conversation, which opens.
    func acceptRequest(_ identity: Data) {
        if let node = try? core.acceptRequest(identity: identity) {
            coreContacts = core.contacts()
            conversations = core.conversations()
            openRequest = .contact(key: node)
        }
        requests = core.requests()
        pump()
    }

    /// Blocks a request's identity, not the radio it came through (someone else's).
    func blockRequest(_ identity: Data) {
        try? core.blockRequest(identity: identity)
        requests = core.requests()
        refreshPrefs()
    }

    /// Deletes a request and what it sent; if it writes again, it is a new request.
    func deleteRequest(_ identity: Data) {
        try? core.deleteRequest(identity: identity)
        requests = core.requests()
    }
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
    /// A conversation to open once: a notification tapped, a contact just added, a group just
    /// joined, an SOS answered, or the debug launch argument `-open <name>`. The conversation
    /// list opens it and clears it.
    var openRequest: ChatID?
    #if DEBUG
    /// `-open <name>` chose this node's MeshCore chat before its lichen identity arrived.
    @ObservationIgnored var debugOpenedMesh: Data?
    /// `-open <name>` was resolved: once per launch.
    @ObservationIgnored var debugOpened = false
    /// `-roomLogin <password>` was tried: once per launch.
    @ObservationIgnored var debugRoomLogin = false
    #endif
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

    /// Our advert, asked for in Settings (B-user-4): waiting for the radio's answer, then sent
    /// (when, how far) or not.
    enum AdvertState: Equatable {
        case sending(flood: Bool)
        case sent(flood: Bool, at: Date)
        case notSent(flood: Bool)
    }
    var advert: AdvertState?
    /// SOS incidents heard, by sender node: one alert each (`Search.swift`).
    var incidents: [Data: SosIncident] = [:]
    /// The incident whose full-screen alert is up, until dismissed.
    var sosAlert: SenderID?
    /// The person the Locate tab shows.
    var searchTarget: Data?
    /// Searches we run (protocol.md §11); `searchVersion` changes with their estimates.
    var searches: [SearchState] = []
    var searchVersion = 0
    /// Members searching for us, and our radio's power while a search holds it lowered.
    var searchedBy: [SearchedBy] = []
    var loweredPower: Int8?
    /// What our SOS is doing.
    var sosStatus: SosStatus?
    /// The compass heading, degrees from true north, while the Locate tab is on screen.
    var heading: Double?
    @ObservationIgnored var lastBatteryReport: Date?
    /// Conversations with older messages to load, and those whose history failed to load.
    var olderAvailable: [ChatID: Bool] = [:]
    var historyError: [ChatID: String] = [:]
    /// A screen to show at launch (debug launch argument `-screen <name>`, for screenshots).
    var debugScreen: String?
    /// Image previews whose full variant we asked for.
    var requestedFull: Set<Int64> = []
    /// Contacts' requests for the full variant of our photos, waiting for the user (§13.3),
    /// by conversation; in memory, like the core's.
    var fullRequests: [ChatID: [FullImageRequest]] = [:]
    let recorder = VoiceRecorder()
    @ObservationIgnored let notifier = Notifier()
    /// The app lock (Settings › Privacy): app only, the radio goes on behind it.
    @ObservationIgnored let lock = AppLock()
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
        /// The message database cannot be opened (full or damaged storage): nothing could be
        /// sent or kept (recheck R02).
        case storage
        /// The key of the encrypted database cannot be read now (ADR-0036): try again.
        case databaseKeyUnavailable
        /// The database is encrypted but its key is gone (the Keychain was reset): a new key
        /// would not open it. Restore a backup, or start again with an empty history.
        case databaseKeyLost
    }
    @ObservationIgnored let companion = Companion()
    /// The companion over TCP (Wi-Fi, or `lichen bridge`): used instead of Bluetooth while the
    /// user wants it (`Link.swift`).
    @ObservationIgnored let tcp = TcpCompanion()
    /// How the radio is reached now.
    var link = RadioLink.bluetooth
    /// Why the last TCP connection failed, for the pairing screen.
    var tcpError: String?
    /// The app's recent log, for problem reports (ADR-0045).
    @ObservationIgnored var appLog: [ReportLine] = []
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var demo: DemoNetwork?
    @ObservationIgnored private var demoTimer: Timer?
    /// The simulated network runs on a queue of its own, reads and writes in order: catching up
    /// after a pause can take seconds, and must not freeze the screen.
    @ObservationIgnored private let demoQueue = DispatchQueue(label: "lichen.demo", qos: .userInitiated)
    @ObservationIgnored private var demoReading = false
    @ObservationIgnored private var debugSay: String?
    @ObservationIgnored private var debugStickerSent = false
    @ObservationIgnored private var debugVoiceSent = false
    @ObservationIgnored private var debugFilled = false
    @ObservationIgnored private var debugAnswered = false
    @ObservationIgnored private let started = ProcessInfo.processInfo.systemUptime
    /// The core's clock (`now()`): `clockBase` ms at uptime `clockMark`, running `clockSpeed`
    /// times faster than the wall clock (1, but up to 4 in the demo with debug `demoSpeed`).
    @ObservationIgnored private var clockBase: UInt64 = 0
    @ObservationIgnored private var clockMark = ProcessInfo.processInfo.systemUptime
    @ObservationIgnored private(set) var clockSpeed = 1.0
    @ObservationIgnored let locator = Locator()
    @ObservationIgnored private var lastFed: UInt64?
    @ObservationIgnored private var demoFixTimer: Timer?
    @ObservationIgnored private var bridge: LichenBridge?

    private static let nodeKeyDefault = "lichen.nodeKey"

    init() {
        Self.clearOpenCopies()
        UserDefaults.standard.removeObject(forKey: "lichen.drafts")
        switch Self.loadIdentity() {
        case let .success(seed):
            identitySeed = seed
            let (made, problem) = Self.persistentCore(seed: seed)
            core = made
            identityProblem = problem
            savedRadios = core.savedRadios()
        case let .failure(problem):
            identityProblem = problem
            core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        }
        coreContacts = core.contacts()
        groups = core.groups()
        // Invitations not answered yet are stored: back after a restart (§10.1).
        invitations = core.groupInvitations()
        requests = core.requests()
        channels = core.channels()
        conversations = core.conversations()
        refreshPrefs()
        companion.onState = { [weak self] s in MainActor.assumeIsolated { self?.companionChanged(s) } }
        companion.onFound = { [weak self] f in MainActor.assumeIsolated { self?.found = f } }
        companion.onFrame = { [weak self] d in MainActor.assumeIsolated { self?.bluetoothFrame(d) } }
        setUpTcp()
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
            openRequest = chat
        }
        notifier.locked = { [weak self] in self?.lock.locked ?? false }
        // Reply and Mark as read from a notification (ADR-0034): the answer is a message like
        // any other, in the outbox when the radio is away.
        notifier.onReply = { [weak self] chat, text in
            guard let self else { return }
            if let error = send(text, to: chat) {
                notifier.notSent(chat, reason: error)
            } else {
                markRead(chat)
                notifier.clear(chat)
            }
        }
        notifier.onMarkRead = { [weak self] chat in
            guard let self else { return }
            markRead(chat)
            notifier.clear(chat)
        }
        locator.onAuthorization = { [weak self] s in self?.locationDenied = s == .denied || s == .restricted }
        locationDenied = [.denied, .restricted].contains(locator.authorization)
        shares = core.shares()
        positions = core.positions(nowMs: now())
        updateLocator()
        // No download runs yet: those an earlier run left unfinished, and those no stored
        // message announces any more, go (second read of 2026-11-17, N14). The core does
        // nothing without this identity's database on disk.
        if let dir = Self.folder("downloads") { _ = core.sweepDownloads(dir: dir.path) }
        loadDownloads()
        startBridge()
        #if DEBUG
        let args = UserDefaults.standard
        debugSay = args.string(forKey: "say")
        if args.bool(forKey: "demo") { startDemo() }
        switch args.string(forKey: "tab") {
        case "map": tab = .map
        case "locate": tab = .search
        case "settings": tab = .settings
        default: break
        }
        debugScreen = args.string(forKey: "screen")
        if debugScreen == "privacy" { tab = .settings }
        // -identityProblem unavailable|corrupt|notSaved|storage|databaseKey|databaseKeyLost: the
        // identity (or database) failure screen.
        switch args.string(forKey: "identityProblem") {
        case "unavailable": identityProblem = .unavailable
        case "corrupt": identityProblem = .corrupt
        case "notSaved": identityProblem = .notSaved
        case "storage": identityProblem = .storage
        case "databaseKey": identityProblem = .databaseKeyUnavailable
        case "databaseKeyLost": identityProblem = .databaseKeyLost
        default: break
        }
        debugSavedRadios()
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
            companionState = linkState
        }
        resetState()
        identitySeed = seed
        let (made, problem) = Self.persistentCore(seed: seed)
        core = made
        identityProblem = problem
        savedRadios = core.savedRadios()
        coreContacts = core.contacts()
        groups = core.groups()
        // Invitations not answered yet are stored: back after a restart (§10.1).
        invitations = core.groupInvitations()
        requests = core.requests()
        channels = core.channels()
        conversations = core.conversations()
        refreshPrefs()
        shares = core.shares()
        positions = core.positions(nowMs: now())
        if isConnected { core.connect(nowMs: now()) }
        restartBridge()
        updateLocator()
        pump()
    }

    /// The core of our identity with its database, or a throwaway one and why the database
    /// cannot be used. The database is encrypted (ADR-0036): its key is drawn at the first
    /// start, then always read; a database already encrypted is never given a new key.
    private static func persistentCore(seed: Data) -> (LichenCore, IdentityProblem?) {
        switch databaseKey(seed: seed) {
        case let .success(key):
            let core = makeCore(seed: seed, databaseKey: key)
            return (core, core.storageOk() ? nil : .storage)
        case let .failure(problem):
            return (makeCore(seed: Keychain.randomSeed(), persistent: false), problem)
        }
    }

    /// The database key, drawn at the first start; nil when the core keeps its database in
    /// clear (built without SQLCipher).
    private static func databaseKey(seed: Data) -> Result<Data?, IdentityProblem> {
        guard databaseEncryption(), let dir = dataDirectory() else { return .success(nil) }
        switch Keychain.readDatabaseKey() {
        case let .found(key):
            return .success(key)
        case .unavailable:
            return .failure(.databaseKeyUnavailable)
        case .absent, .lost:
            switch try? encryptedDatabase(dataDir: dir, identitySeed: seed) {
            case true?:
                // A database already encrypted needs its own key: a new one would not open it.
                return .failure(.databaseKeyLost)
            case false?:
                guard let key = try? Keychain.createDatabaseKey() else { return .failure(.databaseKeyUnavailable) }
                return .success(key)
            case nil:
                return .failure(.databaseKeyUnavailable)
            }
        }
    }

    private static func makeCore(seed: Data, persistent: Bool = true, databaseKey: Data? = nil) -> LichenCore {
        if persistent, let databaseKey {
            // The seed and the key are 32 bytes: construction cannot fail.
            return try! LichenCore.withDatabaseKey(
                appName: "lichen-ios", rngSeed: Keychain.randomSeed(), identitySeed: seed,
                dataDir: dataDirectory(), databaseKey: databaseKey
            )
        }
        // The seed is 32 bytes and the identity 64 or absent: construction cannot fail.
        return try! LichenCore(
            appName: "lichen-ios",
            rngSeed: Keychain.randomSeed(),
            identitySeed: seed,
            dataDir: persistent ? dataDirectory() : nil
        )
    }

    /// The user starts again with an empty history after the database key was lost
    /// (`databaseKeyLost`): the unreadable database goes (messages, contacts, groups), a new key
    /// is drawn. The identity and the downloads stay.
    func discardUnreadableHistory() -> String? {
        guard let seed = identitySeed, let dir = Self.dataDirectory() else { return LichenError.Storage.display }
        do {
            try discardDatabase(dataDir: dir, identitySeed: seed)
        } catch {
            return LichenError.Storage.display
        }
        useIdentity(seed)
        return nil
    }

    /// Everything lichen keeps lives in Application Support/lichen (file protection until first
    /// unlock): the databases, offered and downloaded media, Tor's state, the drafts. The
    /// folder is excluded from iCloud and Finder backups: a backup would carry the history
    /// and the group keys off the phone (review of 2026-11-15, SR-22). The identity is in the
    /// Keychain (this device only) and comes back from the 24 words.
    private static func dataDirectory() -> String? {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        else { return nil }
        var dir = base.appendingPathComponent("lichen", isDirectory: true)
        try? FileManager.default.createDirectory(
            at: dir, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
        )
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? dir.setResourceValues(values)
        return dir.path
    }

    #if DEBUG
    /// -freshInstall YES: the app starts as if just installed (UI tests): its identity and
    /// database key leave the Keychain, its data folder and its settings go. Before anything
    /// reads them.
    static func debugFreshInstall() {
        guard UserDefaults.standard.bool(forKey: "freshInstall") else { return }
        Keychain.debugErase()
        if let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            try? FileManager.default.removeItem(at: base.appendingPathComponent("lichen", isDirectory: true))
        }
        if let id = Bundle.main.bundleIdentifier { UserDefaults.standard.removePersistentDomain(forName: id) }
    }

    /// -savedRadios YES: two radios this phone used before, in its list (Settings › Radios),
    /// once; nothing answers at their addresses.
    private func debugSavedRadios() {
        guard UserDefaults.standard.bool(forKey: "savedRadios"), !isDemo, savedRadios.isEmpty else { return }
        try? core.rememberRadio(address: "6A1F0C2E-5B7D-4E39-9C1A-2F8E4D6B3A10", link: .bluetooth, name: "Heltec V4")
        try? core.rememberRadio(address: "127.0.0.1:5999", link: .tcp, name: "Workshop bridge")
        savedRadios = core.savedRadios()
    }
    #endif

    // MARK: demo network
    // MARK: demo network

    /// Runs the app on a simulated MeshCore network (two simulated users, a group), in real time.
    func startDemo() {
        resetState()
        core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        // The demo's people are friends: their groups are joined without asking, and their
        // introductions are not requests (ADR-0046); -requestDemo YES keeps requests on, to see
        // Camille's first message arrive as one.
        #if DEBUG
        // -groupInvitationDemo YES: the demo keeps group invitations on: Camille's group arrives
        // as an invitation in Messages, to join or decline (§10.1).
        let groupInvitationDemo = UserDefaults.standard.bool(forKey: "groupInvitationDemo")
        #else
        let groupInvitationDemo = false
        #endif
        if !groupInvitationDemo { core.setGroupConsent(ask: false) }
        #if DEBUG
        let requestDemo = UserDefaults.standard.bool(forKey: "requestDemo")
        #else
        let requestDemo = false
        #endif
        if !requestDemo { core.setRequestConsent(ask: false) }
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
        // -marcRepeater YES: the radio files Marc as a repeater; his chat stays in Messages.
        if args.bool(forKey: "marcRepeater") { net.marcAsRepeater() }
        // -demoSkip S: the script's timed events come S seconds sooner, in the same order (UI
        // tests: Camille's first message in about 10 s instead of 30).
        let skip = args.integer(forKey: "demoSkip")
        if skip > 0 { net.skipAhead(seconds: UInt64(skip)) }
        // -quietDemo YES: no group, photo, position share or channel post: Camille answers in
        // seconds (UI tests of direct chats).
        if args.bool(forKey: "quietDemo") { net.quiet() }
        // -radioRefuses power: the demo radio refuses transmit power changes (D-4).
        if args.string(forKey: "radioRefuses") == "power" { net.refuseTxPower() }
        // -radioIgnores <kind>: the demo radio answers OK and keeps its old value (B-user-5).
        if let kind = args.string(forKey: "radioIgnores") { net.ignoreSettings(kind: kind) }
        // -radioSlow S: the demo radio holds its answers for S seconds of the demo's time (D-9).
        let slow = args.integer(forKey: "radioSlow")
        if slow > 0 { net.holdReplies(seconds: UInt64(slow)) }
        // -answerAfter S: Camille and Julien answer a text S seconds after it reaches them, not 4
        // (UI tests: room to block Camille before her answer is written, CT-10).
        let answerAfter = args.integer(forKey: "answerAfter")
        if answerAfter > 0 { net.answerAfter(seconds: UInt64(answerAfter)) }
        // -plainHeard YES: what Lucie and Marc send with the stock app reaches the radio however
        // busy the demo's air was: one the air lost is handed over 20 s later (UI tests).
        if args.bool(forKey: "plainHeard") { net.plainHeard() }
        // -radioNoFix YES: the demo radio's GPS is on but has no fix, no position (B-user-2).
        if args.bool(forKey: "radioNoFix") { net.gpsWithoutFix() }
        // -manyContacts N: the demo radio also knows N people with the stock MeshCore app (F-4).
        let many = args.integer(forKey: "manyContacts")
        if many > 0 { net.manyContacts(count: UInt32(many)) }
        // -demoSpeed N (up to 4): the demo network and the core run N times faster than the wall
        // clock (UI tests). Never outside the demo.
        let speed = min(4, max(1, args.integer(forKey: "demoSpeed")))
        net.setSpeed(speed: UInt32(speed))
        setClockSpeed(Double(speed))
        #endif
        companionState = .connected("ThinkNode M5")
        core.connect(nowMs: now())
        // Every 0.2 s of the demo's time.
        demoTimer = Timer.scheduledTimer(withTimeInterval: 0.2 / clockSpeed, repeats: true) { [weak self] _ in
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
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(off) / clockSpeed) { [weak self] in
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
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(on) / clockSpeed) { [weak self] in
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

    /// Imports the database the official MeshCore app exports (ADR-0033): what came in, or why
    /// not.
    func importMeshCore(from url: URL) -> String {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let r = try core.importMeshcoreApp(path: url.path)
            var text = String(localized: "\(Int(r.direct)) direct messages and \(Int(r.channel)) channel posts imported.")
            if r.alreadyHere > 0 {
                text += " " + String(localized: "\(Int(r.alreadyHere)) were already here.")
            }
            return text
        } catch let e as LichenError {
            return e.display
        } catch {
            return LichenError.NotAnExport.display
        }
    }

    func stopDemo() {
        leaveDemo()
        resetState()
        switch Self.loadIdentity() {
        case let .success(seed):
            identitySeed = seed
            let (made, problem) = Self.persistentCore(seed: seed)
            core = made
            identityProblem = problem
            savedRadios = core.savedRadios()
        case let .failure(problem):
            identityProblem = problem
            core = Self.makeCore(seed: Keychain.randomSeed(), persistent: false)
        }
        companionState = linkState
        shares = core.shares()
        refreshPrefs()
        updateLocator()
    }

    private func leaveDemo() {
        if clockSpeed != 1 { setClockSpeed(1) }
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
        requests = []
        coreContacts = []
        nodes = []
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
        advert = nil
    }

    var isConnected: Bool {
        if case .connected = companionState { return true }
        return false
    }

    // MARK: runner

    func now() -> UInt64 {
        let uptime = ProcessInfo.processInfo.systemUptime
        guard clockSpeed > 1 else { return clockBase + UInt64((uptime - clockMark) * 1000 * clockSpeed) }
        // -demoSpeed: a stall of the app (a UI test's accessibility queries) is not multiplied
        // into a jump past the core's 5 s wait for the radio's answer, which would then be
        // dropped: at most 0.25 s of the wall clock counts per reading.
        clockBase += UInt64(min(max(uptime - clockMark, 0), 0.25) * 1000 * clockSpeed)
        clockMark = uptime
        return clockBase
    }

    /// The core's clock goes on from where it is, `speed` times faster than the wall clock.
    private func setClockSpeed(_ speed: Double) {
        clockBase = now()
        clockMark = ProcessInfo.processInfo.systemUptime
        clockSpeed = speed
    }

    private func companionChanged(_ s: Companion.State) {
        note("Bluetooth: \(s.logWord)")
        // A TCP link the user asked for comes first (`Link.swift`).
        if tcp.target != nil { return }
        link = .bluetooth
        linkChanged(s)
    }

    /// The radio link in use (Bluetooth or TCP) is now `s`.
    func linkChanged(_ s: Companion.State) {
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

    func frame(_ data: Data) {
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
        // The radio's battery, for the low-battery warning (ADR-0042).
        core.refreshBatteryIfDue(nowMs: t)
        var toDemo: [Data] = []
        while let f = core.pollCompanion(nowMs: t) {
            if demo != nil {
                if !demoRadioOff { toDemo.append(f) }
            } else {
                writeRadio(f)
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
            // The core's time to wait, on the wall clock.
            let delay = max(0.02, Double(Int64(next) - Int64(t)) / 1000 / clockSpeed)
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
            radioSettings = core.radioSettings()
            UserDefaults.standard.set(key, forKey: Self.nodeKeyDefault)
            radioConnected()
        case let .identityLearned(node, _, _):
            coreContacts = core.contacts()
            #if DEBUG
            // -open <name>: opened before their identity arrived, as a MeshCore chat; now that
            // they turn out to have lichen, their conversation. Only for the launch argument.
            if debugOpenedMesh == node {
                debugOpenedMesh = nil
                openRequest = .contact(key: node)
            }
            #endif
        case let .identityConflict(node, identity):
            conflicts[node] = identity
        case let .radioOutcome(outcome):
            // What became of a setting (B-user-5): said on its row. A contact added from a link
            // that the radio did not add (§16.6) is said on Add a contact.
            if outcome.setting == .contact {
                if outcome.outcome == .refused { linkRefusals += 1 }
            } else if let row = radioRowOf[outcome.setting] {
                radioRows[row] = RadioRowState(outcome: outcome.outcome, at: Date())
            }
            // Nothing waits any more: the radio's own values.
            if core.radioPending().isEmpty { radioAsked = nil }
            radioSettings = core.radioSettings()
        case .radioSettings:
            // Read again after a change (ADR-0032): the header follows the radio's parameters.
            let r = core.radioSettings()
            radioSettings = r
            radio = core.radio()
            if !r.name.isEmpty { nodeName = r.name }
        case .contactsUpdated:
            coreContacts = core.contacts()
            nodes = core.nodes()
            #if DEBUG
            if !debugOpened, let name = UserDefaults.standard.string(forKey: "open"),
               let c = contacts.first(where: { $0.name == name }) {
                // Someone without lichen (the demo's Marc): their MeshCore chat (§15).
                openRequest = c.identity == nil ? .mesh(key: c.key) : .contact(key: c.key)
                debugOpened = true
                if c.identity == nil { debugOpenedMesh = c.key }
            } else if !debugOpened, let name = UserDefaults.standard.string(forKey: "open"),
                      let room = nodes.first(where: { $0.kind == .roomServer && $0.name == name }) {
                // A room server's posts (§15.8), as its row in Messages opens them.
                openRequest = .room(key: room.key)
                debugOpened = true
            }
            // -roomLogin <password>: log in to the radio's room servers once, remembered (§15.8):
            // once the radio lists them (its first contacts may come before the room servers).
            if !debugRoomLogin, let password = UserDefaults.standard.string(forKey: "roomLogin") {
                let rooms = nodes.filter { $0.kind == .roomServer }
                if !rooms.isEmpty { debugRoomLogin = true }
                for room in rooms {
                    _ = serverLogin(room.key, password: password, remember: true)
                }
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
                openRequest = .group(gid: gid)
            }
            // -screen join: Camille shows an invitation to a group of hers, and you scan it.
            if isDemo, debugScreen == "join", !debugInviteMade,
               contacts.contains(where: { $0.name == "Camille" }) {
                debugInviteMade = true
                debugInvite = demo?.camilleInvite()
            }
            // -contactDemo scan|paste: Léa, next to you, shows her contact code: the scanner
            // reads it in person (the camera's stand-in), or it waits on the pasteboard.
            if isDemo, !debugInviteMade, let how = UserDefaults.standard.string(forKey: "contactDemo"),
               let card = demo?.leaCard() {
                debugInviteMade = true
                if how == "scan" { debugInvite = card } else { UIPasteboard.general.string = card }
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
            prefetchGif(message)
            // A muted chat stays silent; SOS alerts go through `notifier.sos`, never muted.
            if !isMuted(.contact(key: from)) {
                notifier.message(.contact(key: from), title: name(of: from), body: message.display)
            }
            #if DEBUG
            if let say = debugSay {
                debugSay = nil
                _ = send(say, to: .contact(key: from))
            }
            // -sticker <emoji>: a sticker to whoever writes first (Camille answers with one).
            if let emoji = UserDefaults.standard.string(forKey: "sticker"), !debugStickerSent {
                debugStickerSent = true
                _ = sendSticker(emoji, to: .contact(key: from))
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
        case let .newerVersion(node, _):
            newerPeers.insert(node)
        case let .messageRemoved(id, chat):
            // Its time was up (§6.5): off the screen, and off the lock screen.
            chats[chat]?.removeAll { $0.id == id }
            conversations = core.conversations()
            notifier.clear(chat)
        case let .mediaGone(hash):
            // A deleted message's content: no longer offered, no longer kept (§6.4, SR-28).
            try? bridge?.revoke(hash: hash)
            if let url = downloads.removeValue(forKey: hash) {
                try? FileManager.default.removeItem(at: url)
            }
        case let .groupInvited(_, name, admin, _):
            groupsChanged()
            // Tapping it opens the admin's conversation; the invitation waits in Messages.
            notifier.message(.contact(key: admin), title: String(localized: "Group invitation"),
                             body: String(localized: "\(self.name(of: admin)) invites you to “\(name)”"), actions: false)
        case let .groupJoined(gid, name, admin):
            // The group whose invitation we accepted, by its admin and its name: another of
            // theirs that adds us meanwhile is not it.
            if joining[admin] == name {
                joining.removeValue(forKey: admin)
                joinedByInvite[admin] = gid
            }
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
        case let .groupMessage(gid, from, message, snr, _, _):
            prefetchGif(message)
            lastSnr[from] = snr
            let chat = ChatID.group(gid: gid)
            if !isMuted(chat) {
                notifier.message(chat, title: title(of: chat), body: "\(name(of: from)): \(message.display)")
            }
        case let .plainReceived(chat, label, text, mentionsUs):
            // A MeshCore message (§15.1): "Marc · MeshCore", or "Lucie · Public" on a channel.
            // Never like a lichen chat's: a node can send plain text under any name, and its
            // notification must not pass for the encrypted conversation (review of
            // 2026-11-15, SR-30). One that mentions us notifies even when muted (§15.9).
            if mentionsUs {
                // "Lucie mentioned you in Public", even when muted.
                let who = label ?? self.title(of: chat)
                let title: String = switch chat {
                case .channel, .room: String(localized: "\(who) mentioned you in \(self.title(of: chat))")
                default: String(localized: "\(who) mentioned you")
                }
                notifier.message(chat, title: title, body: meshcoreDisplayText(text: text))
            } else if !isMuted(chat) {
                let title: String = switch chat {
                case .channel, .room: [label, self.title(of: chat)].compactMap { $0 }.joined(separator: " · ")
                default: "\(label ?? self.title(of: chat)) · MeshCore"
                }
                notifier.message(chat, title: title, body: meshcoreDisplayText(text: text))
            }
            #if DEBUG
            // -sayPlain <text>: answer the first MeshCore direct message.
            if case .mesh = chat, let say = UserDefaults.standard.string(forKey: "sayPlain"), !debugPlainSaid {
                debugPlainSaid = true
                _ = send(say, to: chat)
            }
            #endif
        case .chatPrefsChanged:
            // An archived conversation came back with a message (ADR-0034).
            refreshPrefs()
        case let .identityRequested(node, _):
            // ADR-0046: it waits in Messages; tapping the notification opens the app there.
            requests = core.requests()
            notifier.request(title: String(localized: "Message request"),
                             body: String(localized: "Someone using \(self.name(of: node))’s radio wants to write to you"))
        case .requestsChanged:
            requests = core.requests()
        case .channelsUpdated:
            channels = core.channels()
            refreshPrefs()
            #if DEBUG
            // -open Public: a MeshCore channel.
            if !debugOpened, let name = UserDefaults.standard.string(forKey: "open"),
               let c = channels.first(where: { $0.name == name }) {
                openRequest = .channel(id: c.id)
                debugOpened = true
            }
            #endif
        case let .messageChanged(m):
            upsert(m)
            conversations = core.conversations()
            // A timer changed, by us or by them (§6.5).
            if case let .notice(11, _, _, _, text) = m.message, let s = UInt32(text) { timers[m.chat] = s }
            // Taken back by its author (§6.4): its notification goes too.
            if case .deleted = m.message, !m.outgoing { notifier.clear(m.chat) }
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
        case let .fullImageRequested(chat, _, _):
            // It waits on our photo for the user's answer (§13.3): nothing goes before.
            fullRequests[chat] = core.fullImageRequests(chat: chat)
            #if DEBUG
            // -autoFull YES: sent at once, as before the user was asked (screenshots, tests).
            if UserDefaults.standard.bool(forKey: "autoFull") {
                let pending = fullRequests[chat] ?? []
                DispatchQueue.main.async { [weak self] in
                    for r in pending { _ = self?.answerFull(r.messageId, in: chat, send: true) }
                }
                break
            }
            #endif
            if !isMuted(chat), case let .contact(key) = chat {
                notifier.message(chat, title: title(of: chat),
                                 body: String(localized: "\(name(of: key)) asks for the full photo"), actions: false)
            }
        case let .searched(gid, by):
            // A member asks where we are and we answer (SOS or this group's consent, §11).
            let chat = ChatID.group(gid: gid)
            notifier.message(chat, title: title(of: chat),
                             body: String(localized: "\(name(of: by)) is locating you"), actions: false)
            searchedBy = core.searchedBy(nowMs: now())
        case .searchUpdated:
            searchVersion += 1
            refreshSearches()
        case let .txPower(dbm, previous):
            // lc-ffi keeps the power to restore; we show it while it is lowered.
            loweredPower = previous == nil ? nil : dbm
        case let .advert(flood, sent):
            // What the radio answered to the advert asked for in Settings (B-user-4); the core's
            // own adverts (after a card, a join) say nothing there.
            if case .sending = advert {
                advert = sent ? .sent(flood: flood, at: Date()) : .notSent(flood: flood)
            }
        case .nodeUpdated:
            // A trace or a server's answer (§16).
            meshToolsVersion += 1
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
            timers[chat] = (try? core.disappearing(chat: chat)) ?? 0
            chats[chat] = page
            olderAvailable[chat] = page.count == Int(Self.historyPage)
            historyError[chat] = nil
            fullRequests[chat] = core.fullImageRequests(chat: chat)
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

    /// The text typed in `chat` and not sent, kept across screens and restarts, for this
    /// identity (or the demo) only (recheck R07).
    func draft(_ chat: ChatID) -> String {
        drafts[draftKey(chat)] ?? ""
    }

    func setDraft(_ chat: ChatID, _ text: String) {
        let key = draftKey(chat)
        guard drafts[key] != (text.isEmpty ? nil : text) else { return }
        drafts[key] = text.isEmpty ? nil : text
        Self.saveDrafts(drafts)
    }

    private func draftKey(_ chat: ChatID) -> String {
        "\(isDemo ? "demo" : core.fingerprint().joined())|\(chat.storageKey)"
    }

    /// The message the draft of `chat` answers (§6.2), kept with the draft (ADR-0034).
    func replyDraft(_ chat: ChatID) -> Int64? {
        drafts[draftKey(chat) + "|reply"].flatMap { Int64($0) }
    }

    func setReplyDraft(_ chat: ChatID, _ id: Int64?) {
        let key = draftKey(chat) + "|reply"
        let value = id.map { String($0) }
        guard drafts[key] != value else { return }
        drafts[key] = value
        Self.saveDrafts(drafts)
    }

    /// A recording stopped because its conversation was left: offered there again, to listen
    /// to, send or discard (ADR-0034). In memory only, like the recording itself.
    struct VoiceDraft {
        let pcm: [Int16]
        let mode: UInt8
    }

    @ObservationIgnored var voiceDrafts: [ChatID: VoiceDraft] = [:]

    /// Conversation `chat` is left: a recording made for it stops and waits for it.
    func keepRecording(for chat: ChatID) {
        guard recorder.chat == chat else { return }
        guard recorder.recording else {
            // Still asking for the microphone: nothing recorded.
            recorder.cancel(for: chat)
            return
        }
        if let pcm = recorder.stop(keep: true) {
            let mode: UInt8 = if case .group = chat { Self.groupVoiceMode } else { UInt8(voiceMode) }
            voiceDrafts[chat] = VoiceDraft(pcm: pcm, mode: mode)
        }
    }

    /// Drafts by identity, in lichen's folder: like the history, out of backups (SR-22). They
    /// used to be in the preferences, which backups carry: moved once, then removed there.
    @ObservationIgnored private var drafts: [String: String] = AppModel.loadDrafts()
    private static let draftsDefault = "lichen.drafts.byIdentity"

    private static var draftsFile: URL? {
        dataDirectory().map { URL(fileURLWithPath: $0).appendingPathComponent("drafts.json") }
    }

    private static func loadDrafts() -> [String: String] {
        var all: [String: String] = [:]
        if let url = draftsFile, let data = try? Data(contentsOf: url),
           let saved = try? JSONDecoder().decode([String: String].self, from: data) {
            all = saved
        }
        if let old = UserDefaults.standard.dictionary(forKey: draftsDefault) as? [String: String] {
            all.merge(old) { kept, _ in kept }
            saveDrafts(all)
            UserDefaults.standard.removeObject(forKey: draftsDefault)
        }
        return all
    }

    private static func saveDrafts(_ all: [String: String]) {
        guard let url = draftsFile, let data = try? JSONEncoder().encode(all) else { return }
        try? data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    /// Makes message `id` (a search result, a quoted message) part of the loaded history: what
    /// lies between it and the oldest message shown is read at once (recheck R08). False when
    /// it is no longer on this phone.
    @discardableResult
    func loadUntil(_ chat: ChatID, _ id: Int64) -> Bool {
        if chats[chat] == nil { load(chat) }
        if chats[chat]?.contains(where: { $0.id == id }) ?? false { return true }
        guard let first = chats[chat]?.first?.id, id < first,
              let gap = try? core.historyFrom(chat: chat, fromId: id, beforeId: first),
              gap.first?.id == id
        else { return false }
        chats[chat] = gap + (chats[chat] ?? [])
        return true
    }

    // MARK: chat essentials (pins, mutes, search)

    func refreshPrefs() {
        prefs = Dictionary(core.chatPrefs().map { ($0.chat, $0) }, uniquingKeysWith: { a, _ in a })
        blocked = core.blocked()
    }

    // MARK: blocking and archiving (ADR-0034)

    /// People the user blocked, the latest first.
    var blocked: [BlockedContact] = []

    func isBlocked(_ node: Data) -> Bool { blocked.contains { $0.node == node } }

    /// The node behind a direct conversation, lichen or MeshCore.
    func node(of chat: ChatID) -> Data? {
        switch chat {
        case let .contact(key), let .mesh(key): key
        case .group, .channel, .room: nil
        }
    }

    /// Blocks contact `node`: local, they are not told. Returns an error to show, or nil.
    func block(_ node: Data) -> String? {
        do {
            try core.block(node: node, nowMs: now())
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not block.")
        }
        refreshPrefs()
        reloadGroupChats()
        pump()
        return nil
    }

    func unblock(_ node: Data) {
        try? core.unblock(node: node)
        refreshPrefs()
        reloadGroupChats()
    }

    /// A block hides (or shows again) a member's posts already in group histories.
    private func reloadGroupChats() {
        for chat in Array(chats.keys) {
            if case .group = chat { load(chat) }
        }
        conversations = core.conversations()
    }

    // MARK: sharing into lichen (ADR-0034)

    /// A file or a photo another app opened in lichen ("Open in lichen"), read, its copy gone,
    /// its name sanitised: it waits for the user to choose a conversation.
    struct SharedFile: Identifiable {
        let id = UUID()
        let name: String
        let data: Data
        let isImage: Bool
    }

    var incomingShare: SharedFile?
    /// What the user chose to send to a conversation: its composer takes it.
    var sharedFor: [ChatID: SharedFile] = [:]
    var shareError: String?

    /// A URL the system opened lichen with. Only a file is taken: lichen registers no URL
    /// scheme, and anything else is ignored. The system's copy (Documents/Inbox) is read once,
    /// at most 64 MB, and deleted.
    func receiveShared(_ url: URL) {
        guard url.isFileURL else { return }
        let inbox = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Inbox", isDirectory: true).standardizedFileURL.path
        defer {
            if let inbox, url.standardizedFileURL.path.hasPrefix(inbox + "/") {
                try? FileManager.default.removeItem(at: url)
            }
        }
        switch readPicked(url) {
        case let .success(draft):
            let safe = fitFileName(name: draft.name, maxBytes: 64)
            let name = safe.isEmpty ? "file" : safe
            let type = UTType(filenameExtension: (name as NSString).pathExtension)
            incomingShare = SharedFile(name: name, data: draft.data, isImage: type?.conforms(to: .image) ?? false)
        case let .failure(problem):
            shareError = problem.message
        }
    }

    /// Sends what was shared to `chat`: its conversation opens on the photo or file sheet.
    func share(_ shared: SharedFile, to chat: ChatID) {
        incomingShare = nil
        sharedFor[chat] = shared
        tab = .messages
        openRequest = chat
    }

    // MARK: media and storage (ADR-0034)

    /// What `chat` holds: photos, files and voice messages, the newest first.
    func chatMedia(_ chat: ChatID) -> [StoredMessage] {
        (try? core.chatMedia(chat: chat, beforeId: nil, limit: 2_000)) ?? []
    }

    /// What each conversation takes on this phone: its messages and the media kept for it,
    /// and the files downloaded over the bridge for it.
    func storageUsage() -> [(usage: ChatUsage, bytes: Int64)] {
        core.storageUsage().map { u in
            let downloaded = u.announced.compactMap { downloads[$0] }.reduce(Int64(0)) { sum, url in
                sum + ((try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value ?? 0)
            }
            return (u, Int64(clamping: u.bytes) + downloaded)
        }
    }

    /// Removes photos, files and voice messages older than `days` days from this phone, as
    /// "delete for me" does (their downloads go with them, SR-28). Returns how many.
    func clearMedia(olderThanDays days: Int) -> Int {
        let before = Int64((Date().timeIntervalSince1970 - Double(days) * 86_400) * 1000)
        let n = (try? core.clearMediaOlderThan(beforeMs: before)) ?? 0
        for chat in Array(chats.keys) { load(chat) }
        conversations = core.conversations()
        pump()
        return Int(n)
    }

    /// Archived: out of the list, under "Archived", until a message comes (unless muted).
    func isArchived(_ chat: ChatID) -> Bool { prefs[chat]?.archived ?? false }

    func setArchived(_ chat: ChatID, _ archived: Bool) {
        try? core.setArchived(chat: chat, archived: archived)
        refreshPrefs()
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
        setDraft(chat, "")
        setReplyDraft(chat, nil)
        voiceDrafts[chat] = nil
        chats[chat] = nil
        conversations = core.conversations()
    }

    /// A MeshCore advert (flood), so that radios around add ours as a contact.
    /// Our node's advert, zero-hop (`flood` false: radios in direct range) or flooded (repeaters
    /// pass it on): `advert` follows what the radio answers (protocol.md §1.1).
    func sendAdvert(flood: Bool) {
        advert = .sending(flood: flood)
        core.sendAdvert(flood: flood, nowMs: now())
        pump()
    }

    /// What sending `text` to `chat` costs us; for a group, our own floods (both copies).
    func estimate(_ chat: ChatID, text: String) -> Estimate? {
        guard !text.isEmpty else { return nil }
        switch chat {
        case let .contact(key): return try? core.estimateText(to: key, textLen: UInt32(text.utf8.count))
        case .group: return core.estimateGroupText(textLen: UInt32(text.utf8.count))
        case .mesh, .channel, .room: return try? core.estimatePlain(chat: chat, len: UInt32(text.utf8.count))
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
            case .mesh, .channel, .room:
                m = try core.sendPlain(chat: chat, text: text, sentAt: sentAt, nowMs: now())
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

    // MARK: stickers and GIFs (protocol.md §6.3)

    /// Sends a sticker; returns an error message to show, or nil.
    func sendSticker(_ emoji: String, to chat: ChatID) -> String? {
        sendMessage(chat) { try core.sendSticker(chat: chat, emoji: emoji, sentAt: $0, nowMs: $1) }
    }

    /// Sends GIPHY GIF `link` (only its id, shape and title travel).
    func sendGif(_ link: GifLink, size: (UInt16, UInt16)?, to chat: ChatID) -> String? {
        sendMessage(chat) {
            try core.sendGif(chat: chat, id: link.id, width: size?.0 ?? 0, height: size?.1 ?? 0,
                             title: link.title, sentAt: $0, nowMs: $1)
        }
    }

    private func sendMessage(_ chat: ChatID, _ send: (UInt32, UInt64) throws -> StoredMessage) -> String? {
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

    /// GIFs load by themselves: asked for, or through Tor.
    var gifsLoadThemselves: Bool { gifAutoLoad || torStatus == .ready }

    /// With Tor on, the map loads its tiles only once allowed, for this run of the app (SR-36).
    var mapWithTorAllowed = false

    /// With GIFs loading by themselves, a received GIF loads as it arrives, not when it is read:
    /// GIPHY's view counter then tells its sender no more than the delivery lichen acknowledges
    /// anyway (review of 2026-11-15, SR-36).
    private func prefetchGif(_ m: Message) {
        guard gifsLoadThemselves, case let .gif(_, provider, _, _, id, _) = m, provider == 1, cachedGif(id) == nil else { return }
        Task { _ = await loadGif(id) }
    }

    private static var gifFolder: URL? {
        guard let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first else { return nil }
        let dir = base.appendingPathComponent("gifs", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// A GIF loaded before (the system may clear the caches folder).
    func cachedGif(_ id: String) -> Data? {
        guard id.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber) }), let dir = Self.gifFolder else { return nil }
        return try? Data(contentsOf: dir.appendingPathComponent(id + ".gif"))
    }

    /// GIPHY GIF `id`, through Tor when Tor is on, directly otherwise; kept in the caches.
    func loadGif(_ id: String) async -> Data? {
        if let data = cachedGif(id) { return data }
        // With Tor on, through Tor only, also while it starts (then it fails, and loads once
        // Tor is up): GIPHY never sees the address (§6.3, review of 2026-11-15, SR-26).
        guard let bridge, let data = try? await bridge.gif(id: id, viaTor: torEnabled) else { return nil }
        if let dir = Self.gifFolder {
            try? data.write(to: dir.appendingPathComponent(id + ".gif"))
            Self.trimGifs(dir)
        }
        return data
    }

    /// The caches' GIFs within 50 MB, the oldest dropped first.
    private static func trimGifs(_ dir: URL) {
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey]
        guard let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: keys) else { return }
        let sized = files.compactMap { f -> (URL, Int, Date)? in
            guard let v = try? f.resourceValues(forKeys: Set(keys)) else { return nil }
            return (f, v.fileSize ?? 0, v.contentModificationDate ?? .distantPast)
        }.sorted { $0.2 > $1.2 }
        var total = 0
        for (f, size, _) in sized {
            total += size
            if total > 50 << 20 { try? FileManager.default.removeItem(at: f) }
        }
    }

    // MARK: media (protocol.md §13)

    func estimate(_ chat: ChatID, datagramLen: UInt32) -> Estimate? {
        guard case let .contact(key) = chat else { return nil }
        return try? core.estimateDatagram(to: key, len: datagramLen)
    }

    /// What a voice message of `len` bytes costs: on the path to a contact, or our own floods
    /// to a group (both copies, §13.1).
    func voiceEstimate(_ chat: ChatID, datagramLen: UInt32) -> Estimate? {
        if case .group = chat { return core.estimateGroupDatagram(len: datagramLen) }
        return estimate(chat, datagramLen: datagramLen)
    }

    /// A group's voice notes (ADR-0035): Codec2 700C, 6 seconds at most (5 fragments).
    static let groupVoiceMode: UInt8 = 0
    static let groupVoiceSeconds: TimeInterval = 6

    func sendVoice(_ voice: PreparedVoice, to chat: ChatID) -> String? {
        switch chat {
        case let .contact(key):
            return sendMedia(chat) { try core.sendVoice(to: key, voice: voice, sentAt: $0, nowMs: $1) }
        case let .group(gid):
            return sendMedia(chat) { try core.sendGroupVoice(gid: gid, voice: voice, sentAt: $0, nowMs: $1) }
        case .mesh, .channel, .room:
            return LichenError.MediaInGroup.display
        }
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
    ///
    /// A received name is only a label (recheck R01): every copy goes into a new folder of its
    /// own, nothing is ever removed or overwritten at a path derived from it, and the result must
    /// stay inside that folder. The copies are deleted at the next start.
    func openable(name: String, data: Data? = nil, file: URL? = nil) -> URL? {
        guard let root = Self.folder("open") else { return nil }
        let dir = root.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let safe = fitFileName(name: name, maxBytes: 120)
        let url = dir.appendingPathComponent(safe.isEmpty ? "file" : safe, isDirectory: false)
        guard url.standardizedFileURL.deletingLastPathComponent().path == dir.standardizedFileURL.path,
              (try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: false)) != nil
        else { return nil }
        if let data {
            return (try? data.write(to: url, options: .withoutOverwriting)) != nil ? url : nil
        }
        if let file {
            return (try? FileManager.default.copyItem(at: file, to: url)) != nil ? url : nil
        }
        return nil
    }

    /// Copies made to open or share files, from the previous run: nothing holds them any more.
    static func clearOpenCopies() {
        guard let base = dataDirectory() else { return }
        try? FileManager.default.removeItem(at: URL(fileURLWithPath: base).appendingPathComponent("open", isDirectory: true))
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("share", isDirectory: true)
        try? FileManager.default.removeItem(at: tmp)
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

    /// A contact's request for the full variant of our photo `message`, if one waits (§13.3).
    func fullRequest(for message: StoredMessage) -> FullImageRequest? {
        guard message.outgoing else { return nil }
        return fullRequests[message.chat]?.first { $0.messageId == message.id }
    }

    /// Answers a contact's request for the full variant of our photo (§13.3): `send` sends it,
    /// refused when it does not fit in the hour's airtime (the request then still waits); not
    /// now forgets it, and the contact gets no answer.
    func answerFull(_ messageId: Int64, in chat: ChatID, send: Bool) -> String? {
        defer { fullRequests[chat] = core.fullImageRequests(chat: chat) }
        do {
            try core.answerFullImageRequest(messageId: messageId, send: send, nowMs: now())
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

    /// Answers `target` with `text` (a text that waits for the radio like any other). A
    /// MeshCore message is answered by naming its sender first, `@[Name] ` (§15.9).
    func reply(_ text: String, to target: StoredMessage) -> String? {
        do {
            let sentAt = UInt32(Date().timeIntervalSince1970)
            let m = target.chat.isPlain
                ? try core.sendPlainReply(messageId: target.id, text: text, sentAt: sentAt, nowMs: now())
                : try core.sendReply(chat: target.chat, toMessageId: target.id, text: text, sentAt: sentAt, nowMs: now())
            upsert(m)
            conversations = core.conversations()
            pump()
            return nil
        } catch LichenError.NotReferable where target.chat.isPlain {
            return String(localized: "Can’t reply to this name.")
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// Whether a MeshCore message can be answered with a mention (§15.9): a text.
    func plainReplyable(_ m: StoredMessage) -> Bool {
        guard m.chat.isPlain, case .text = m.message else { return false }
        return true
    }

    /// The name a reply to MeshCore message `m` mentions (§15.9): a post's author, ours, or the
    /// name the node's radio advertises.
    func plainSender(_ m: StoredMessage) -> String {
        if let a = m.authorName { return a }
        if m.outgoing { return nodeName }
        if case let .mesh(key) = m.chat, let c = contacts.first(where: { $0.key == key }) { return c.nodeName }
        return title(of: m.chat)
    }

    /// A MeshCore text cut into spans: text, links, places, mentions (§15.9).
    func meshSpans(_ text: String) -> [TextSpan] {
        core.meshcoreSpans(text: text)
    }

    // MARK: editing and deleting (protocol.md §6.4)

    /// Changes the text of our `target`; returns an error message to show, or nil.
    func edit(_ text: String, of target: StoredMessage) -> String? {
        do {
            let m = try core.sendEdit(messageId: target.id, text: text, nowMs: now())
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

    /// Takes our message back on the phones that receive it.
    func deleteForEveryone(_ m: StoredMessage) -> String? {
        do {
            upsert(try core.sendDelete(messageId: m.id, nowMs: now()))
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// Forwards `m` to `chat` as a new message of ours (ADR-0027).
    func forward(_ m: StoredMessage, to chat: ChatID) -> String? {
        do {
            let sent = try core.forward(messageId: m.id, to: chat, sentAt: UInt32(Date().timeIntervalSince1970), nowMs: now())
            if chats[chat] == nil { load(chat) } else { upsert(sent) }
            conversations = core.conversations()
            pump()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not send.")
        }
    }

    /// Removes a message from this phone only.
    func deleteForMe(_ m: StoredMessage) -> String? {
        do {
            try core.deleteForMe(messageId: m.id)
            chats[m.chat]?.removeAll { $0.id == m.id }
            conversations = core.conversations()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return String(localized: "Could not delete.")
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
        invitations = core.groupInvitations()
        coreContacts = core.contacts()
        #if DEBUG
        if !debugOpened, let name = UserDefaults.standard.string(forKey: "open"),
           let g = groups.first(where: { $0.name == name }) {
            openRequest = .group(gid: g.gid)
            debugOpened = true
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

    /// Asks the admin of an invitation to add us; the group arrives as `groupJoined`. Only a
    /// `scanned` invitation verifies the admin (second read of 2026-11-17, N1).
    func acceptInvite(_ text: String, _ info: InviteInfo, scanned: Bool) -> String? {
        do {
            _ = try core.acceptInvite(text: text, scanned: scanned, nowMs: now())
            joining[info.admin] = info.groupName
            joinedByInvite[info.admin] = nil
            coreContacts = core.contacts()
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

    /// Hands a group we run to `member` (protocol.md §10.8): "Make admin", or, with `leave`,
    /// how the admin leaves without closing the group.
    func handOver(_ gid: Data, to member: Data, leave: Bool) -> String? {
        let e = groupCall { try core.handOver(gid: gid, member: member, leave: leave, nowMs: now()) }
        if leave { shares = core.shares() }
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

    /// A place written in a message (ADR-0034), on the map until dismissed.
    var mapPin: MapPin?

    /// Shows a place from a message's text on the map: lichen's own map, nothing fetched but
    /// its tiles (offline areas included).
    func showOnMap(lat: Double, lon: Double) {
        mapPin = MapPin(lat: lat, lon: lon, version: (mapPin?.version ?? 0) + 1)
        tab = .map
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
    ///
    /// iroh runs only while it is needed: something we offer is still served (7 days), a send
    /// sheet is open, or a download goes through it. Otherwise its relays would see the phone's
    /// address and its lasting endpoint id at every launch, and anyone who once got an
    /// announcement could see whether it is online (review of 2026-11-15, SR-29).
    private func applyIroh(restart: Bool = false) {
        guard let bridge else { return }
        let wanted = irohEnabled && (bridgeDemand > 0 || bridge.hasOffers())
        let relayOnly = relayOnly
        Task {
            if !wanted || restart {
                await bridge.stopIroh()
                bridgeOnline = false
            }
            guard wanted else { return }
            if bridge.endpointId() == nil { _ = try? await bridge.startIroh(relayOnly: relayOnly) }
            bridgeOnline = await bridge.online(limitS: 30)
        }
    }

    /// Who needs iroh now: open send sheets and downloads through it.
    @ObservationIgnored private var bridgeDemand = 0

    /// A send sheet opened: iroh starts, so that it can be offered.
    func needBridge() {
        bridgeDemand += 1
        applyIroh()
    }

    /// The sheet closed: iroh stops unless something is still offered.
    func releaseBridge() {
        bridgeDemand = max(0, bridgeDemand - 1)
        applyIroh()
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

    /// Where content of `hash` named `name` is kept: `downloads/<hash>.<extension>`.
    private func downloadURL(hash: Data, name: String) -> URL? {
        let ext = (name as NSString).pathExtension
        return Self.folder("downloads")?.appendingPathComponent(hash.hex)
            .appendingPathExtension(ext.isEmpty ? "bin" : ext)
    }

    private func keep(_ bytes: Data, hash: Data, name: String) -> URL? {
        guard let url = downloadURL(hash: hash, name: name) else { return nil }
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
        // iroh starts for the download when it is not running (SR-29).
        let iroh = irohEnabled ? providers.first { $0.transport == 1 } : nil
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
            bridgeDemand += 1
            defer { releaseBridge() }
            for p in route {
                fetching[hash] = p.transport
                do {
                    if p.transport == 1, bridge.endpointId() == nil {
                        _ = try await bridge.startIroh(relayOnly: relayOnly)
                        bridgeOnline = await bridge.online(limitS: 30)
                    }
                    if p.transport == 2 {
                        let (state, cache) = Self.torDirs
                        _ = try await bridge.startTor(stateDir: state, cacheDir: cache, publish: false, limitS: 180)
                    }
                    guard let url = downloadURL(hash: hash, name: name) else { break }
                    // Straight to the file, verified before it appears: the content never sits
                    // in memory or crosses the bindings (review of 2026-11-15, SR-33).
                    _ = try await bridge.fetchToFile(transport: p.transport, provider: p.key, hash: hash,
                                                     size: UInt64(size), path: url.path)
                    // Cancelled meanwhile: dropped.
                    if Task.isCancelled {
                        try? FileManager.default.removeItem(at: url)
                        return
                    }
                    try? FileManager.default.setAttributes(
                        [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: url.path)
                    downloads[hash] = url
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

    /// One of our own codes (our card, an invitation to a group of ours), scanned or pasted
    /// back (D-3).
    func isOwnCode(_ text: String) -> Bool {
        core.ownCode(text: text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    /// Their chat asks to compare safety numbers (B-user-1): their key is known and not
    /// verified, no new key waits, and the user did not say Later. On both phones, whoever
    /// scanned whom: the one who did not scan learns the other's key from a message.
    func asksToCompare(_ node: Data) -> Bool {
        guard let c = contact(node), c.identity != nil, !c.verified, conflicts[node] == nil else { return false }
        return !compareLater.contains(node.hex)
    }

    /// Later: their chat stops asking to compare.
    func compareLaterWith(_ node: Data) {
        compareLater.insert(node.hex)
    }

    /// Adds the contact of a card: verified when scanned in person, not when pasted (§4.7).
    /// A MeshCore contact link (F-2, ADR-0047): the signed advert the stock app shares, or a
    /// `meshcore://contact/add?…` code. `BadSignature` when the advert was changed or made up.
    func readMeshCoreLink(_ text: String) -> Result<MeshCoreLink, LichenError> {
        do {
            return .success(try readMeshcoreLink(text: text.trimmingCharacters(in: .whitespacesAndNewlines)))
        } catch let e as LichenError {
            return .failure(e)
        } catch {
            return .failure(.BadCode)
        }
    }

    /// Sends a link's node to the radio (§16.6): it then comes among the contacts or the nodes,
    /// or the radio's refusal bumps `linkRefusals`. `NoIdentity` without a radio.
    func addMeshCoreLink(_ text: String) -> Result<MeshCoreAdd, LichenError> {
        do {
            let r = try core.addMeshcoreContact(text: text.trimmingCharacters(in: .whitespacesAndNewlines))
            pump()
            return .success(r)
        } catch let e as LichenError {
            return .failure(e)
        } catch {
            return .failure(.BadCode)
        }
    }

    func addCard(_ text: String, scanned: Bool) -> Result<ContactInfo, LichenError> {
        do {
            let c = try core.addContactCard(card: text, scanned: scanned)
            coreContacts = core.contacts()
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
        coreContacts = core.contacts()
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
        coreContacts = core.contacts()
        return done
    }

    /// Accepts the new identity `expected` the user compared, verified; false when another
    /// one arrived meanwhile (compare again).
    func acceptIdentity(_ node: Data, expected: Data) -> Bool {
        let done = (try? core.acceptIdentity(node: node, expected: expected, verify: true)) ?? false
        if done { conflicts[node] = nil } else { conflicts[node] = (try? core.pendingIdentity(node: node)) ?? nil }
        coreContacts = core.contacts()
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

    // MARK: encrypted backup (ADR-0037, docs/spec/backup.md)

    enum BackupOutcome {
        case made(URL, BackupSummary)
        case failed(String)
    }

    /// The settings a backup carries (backup.md §4), under the names both apps use.
    private func backupSettings() -> Data {
        var s: [String: Any] = [
            "lichen.myName": myName,
            "lichen.tor": torEnabled,
            "lichen.iroh": irohEnabled,
            "lichen.relayOnly": relayOnly,
            "lichen.gifAuto": gifAutoLoad,
            "lichen.voiceMode": voiceMode,
        ]
        let d = UserDefaults.standard
        s[radioDetailsKey] = d.bool(forKey: radioDetailsKey)
        if let a = d.string(forKey: "appearance") { s["appearance"] = a }
        if let m = d.string(forKey: "map.style") { s["map.style"] = m }
        return (try? JSONSerialization.data(withJSONObject: s, options: [.sortedKeys])) ?? Data()
    }

    /// The settings of a restored backup: the keys this app knows, with values of the right type.
    private func applyBackupSettings(_ data: Data) {
        guard !data.isEmpty, let s = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return }
        if let v = s["lichen.myName"] as? String { myName = v }
        if let v = s["lichen.tor"] as? Bool { torEnabled = v }
        if let v = s["lichen.iroh"] as? Bool { irohEnabled = v }
        if let v = s["lichen.relayOnly"] as? Bool { relayOnly = v }
        if let v = s["lichen.gifAuto"] as? Bool { gifAutoLoad = v }
        if let v = s["lichen.voiceMode"] as? Int, [0, 2, 3].contains(v) { voiceMode = v }
        if let v = s[radioDetailsKey] as? Bool { UserDefaults.standard.set(v, forKey: radioDetailsKey) }
        // Android writes these in capitals.
        if let v = (s["appearance"] as? String)?.lowercased(), Appearance(rawValue: v) != nil {
            UserDefaults.standard.set(v, forKey: "appearance")
        }
        if let v = (s["map.style"] as? String)?.lowercased(), MapStyle(rawValue: v) != nil {
            UserDefaults.standard.set(v, forKey: "map.style")
        }
    }

    /// Writes a backup of this phone's data into a temporary file, for the user to save or send.
    /// Runs off the main thread; the app keeps working meanwhile.
    func exportBackup() async -> BackupOutcome {
        guard !isDemo, identityProblem == nil else { return .failed(LichenError.NotAllowed.display) }
        let day = Date().formatted(.iso8601.year().month().day())
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("lichen-\(day).lichen-backup")
        try? FileManager.default.removeItem(at: url)
        let (core, settings) = (core, backupSettings())
        do {
            let made = try await Task.detached(priority: .userInitiated) {
                try core.exportBackup(path: url.path, settings: settings)
            }.value
            return .made(url, made)
        } catch LichenError.NotAllowed {
            return .failed(String(localized: "There is nothing to back up yet."))
        } catch let e as LichenError {
            return .failed(e.display)
        } catch {
            return .failed(LichenError.Storage.display)
        }
    }

    /// Restores a backup with its 24 words: its identity, messages, contacts, groups, settings
    /// and media replace this phone's. Nil once done, otherwise why not (nothing was changed).
    func restoreBackup(from url: URL, words: [String]) async -> String? {
        guard let dir = Self.dataDirectory() else { return LichenError.Storage.display }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let core = core
        do {
            let r = try await Task.detached(priority: .userInitiated) {
                try core.restoreBackup(path: url.path, words: words, dataDir: dir)
            }.value
            do {
                try Keychain.replaceIdentitySeed(r.identitySeed)
            } catch {
                // The data is in place, but the identity is not saved: the app goes on with its
                // own, its store open again.
                if let identitySeed { useIdentity(identitySeed) }
                return String(localized: "The identity could not be saved on this phone. Try again.")
            }
            phraseBackedUp = true
            useIdentity(r.identitySeed)
            applyBackupSettings(r.settings)
            loadDownloads()
            return nil
        } catch let e as LichenError {
            return e.display
        } catch {
            return LichenError.Storage.display
        }
    }

    // MARK: lookups for the views

    func name(of key: Data) -> String {
        if key == nodeKey { return String(localized: "You") }
        return contacts.first { $0.key == key }?.name ?? key.prefix(4).hex
    }

    func title(of chat: ChatID) -> String {
        switch chat {
        case let .contact(key), let .mesh(key):
            if key == nodeKey || contacts.contains(where: { $0.key == key }) { return name(of: key) }
            return conversationTitle(chat, named: storedName(chat))
        case let .group(gid): return groups.first { $0.gid == gid }?.name ?? conversationTitle(chat, named: storedName(chat))
        case let .channel(id): return channels.first { $0.id == id }?.name ?? conversationTitle(chat, named: storedName(chat))
        case let .room(key): return node(key)?.name ?? conversationTitle(chat, named: storedName(chat))
        }
    }

    /// The name the core gives a conversation that has messages (§15.8); empty when unknown.
    private func storedName(_ chat: ChatID) -> String {
        conversations.first { $0.chat == chat }?.name ?? ""
    }

    /// A conversation's title from the name the core gives it: a node nothing names is "Unknown
    /// radio" and the start of its key.
    func conversationTitle(_ chat: ChatID, named name: String) -> String {
        if !name.isEmpty { return name }
        switch chat {
        case let .contact(key), let .mesh(key), let .room(key):
            return String(localized: "Unknown radio · \(key.prefix(3).hex.uppercased())")
        case .group: return String(localized: "Group")
        case .channel: return String(localized: "Channel")
        }
    }

    // MARK: MeshCore chats (protocol.md §15)

    /// The longest text for a plain chat, in bytes.
    func plainTextMax(_ chat: ChatID) -> Int {
        Int((try? core.plainTextMax(chat: chat)) ?? 160)
    }

    /// Joins Public, a hashtag, or a private channel (`secret`: 32 hex digits); returns the
    /// channel, or an error message.
    func joinChannel(_ kind: ChannelKind, name: String = "", secret: String = "") -> Result<ChatID, ChannelError> {
        do {
            let chat: ChatID = switch kind {
            case .publicChannel: try core.joinPublicChannel()
            case .hashtagChannel: try core.joinHashtagChannel(tag: name)
            case .privateChannel: try core.joinPrivateChannel(name: name, secretHex: secret)
            }
            channels = core.channels()
            refreshPrefs()
            pump()
            return .success(chat)
        } catch LichenError.BadCode {
            return .failure(ChannelError(message: kind == .hashtagChannel
                ? String(localized: "A hashtag is letters, digits and dashes.")
                : String(localized: "A channel secret is 32 hexadecimal digits.")))
        } catch LichenError.NoIdentity {
            return .failure(ChannelError(message: String(localized: "Connect the radio first: channels live on it.")))
        } catch LichenError.TooLarge {
            return .failure(ChannelError(message: String(localized: "The radio has no free channel slot.")))
        } catch {
            return .failure(ChannelError(message: String(localized: "Could not join the channel.")))
        }
    }

    struct ChannelError: Error { let message: String }

    func leaveChannel(_ id: Data) {
        try? core.leaveChannel(id: id)
        channels = core.channels()
        pump()
    }

    /// Our node's name as MeshCore users see it (§15.4).
    var meshName: String { core.meshName() }

    func setMeshName(_ name: String) -> String? {
        do {
            try core.setMeshName(name: name, nowMs: now())
            nodeName = core.meshName()
            pump()
            return nil
        } catch {
            return String(localized: "A name is 1 to 31 bytes.")
        }
    }

    var totalUnread: Int { conversations.reduce(0) { $0 + Int($1.unread) } }
}

/// A place from a message's text, shown on the map; `version` tells a new request apart.
struct MapPin: Equatable {
    let lat: Double
    let lon: Double
    let version: Int

    /// "45.12340, 5.67890".
    var label: String { String(format: "%.5f, %.5f", lat, lon) }
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

    /// One line for the conversation list ("Camille: …" in groups and channels).
    @MainActor func preview(_ model: AppModel) -> String {
        // A MeshCore text's mentions as "@name" (§15.9).
        let text = chat.isPlain ? meshcoreDisplayText(text: message.display) : message.display
        if let authorName, !outgoing { return "\(authorName): \(text)" }
        if let author, !outgoing, !chat.isPlain { return "\(model.name(of: author)): \(text)" }
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
        case let .sticker(_, _, emoji): return String(localized: "Sticker \(emoji)")
        case .deleted: return String(localized: "Message deleted")
        case let .gif(_, _, _, _, _, title) where !title.isEmpty: return String(localized: "GIF: \(title)")
        case .gif: return String(localized: "GIF")
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
        case 11:
            let seconds = UInt32(text) ?? 0
            if seconds == 0 {
                return mine ? String(localized: "You turned off disappearing messages")
                    : String(localized: "\(who) turned off disappearing messages")
            }
            let after = timerLabel(seconds)
            return mine ? String(localized: "You set messages to disappear after \(after)")
                : String(localized: "\(who) set messages to disappear after \(after)")
        // The name shown before (SR-30): a verified contact keeps it, the line says what changed.
        case 12: return String(localized: "\(actorName)’s radio is now called “\(text)”")
        // A handover (§10.8): the former admin acts, the successor is the subject.
        case 40:
            if mine { return String(localized: "You made \(names) admin") }
            if let me, subjects.first?.node == me { return String(localized: "\(who) made you admin") }
            return String(localized: "\(who) made \(names) admin")
        default: return String(localized: "The group changed")
        }
    }

    var date: Date {
        switch self {
        case let .text(sentAt, _), let .voice(sentAt, _, _, _, _), let .image(sentAt, _, _, _, _, _, _),
             let .announce(sentAt, _, _, _, _, _, _), let .file(sentAt, _, _), let .sticker(sentAt, _, _),
             let .gif(sentAt, _, _, _, _, _), let .deleted(sentAt):
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
            return String(localized: "Photos and files go to contacts only: in a group they would have to be flooded. Short voice messages can go to a group.")
        case .BadImage: return String(localized: "This image could not be read.")
        case .NotAPreview: return String(localized: "There is no full version to ask for.")
        case .NotSearching: return String(localized: "You are not locating this person.")
        case let .TooSoon(waitS): return String(localized: "Possible again in \(waitS) s.")
        case .NoPosition: return String(localized: "Your position is not known yet.")
        case .Storage: return String(localized: "The phone’s storage could not be read or written.")
        case .NotCancellable: return String(localized: "Part of it already went out: it can no longer be cancelled.")
        case .NotReferable: return String(localized: "This message is from before replies existed: it cannot be answered.")
        case .NoClock: return String(localized: "The phone’s clock is not set yet.")
        case .Expired: return String(localized: "This invitation has expired: ask for a new one.")
        case .InvalidSetting: return String(localized: "The radio does not take this setting.")
        case .NotAnExport: return String(localized: "This file is not a database exported by the MeshCore app.")
        case .IdentityPending: return String(localized: "This code names another identity than the one this contact has. Compare safety numbers with them before accepting the change.")
        case .NotABackup: return String(localized: "This file is not a lichen backup.")
        case .NewerBackup: return String(localized: "This backup was made by a newer version of lichen: update the app first.")
        case .WrongBackupWords: return String(localized: "These 24 words are not the ones of this backup.")
        case .BackupDamaged: return String(localized: "This backup is damaged or incomplete: nothing was restored.")
        case .NoPath: return String(localized: "No repeater on the way: the radio hears it directly, or does not know a path yet.")
        case .Blocked: return String(localized: "You blocked this contact: unblock them first.")
        case .BadSignature: return String(localized: "This link was changed or made up: its signature doesn’t match.")
        }
    }
}
