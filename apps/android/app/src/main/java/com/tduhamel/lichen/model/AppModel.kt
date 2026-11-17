package com.tduhamel.lichen.model

import android.app.Application
import android.content.Context
import android.hardware.usb.UsbDevice
import android.os.SystemClock
import androidx.core.content.edit
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.InviteInfo
import com.tduhamel.lichen.core.BlockedContact
import com.tduhamel.lichen.core.ChatUsage
import com.tduhamel.lichen.core.ChannelKind
import com.tduhamel.lichen.core.MeshChannel
import com.tduhamel.lichen.core.NodeInfo
import com.tduhamel.lichen.core.SavedRadio
import com.tduhamel.lichen.core.RadioSettings
import com.tduhamel.lichen.core.InviteCode
import com.tduhamel.lichen.core.BackupSummary
import com.tduhamel.lichen.core.Budget
import com.tduhamel.lichen.core.RestoredBackup
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.core.Conversation
import com.tduhamel.lichen.core.DemoNetwork
import com.tduhamel.lichen.core.Estimate
import com.tduhamel.lichen.core.LoraFile
import com.tduhamel.lichen.core.GifLink
import com.tduhamel.lichen.ui.clearOldShares
import com.tduhamel.lichen.core.GroupInfo
import com.tduhamel.lichen.core.GroupInvitation
import com.tduhamel.lichen.core.LichenCore
import com.tduhamel.lichen.core.LichenEvent
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.Radio
import com.tduhamel.lichen.core.ChatPrefs
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.core.BridgeException
import com.tduhamel.lichen.core.LichenBridge
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.PreparedImage
import com.tduhamel.lichen.core.PreparedVoice
import com.tduhamel.lichen.core.Provider
import com.tduhamel.lichen.core.prepareVoice
import com.tduhamel.lichen.core.LocationFix
import com.tduhamel.lichen.core.PeerPosition
import com.tduhamel.lichen.core.SearchState
import com.tduhamel.lichen.core.SearchedBy
import com.tduhamel.lichen.core.ShareInfo
import com.tduhamel.lichen.core.SharePrecision
import com.tduhamel.lichen.core.SosStatus
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sqrt
import kotlin.math.PI
import kotlin.math.sin
import com.tduhamel.lichen.core.seedFromPhrase
import com.tduhamel.lichen.core.databaseEncryption
import com.tduhamel.lichen.core.fitFileName
import java.io.File
import org.json.JSONObject
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The app's appearance setting: the system's by default. */
enum class Appearance { SYSTEM, DARK, LIGHT }

/** A place from a message's text, shown on the map; `version` tells a new request apart. */
data class MapPin(val lat: Double, val lon: Double, val version: Int) {
    /** "45.12340, 5.67890". */
    val label: String get() = String.format(java.util.Locale.ROOT, "%.5f, %.5f", lat, lon)
}

/** The app side of the lichen core: runs it (frames in and out, timer), keeps the UI state.
 * The Kotlin counterpart of the iOS app's `AppModel`; everything runs on the main thread. */
class AppModel(val app: Application) {
    enum class Tab { MESSAGES, MAP, SEARCH, SETTINGS }

    /** The link to the radio, as the radio card shows it. */
    sealed interface RadioState {
        data object Off : RadioState
        data object Idle : RadioState
        data object Scanning : RadioState
        data class Connecting(val name: String) : RadioState
        data class Connected(val name: String) : RadioState
    }

    /** Where the core's companion frames go: the radio over BLE or USB. */
    fun interface RadioLink {
        fun write(frame: ByteArray)
    }

    /** Debug launch extras (debug builds only), as the iOS launch arguments. */
    data class DebugOptions(
        val demo: Boolean = false,
        val open: String? = null,
        val say: String? = null,
        /** Answer the first MeshCore direct message with this (§15). */
        val sayPlain: String? = null,
        val tab: String? = null,
        val screen: String? = null,
        /** A companion to reach over TCP at launch, e.g. `10.0.2.2:5000` (`lichen bridge`). */
        val tcp: String? = null,
        /** A voice message to Camille; full images requested and announcements fetched. */
        val media: Boolean = false,
        /** Share our position (100 m) with this contact at launch. */
        val share: String? = null,
        /** An SOS from Julien (screenshots of the alert and the Search tab). */
        val sosDemo: Boolean = false,
        /** The welcome screens: shown (true), skipped (false), or as saved (null). */
        val onboarding: Boolean? = null,
        /** Camille in SOS too, for five minutes (two incidents at once). */
        val sosDemo2: Boolean = false,
        /** That many older messages with Camille in the demo (paging checks). */
        val fill: Int = 0,
        /** The identity failure screen: `unavailable`, `corrupt` or `notsaved`. */
        val identityProblem: String? = null,
        /** The demo radio drops after that many seconds, with texts waiting (§6.1). */
        val offline: Int = 0,
        /** And comes back after that many seconds. */
        val online: Int = 0,
        /** Texts expire after that many seconds instead of 24 h. */
        val expiry: Int = 0,
        /** Léa, next to you on the demo network, scans the group invitation you show. */
        val inviteDemo: Boolean = false,
        /** A scanned invitation is joined at once, as if "Join" were tapped. */
        val joinNow: Boolean = false,
        /** The chat opened with `open` gets this disappearing-message timer, seconds (§6.5). */
        val timer: Int = 0,
    )

    /** Why our identity cannot be used: [IdentityProblemScreen] shows instead of the app. */
    enum class IdentityProblem {
        /** The Keystore cannot be used now: nothing was changed, try again. */
        UNAVAILABLE,

        /** What is stored is not an identity. */
        CORRUPT,

        /** A first identity could not be saved. */
        NOT_SAVED,

        /** The message database cannot be opened: nothing could be sent or kept (recheck R02). */
        STORAGE,
    }

    enum class TorStatus { OFF, CONNECTING, PUBLISHING, READY, FAILED }

    private val res get() = app.resources
    private val prefs = app.getSharedPreferences("lichen", Context.MODE_PRIVATE)

    /** The app lock (Settings › Privacy): app only, the radio goes on behind it. */
    val lock = AppLock(prefs)
    private val identityStore = IdentityStore(app)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val started = SystemClock.elapsedRealtime()

    var radioState by mutableStateOf<RadioState>(RadioState.Idle)
    var nodeName by mutableStateOf("")
    var nodeKey by mutableStateOf<ByteArray?>(null)
    var radio by mutableStateOf<Radio?>(null)
    var budget by mutableStateOf(Budget(0uL, 360_000uL))

    /** New identities presented by verified contacts, waiting for the user (node hex → identity). */
    var conflicts by mutableStateOf(mapOf<String, ByteArray>())
    var contacts by mutableStateOf(listOf<ContactInfo>())

    /** MeshCore nodes the radio knows or heard (protocol.md §16.1). */
    var nodes by mutableStateOf(listOf<NodeInfo>())

    /** Bumped when a trace or a server's answer changes (§16): screens read them again. */
    var meshToolsVersion by mutableStateOf(0)

    /** Radios this phone was paired with (ADR-0042). */
    var savedRadios by mutableStateOf(listOf<SavedRadio>())
    /** What the radio reported of its settings (protocol §1.1), read again after each change. */
    var radioSettings by mutableStateOf<RadioSettings?>(null)
    /** Why the radio did not take a setting, for a dialog. */
    var radioError by mutableStateOf<String?>(null)

    /** Changes one of the radio's settings (ADR-0032); a refusal shows as [radioError]. */
    fun radioSet(change: () -> Unit) {
        try {
            change()
        } catch (e: LichenException) {
            radioError = e.display(app.resources)
        }
    }
    var groups by mutableStateOf(listOf<GroupInfo>())

    /** Groups that list us and wait for an answer (§10.1): someone not verified in person
     * cannot put us in a group. */
    var invitations by mutableStateOf(listOf<GroupInvitation>())

    /** Contacts whose phone speaks a newer protocol version (ADR-0029), by hex node key. */
    var newerPeers by mutableStateOf(setOf<String>())

    /** Each open conversation's disappearing-message timer, seconds; 0: off (§6.5). */
    val timers = mutableStateMapOf<ChatId, UInt>()

    /**
     * Messages of `chat` stored from now on disappear after `seconds` (0: they stay). A lichen
     * conversation gets the same setting; a MeshCore chat keeps it on this phone.
     */
    fun setTimer(chat: ChatId, seconds: UInt): String? = try {
        core.setDisappearing(chat.ref, seconds)
        timers[chat] = seconds
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Joins the group of an invitation; it then appears with the others. */
    fun acceptGroup(gid: ByteArray) {
        runCatching { core.acceptGroup(gid, now()) }
        groups = core.groups()
        invitations = core.groupInvitations()
        contacts = people()
        pump()
    }

    /** Forgets an invitation; its admin is not told. */
    fun declineGroup(gid: ByteArray) {
        runCatching { core.declineGroup(gid) }
        invitations = core.groupInvitations()
    }
    /** The radio's MeshCore channels (protocol.md §15.3). */
    var channels by mutableStateOf(listOf<MeshChannel>())

    /** Loaded histories, by conversation (filled when a chat is opened). */
    val chats = mutableStateMapOf<ChatId, List<StoredMessage>>()

    /** Conversations with messages, most recent first, with their unread counts. */
    var conversations by mutableStateOf(listOf<Conversation>())

    /** Contacts with a session (node hex). */
    var secured by mutableStateOf(setOf<String>())
    var lastSnr by mutableStateOf(mapOf<String, Float>())

    /** Running on the simulated network instead of a real radio. */
    var isDemo by mutableStateOf(false)

    /** The demo's simulated radio is switched off (Settings), to see messages wait (§6.1). */
    var demoRadioOff by mutableStateOf(false)
        private set
    var tab by mutableStateOf(Tab.MESSAGES)

    /** A chat to open (debug extra `open`, notifications later). */
    var openChat by mutableStateOf<ChatId?>(null)

    /** Invitations we accepted, by admin node (hex): the group's name, until it arrives (§10.5). */
    var joining by mutableStateOf(mapOf<String, String>())
        private set

    /** Groups that arrived after we accepted their invitation, by admin node (hex). */
    var joinedByInvite by mutableStateOf(mapOf<String, ByteArray>())
        private set

    /** An invitation to scan at launch (debug `screen join`: Camille's, on the demo). */
    var debugInvite by mutableStateOf<String?>(null)
    private var debugInviteMade = false

    /** Pinned and muted conversations (local, per identity). */
    var chatPrefs by mutableStateOf(mapOf<ChatId, ChatPrefs>())
        private set

    /** A message to open its chat on (from a search): taken by that chat's screen. */
    var focus by mutableStateOf<Pair<ChatId, Long>?>(null)
    var debugScreen by mutableStateOf<String?>(null)

    var appearance by mutableStateOf(
        runCatching { Appearance.valueOf(prefs.getString(APPEARANCE, null) ?: "") }.getOrDefault(Appearance.SYSTEM),
    )
        private set

    /** The name on our contact card (the node's name by default). */
    var myName by mutableStateOf(prefs.getString(MY_NAME, null) ?: "")
        private set

    /** The welcome screens were seen (first launch). */
    var onboarded by mutableStateOf(prefs.getBoolean(ONBOARDED, false))
        private set

    fun markOnboarded(done: Boolean) {
        onboarded = done
        prefs.edit { putBoolean(ONBOARDED, done) }
    }

    /** The recovery phrase was written down (Identity screen). */
    var phraseBackedUp by mutableStateOf(prefs.getBoolean(BACKED_UP, false))
        private set

    // Location (protocol.md §12).
    var shares by mutableStateOf(listOf<ShareInfo>())
    var positions by mutableStateOf(listOf<PeerPosition>())
    var sos by mutableStateOf(false)
        private set
    var myFix by mutableStateOf<LocationFix?>(null)
        private set

    /** The location permission is not granted. */
    var locationDenied by mutableStateOf(false)

    /** Only the approximate location is allowed (about 2 km): exact shares send it as it is. */
    var locationApproximate by mutableStateOf(false)

    /** The phone's location services are off. */
    var locationServicesOff by mutableStateOf(false)

    /** SOS incidents heard, by sender (hex): one alert each (Search.kt). */
    var incidents by mutableStateOf(mapOf<String, SosIncident>())

    /** The sender (hex) whose full-screen SOS alert is up, until dismissed. */
    var sosAlert by mutableStateOf<String?>(null)

    /** The person (hex) the Search tab shows; `openSearch` asks the tab to open on them. */
    var searchTarget by mutableStateOf<String?>(null)
    var openSearch by mutableStateOf<String?>(null)

    /** A place written in a message (ADR-0034), on the map until dismissed; `openMap` asks for
     * the Map tab. */
    var mapPin by mutableStateOf<MapPin?>(null)
    var openMap by mutableStateOf(false)

    /** Shows a place from a message's text on lichen's own map: nothing is fetched but its
     * tiles (offline areas included). */
    fun showOnMap(lat: Double, lon: Double) {
        mapPin = MapPin(lat, lon, (mapPin?.version ?: 0) + 1)
        tab = Tab.MAP
        openMap = true
    }

    /** Searches we run (protocol.md §11); `searchVersion` changes with their estimates. */
    var searches by mutableStateOf(listOf<SearchState>())
    var searchVersion by mutableStateOf(0)

    /** Members searching for us, and our radio's power while a search holds it lowered. */
    var searchedBy by mutableStateOf(listOf<SearchedBy>())
    var loweredPower by mutableStateOf<Byte?>(null)

    /** What our SOS is doing. */
    var sosStatus by mutableStateOf<SosStatus?>(null)

    /** The compass heading (degrees from true north) while the Search tab is on screen. */
    var heading by mutableStateOf<Double?>(null)
    private val compass = Compass(app) { heading = it }
    internal var lastBatteryReport = 0L

    /** Screens that need our position while shown (map, search, SOS alert). */
    private val locationUsers = mutableSetOf<String>()

    fun needsLocation(who: String, on: Boolean) {
        val changed = if (on) locationUsers.add(who) else locationUsers.remove(who)
        if (changed) updateLocator()
    }

    /** The compass, while the Search tab is on screen. */
    fun useHeading(on: Boolean) {
        if (on && compass.available) {
            compass.start()
        } else {
            compass.stop()
            heading = null
        }
    }
    private val locator = Locator(app) { gotFix(it) }
    private var lastFed: ULong? = null
    private var demoFixJob: Job? = null

    // Media (protocol.md §13).
    val recorder = VoiceRecorder()
    val player = VoicePlayer()

    /** Image previews whose full variant we asked for (message ids). */
    var requestedFull by mutableStateOf(setOf<Long>())

    // Internet bridge (ADR-0015).
    /** iroh reachable (a relay answered). */
    var bridgeOnline by mutableStateOf(false)
    var irohEnabled by mutableStateOf(prefs.getBoolean(IROH, true))
        private set
    var relayOnly by mutableStateOf(prefs.getBoolean(RELAY_ONLY, false))
        private set
    var torEnabled by mutableStateOf(prefs.getBoolean(TOR, false))
        private set
    var torStatus by mutableStateOf(TorStatus.OFF)

    /** Received GIFs load by themselves, without Tor too: GIPHY then sees our IP address
     * (ADR-0025). With Tor ready they always do, through it. */
    var gifAutoLoad by mutableStateOf(prefs.getBoolean(GIF_AUTO, false))
        private set

    fun chooseGifAutoLoad(on: Boolean) {
        gifAutoLoad = on
        prefs.edit { putBoolean(GIF_AUTO, on) }
    }

    /** GIFs load by themselves: asked for, or through Tor. */
    val gifsLoadThemselves: Boolean get() = gifAutoLoad || torStatus == TorStatus.READY

    /** With Tor on, the map loads its tiles only once allowed, for this run of the app (SR-36). */
    var mapWithTorAllowed by mutableStateOf(false)

    /**
     * With GIFs loading by themselves, a received GIF loads as it arrives, not when it is read:
     * GIPHY's view counter then tells its sender no more than the delivery lichen acknowledges
     * anyway (review of 2026-11-15, SR-36).
     */
    private fun prefetchGif(m: Message) {
        val gif = m as? Message.Gif ?: return
        if (!gifsLoadThemselves || gif.provider.toInt() != 1 || cachedGif(gif.id) != null) return
        scope.launch { loadGif(gif.id) }
    }

    /** Content fetched or offered over the bridge, by BLAKE3 hash (hex). */
    val downloads = mutableStateMapOf<String, File>()

    /** Downloads in progress: the transport each one uses (1 iroh, 2 Tor). */
    val fetching = mutableStateMapOf<String, Int>()
    val fetchErrors = mutableStateMapOf<String, String>()

    /** Downloads in progress, to cancel them. */
    private val fetchJobs = mutableMapOf<String, Job>()
    private var bridge: LichenBridge? = null
    private var onionId: ByteArray? = null

    /** Set while our identity cannot be used: the core is then a throwaway one that stores
     * nothing and never connects. */
    var identityProblem by mutableStateOf<IdentityProblem?>(null)
        private set
    private var identitySeed: ByteArray? = null

    var core: LichenCore = identityCore()
        private set
    var link: RadioLink? = null

    /** Radios found by the Bluetooth scan, strongest first. */
    var found by mutableStateOf(listOf<BleCompanion.Found>())

    /** USB serial devices plugged in (USB OTG). */
    var usbDevices by mutableStateOf(listOf<UsbDevice>())

    /** How the radio is reached: "BLE", "USB" or "TCP". */
    var linkKind by mutableStateOf("BLE")

    /** The app is on screen: received messages need no notification. Back on screen, a TCP
     * link waiting to retry tries at once. */
    var inForeground = false
        set(on) {
            if (on != field) log.note(if (on) "app in the foreground" else "app in the background")
            if (on && !field) tcp.retryNow()
            field = on
        }

    /** The app's recent log, for problem reports (ADR-0045). */
    val log = AppLog(::now)

    /** The conversation on screen: its messages need no notification. */
    var visibleChat: ChatId? = null

    /** Codec2 mode of new voice messages (§13.1): 0 = 700C (compact), 2 = 1600 (standard),
     * 3 = 2400 (clear). */
    var voiceMode by mutableStateOf(prefs.getInt(VOICE_MODE, 2))
        private set

    fun chooseVoiceMode(mode: Int) {
        voiceMode = mode
        prefs.edit { putInt(VOICE_MODE, mode) }
    }

    // USB first: the Bluetooth link may report its state while it is built.
    val usb = UsbCompanion(app, ::usbChanged, ::frame) { usbDevices = it }
    val tcp = TcpCompanion(::tcpChanged, ::frame, { tcpError = it }, log::note)

    /** Why the last TCP connection failed, for the pairing screen. */
    var tcpError by mutableStateOf<String?>(null)

    /** The last TCP companion address, to offer again. */
    var tcpAddress by mutableStateOf(prefs.getString(TCP_ADDRESS, null) ?: "10.0.2.2:5000")
        private set
    // Frames from Bluetooth are left aside while a TCP link is wanted: both may reach a radio.
    val ble = BleCompanion(app, ::bleChanged, { found = it }, { if (tcp.target == null) frame(it) })

    private var demo: DemoNetwork? = null
    private var demoJob: Job? = null
    private var wake: Job? = null
    private var debug = DebugOptions()

    /** Debug extra `joinNow`: a scanned invitation is joined at once. */
    val debugJoinNow: Boolean get() = debug.joinNow
    private var debugApplied = false
    private var debugVoiceSent = false

    init {
        clearOldShares(app)
        refreshAll()
        Notifications.channels(app)
        if (ble.permitted()) ble.start()
        loadDownloads()
        startBridge()
        shares = core.shares()
        positions = core.positions(now())
        refreshLocationAccess()
        updateLocator()
        // The TCP link in use when the app stopped comes back, retried until it answers; once
        // the model is built (its state changes run the core).
        if (prefs.getBoolean(TCP_ACTIVE, false)) {
            scope.launch(Dispatchers.Main) { tcp.connect(tcpAddress, retrying = true) }
        }
    }

    /** The stored identity, or a new one on a first launch; never a silent replacement of an
     * identity that cannot be read. `null` sets [identityProblem]. */
    private fun loadIdentity(): ByteArray? = when (val r = identityStore.read()) {
        is IdentityRead.Found -> r.seed
        IdentityRead.Absent -> runCatching { identityStore.create() }.getOrElse {
            identityProblem = IdentityProblem.NOT_SAVED
            null
        }
        is IdentityRead.Unavailable -> null.also { identityProblem = IdentityProblem.UNAVAILABLE }
        IdentityRead.Corrupt -> null.also { identityProblem = IdentityProblem.CORRUPT }
    }

    /** A core for our identity, or a throwaway one while it cannot be used. */
    private fun identityCore(): LichenCore {
        val seed = loadIdentity()
        identitySeed = seed
        if (seed == null) return makeCore(IdentityStore.randomBytes(32), persistent = false)
        val made = makeCore(seed, persistent = true)
        identityProblem = if (made.storageOk()) null else IdentityProblem.STORAGE
        return made
    }

    /** Reads the identity again ([IdentityProblemScreen]); the app starts with it once it is back. */
    fun retryIdentity() {
        val seed = loadIdentity() ?: return
        useIdentity(seed)
    }

    /** A new identity, chosen by the user when the stored one is unreadable for good: contacts
     * will have to verify it again. */
    fun startNewIdentity(): String? = try {
        val seed = IdentityStore.randomBytes(32)
        identityStore.replace(seed)
        phraseBackedUp = false
        prefs.edit { putBoolean(BACKED_UP, false) }
        useIdentity(seed)
        null
    } catch (_: IdentityWriteException) {
        res.getString(R.string.identity_not_saved)
    }

    /** The core restarts with [seed], our identity from now on. */
    private fun useIdentity(seed: ByteArray) {
        if (isDemo) stopDemo()
        resetState()
        identitySeed = seed
        replaceCore(makeCore(seed, persistent = true))
        identityProblem = if (core.storageOk()) null else IdentityProblem.STORAGE
        refreshAll()
        shares = core.shares()
        positions = core.positions(now())
        restartBridge()
        updateLocator()
        radioChanged(radioState)
        pump()
    }

    /** The database is encrypted when the core can (ADR-0036): with an empty key (the Keystore
     * cannot give it now) the core opens no database rather than a clear one. Without SQLCipher in
     * the core, nothing changes. */
    private fun makeCore(seed: ByteArray, persistent: Boolean): LichenCore =
        if (persistent && databaseEncryption()) {
            LichenCore.withDatabaseKey(
                "lichen-android",
                IdentityStore.randomBytes(32),
                seed,
                dataDirectory()?.path,
                DatabaseKey(app).read(),
            )
        } else {
            LichenCore(
                "lichen-android",
                IdentityStore.randomBytes(32),
                seed,
                if (persistent) dataDirectory()?.path else null,
            )
        }

    /** Per-node databases live in the app's private files. */
    private fun dataDirectory(): File? = File(app.filesDir, "lichen").takeIf { it.isDirectory || it.mkdirs() }

    private fun replaceCore(next: LichenCore) {
        val old = core
        core = next
        old.close()
    }

    private fun refreshAll() {
        contacts = people()
        nodes = core.nodes()
        savedRadios = core.savedRadios()
        groups = core.groups()
        // Invitations not answered yet are stored: back after a restart (§10.1).
        invitations = core.groupInvitations()
        channels = core.channels()
        conversations = core.conversations()
        refreshChatPrefs()
    }

    private fun people() = core.contacts().filter { it.person }

    fun chooseAppearance(a: Appearance) {
        appearance = a
        prefs.edit { putString(APPEARANCE, a.name) }
    }

    /** Applies the debug extras once (screenshots on the emulator). */
    fun applyDebug(options: DebugOptions) {
        if (debugApplied) return
        debugApplied = true
        debug = options
        debugScreen = options.screen
        when (options.identityProblem) {
            "unavailable" -> identityProblem = IdentityProblem.UNAVAILABLE
            "corrupt" -> identityProblem = IdentityProblem.CORRUPT
            "notsaved" -> identityProblem = IdentityProblem.NOT_SAVED
            "storage" -> identityProblem = IdentityProblem.STORAGE
        }
        options.onboarding?.let { markOnboarded(!it) }
        when (options.tab) {
            "map" -> tab = Tab.MAP
            "search" -> tab = Tab.SEARCH
            "settings" -> tab = Tab.SETTINGS
        }
        if (options.demo) startDemo()
        options.tcp?.let { connectTcp(it) }
    }

    // ----- demo network -----

    /** Runs the app on a simulated MeshCore network (two simulated users, a group), in real time. */
    fun startDemo() {
        if (isDemo) return
        resetState()
        replaceCore(makeCore(IdentityStore.randomBytes(32), persistent = false))
        // The demo's people are friends: their groups are joined without asking.
        core.setGroupConsent(false)
        demo = DemoNetwork().also { net ->
            // Camille offers her photo over Tor too when Tor is on (her own onion service).
            if (torEnabled) runCatching { net.camilleOverTor(folder("demo-tor-state").path, folder("demo-tor-cache").path) }
            // Debug `sosdemo`: Julien in SOS without GPS once the group is set up (a real
            // search); `sosdemo2`: Camille too, for five minutes.
            if (debug.sosDemo || debug.sosDemo2) net.julienSos()
            if (debug.sosDemo2) net.camilleSos(240uL, 300uL)
        }
        isDemo = true
        radioState = RadioState.Connected("ThinkNode M5")
        core.connect(now())
        updateLocator()
        demoJob = scope.launch {
            while (isActive) {
                demoStep()
                delay(200)
            }
        }
        pump()
        // Debug `offline`: the demo radio drops after that many seconds, with a text waiting for
        // Camille and one for the group; `online`: it comes back; `expiry`: texts expire sooner.
        if (debug.expiry > 0) core.setOutboxExpiry(debug.expiry.toULong() * 1000uL)
        if (debug.offline > 0) {
            scope.launch {
                delay(debug.offline * 1000L)
                if (!isDemo) return@launch
                setDemoRadio(false)
                contacts.firstOrNull { it.name == "Camille" }?.let {
                    send(res.getString(R.string.debug_offline_text), ChatId.contact(it.key))
                }
                groups.firstOrNull()?.let { send(res.getString(R.string.debug_offline_group), ChatId.group(it.gid)) }
            }
        }
        if (debug.online > 0) {
            scope.launch {
                delay(debug.online * 1000L)
                if (isDemo) setDemoRadio(true)
            }
        }
    }

    /** Switches the demo's simulated radio off or on: off, nothing reaches it and the core
     * pauses, as when Bluetooth drops (§6.1). */
    fun setDemoRadio(on: Boolean) {
        if (!isDemo || on != demoRadioOff) return
        demoRadioOff = !on
        if (on) {
            radioState = RadioState.Connected("ThinkNode M5")
            core.connect(now())
        } else {
            radioState = RadioState.Idle
            core.disconnect(now())
        }
        pump()
    }

    /**
     * Imports the database the official MeshCore app exports (ADR-0033), picked with the system's
     * file picker: copied to the cache first (the core reads a path), removed after. What came in,
     * or why not.
     */
    fun importMeshCore(uri: android.net.Uri): String {
        val res = app.resources
        val copy = File(app.cacheDir, "meshcore-export.db")
        return try {
            app.contentResolver.openInputStream(uri)?.use { input -> copy.outputStream().use { input.copyTo(it) } }
                ?: return LichenException.NotAnExport().display(res)
            val r = core.importMeshcoreApp(copy.path)
            val text = res.getString(R.string.import_done, r.direct.toInt(), r.channel.toInt())
            if (r.alreadyHere > 0u) text + " " + res.getString(R.string.import_already_here, r.alreadyHere.toInt()) else text
        } catch (e: LichenException) {
            e.display(res)
        } catch (e: java.io.IOException) {
            LichenException.NotAnExport().display(res)
        } finally {
            copy.delete()
        }
    }

    fun stopDemo() {
        if (!isDemo) return
        demoJob?.cancel()
        demoJob = null
        demoFixJob?.cancel()
        demoFixJob = null
        demo?.close()
        demo = null
        isDemo = false
        demoRadioOff = false
        resetState()
        replaceCore(identityCore())
        radioState = RadioState.Idle
        refreshAll()
        shares = core.shares()
        updateLocator()
        radioChanged(usb.device?.let { RadioState.Connected(it.productName ?: "USB") } ?: ble.state)
    }

    /** The simulated network runs on a thread of its own, reads and writes in order: catching
     * up after a pause can take seconds, and must not freeze the screen. */
    private val demoContext = Dispatchers.Default.limitedParallelism(1)
    private var demoReading = false

    private fun demoStep() {
        val d = demo ?: return
        if (demoReading) return
        demoReading = true
        val t = now()
        scope.launch {
            val frames = withContext(demoContext) { d.read(t) }
            demoReading = false
            if (demo !== d) return@launch
            // Radio off: the network goes on, and what it pushes is lost.
            for (f in frames) if (!demoRadioOff) core.onCompanionFrame(f, t)
            pump()
        }
    }

    private fun resetState() {
        nodeName = ""
        nodeKey = null
        radio = null
        conflicts = emptyMap()
        contacts = emptyList()
        nodes = emptyList()
        groups = emptyList()
        chats.clear()
        conversations = emptyList()
        secured = emptySet()
        lastSnr = emptyMap()
        shares = emptyList()
        positions = emptyList()
        sos = false
        incidents = emptyMap()
        sosAlert = null
        chatPrefs = emptyMap()
        focus = null
        searchTarget = null
        searches = emptyList()
        searchedBy = emptyList()
        loweredPower = null
        sosStatus = null
        myFix = null
        lastFed = null
    }

    val isConnected: Boolean get() = radioState is RadioState.Connected

    // ----- runner -----

    fun now(): ULong = (SystemClock.elapsedRealtime() - started).toULong()

    /** The radio link changed state (BLE or USB). */
    private fun radioChanged(state: RadioState) {
        if (isDemo) return
        val was = isConnected
        radioState = state
        // No radio session with a throwaway identity (identityProblem).
        if (state is RadioState.Connected && identityProblem == null) {
            // Receive with the screen off (ADR-0006); stops when the user disconnects.
            RadioService.start(app, state.name, sharing = shares.isNotEmpty() || sos)
            core.connect(now())
            pump()
        } else if (was && !isConnected) {
            // Lost: texts wait and a transfer pauses until it is back (§6.1).
            core.disconnect(now())
            pump()
        }
    }

    private fun bleChanged(s: RadioState) {
        log.note("Bluetooth: ${s.logWord}")
        if (usb.device != null || tcp.target != null) return
        linkKind = "BLE"
        link = RadioLink(ble::write)
        radioChanged(s)
    }

    private fun usbChanged(s: RadioState) {
        if (s is RadioState.Connected) {
            ble.stopScan()
            linkKind = "USB"
            link = RadioLink(usb::write)
            radioChanged(s)
        } else {
            linkKind = "BLE"
            link = RadioLink(ble::write)
            radioChanged(ble.state)
        }
    }

    private fun tcpChanged(s: RadioState) {
        log.note("TCP: ${s.logWord}")
        if (s is RadioState.Connected) prefs.edit { putBoolean(TCP_ACTIVE, true) }
        if (tcp.target != null) {
            ble.stopScan()
            linkKind = "TCP"
            link = RadioLink(tcp::write)
            radioChanged(s)
        } else {
            linkKind = "BLE"
            link = RadioLink(ble::write)
            radioChanged(ble.state)
        }
    }

    /** Connects to the companion at [address] ("host:port"); false when it is not an address. */
    fun connectTcp(address: String): Boolean {
        tcpError = null
        if (!tcp.connect(address)) return false
        tcpAddress = address
        prefs.edit { putString(TCP_ADDRESS, address) }
        return true
    }

    /** The Bluetooth permissions were just granted. */
    fun bluetoothReady() = ble.start()

    fun scan() = ble.scan()

    fun stopScan() = ble.stopScan()

    fun pair(address: String) = ble.connect(address)

    fun connectUsb(d: UsbDevice) = usb.connect(d)

    /** Disconnects the radio (and forgets it, over Bluetooth). */
    fun disconnect() {
        when {
            tcp.target != null -> {
                prefs.edit { putBoolean(TCP_ACTIVE, false) }
                tcp.close()
            }
            usb.device != null -> usb.close()
            else -> ble.disconnect()
        }
        RadioService.stop(app)
    }

    /** A frame from the radio (ignored while the demo network runs). */
    fun frame(data: ByteArray) {
        if (isDemo) return
        core.onCompanionFrame(data, now())
        pump()
    }

    /** Writes due frames, reads events, and sets the timer for the next wake-up. */
    fun pump() {
        val t = now()
        core.setClock(System.currentTimeMillis().toULong(), t)
        core.tick(t)
        // The radio's battery, for the low-battery warning (ADR-0042).
        core.refreshBatteryIfDue(t)
        val d = demo
        val toDemo = mutableListOf<ByteArray>()
        while (true) {
            val f = core.pollCompanion(t) ?: break
            if (d != null) {
                if (!demoRadioOff) toDemo.add(f)
            } else {
                link?.write(f)
            }
        }
        if (d != null && toDemo.isNotEmpty()) scope.launch(demoContext) { for (f in toDemo) d.write(f, t) }
        while (true) {
            handle(core.pollEvent() ?: break)
        }
        // Sender keys arrive without an event: refresh the groups when they changed.
        val g = core.groups()
        if (signature(g) != signature(groups)) groups = g
        budget = core.budget(t)
        val st = core.sosStatus(t)
        if (st != sosStatus) sosStatus = st
        if (searches.isNotEmpty() || sos) reportBattery()
        wake?.cancel()
        val next = core.nextWakeup() ?: return
        val wait = maxOf(20L, next.toLong() - t.toLong())
        wake = scope.launch {
            delay(wait)
            pump()
        }
    }

    private fun signature(gs: List<GroupInfo>): String = gs.joinToString("|") { g ->
        g.gid.hex() + g.name + g.keysKnown + g.members.joinToString(",") { it.key.hex() + it.verified }
    }

    private fun handle(e: LichenEvent) {
        when (e) {
            is LichenEvent.Connected -> {
                nodeKey = e.key
                nodeName = e.name
                radio = e.radio
                radioSettings = core.radioSettings()
                radioConnected()
                // The ongoing notification names the node, not the link's address.
                if (!isDemo) RadioService.start(app, e.name, shares.isNotEmpty() || sos)
            }
            is LichenEvent.IdentityLearned -> {
                contacts = people()
                // Debug extra `open`: opened before their identity arrived, as a MeshCore chat;
                // now that they turn out to have lichen, their conversation.
                if (debugMesh == e.node.hex()) {
                    debugMesh = null
                    val chat = ChatId.contact(e.node)
                    if (debug.timer > 0) setTimer(chat, debug.timer.toUInt())
                    openChat = chat
                }
            }
            is LichenEvent.IdentityConflict -> conflicts = conflicts + (e.node.hex() to e.identity)
            LichenEvent.RadioSettings -> {
                // Read again after a change (ADR-0032): the header follows the radio's parameters.
                val r = core.radioSettings()
                radioSettings = r
                radio = core.radio()
                if (r.name.isNotEmpty()) nodeName = r.name
            }
            LichenEvent.ContactsUpdated -> {
                contacts = people()
                nodes = core.nodes()
                debugInvites()
                debugFill()
                debugOpen()
                debugVoice()
                debugLocation()
            }
            is LichenEvent.SessionEstablished -> secured = secured + e.peer.hex()
            is LichenEvent.Received -> {
                lastSnr = lastSnr + (e.from.hex() to e.snrDb)
                prefetchGif(e.message)
                // A muted chat stays silent; SOS alerts have their own path, never muted.
                val chat = ChatId.contact(e.from)
                if (!isMuted(chat)) notify(chat, name(e.from), e.message.display(res))
                debug.say?.let { say ->
                    debug = debug.copy(say = null)
                    send(say, ChatId.contact(e.from))
                }
            }
            is LichenEvent.Progress, is LichenEvent.Delivered, is LichenEvent.Failed, is LichenEvent.GroupSent -> Unit
            is LichenEvent.Reacted -> {
                val chat = ChatId.of(e.chat)
                e.target?.let { upsert(it) }
                // A reaction to one of ours, as messaging apps do; never for a muted chat.
                val target = e.target
                if (target != null && target.outgoing && e.emoji.isNotEmpty() && !isMuted(chat)) {
                    notify(chat, title(chat), res.getString(R.string.reacted_to, name(e.by), e.emoji, target.message.display(res)))
                }
            }
            is LichenEvent.NewerVersion -> newerPeers = newerPeers + e.node.hex()
            // Its time was up (§6.5): off the screen, and out of the notifications.
            is LichenEvent.MessageRemoved -> {
                val chat = ChatId.of(e.chat)
                chats[chat]?.let { list -> chats[chat] = list.filter { it.id != e.id } }
                conversations = core.conversations()
                Notifications.clear(app, chat)
            }
            // A deleted message's content: no longer offered, no longer kept (§6.4, SR-28).
            is LichenEvent.MediaGone -> {
                runCatching { bridge?.revoke(e.hash) }
                downloads.remove(e.hash.hex())?.delete()
            }
            is LichenEvent.GroupInvited -> {
                invitations = core.groupInvitations()
                // Tapping it opens the admin's conversation; the invitation waits in Messages.
                notify(ChatId.contact(e.admin), res.getString(R.string.group_invitation), res.getString(R.string.invites_you_to, name(e.admin), e.name), actions = false)
            }
            is LichenEvent.GroupJoined, is LichenEvent.GroupUpdated, is LichenEvent.GroupLeft -> {
                invitations = core.groupInvitations()
                if (e is LichenEvent.GroupJoined && joining.containsKey(e.admin.hex())) {
                    joining = joining - e.admin.hex()
                    joinedByInvite = joinedByInvite + (e.admin.hex() to e.gid)
                }
                groups = core.groups()
                contacts = people()
                debugOpen()
            }
            is LichenEvent.MemberJoined -> {
                groups = core.groups()
                contacts = people()
                val chat = ChatId.group(e.gid)
                if (!isMuted(chat)) notify(chat, title(chat), res.getString(R.string.joined_with_invite, name(e.node)))
            }
            is LichenEvent.GroupMessage -> {
                lastSnr = lastSnr + (e.from.hex() to e.snrDb)
                prefetchGif(e.message)
                val chat = ChatId.group(e.gid)
                if (!isMuted(chat)) notify(chat, title(chat), "${name(e.from)}: ${e.message.display(res)}")
            }
            is LichenEvent.MessageChanged -> {
                upsert(e.message)
                conversations = core.conversations()
                // A timer changed, by us or by them (§6.5).
                (e.message.message as? Message.Notice)?.takeIf { it.kind.toInt() == 11 }?.text?.toUIntOrNull()?.let {
                    timers[ChatId.of(e.message.chat)] = it
                }
                // Taken back by its author (§6.4): its notification goes too.
                if (e.message.message is Message.Deleted && !e.message.outgoing) {
                    Notifications.clear(app, ChatId.of(e.message.chat))
                }
                // Debug extra `media`: ask for full images as previews arrive, fetch announcements.
                val m = e.message
                if (debug.media && !m.outgoing) {
                    val msg = m.message
                    if (msg is Message.Image && !msg.full && m.id !in requestedFull) requestFull(m)
                    if (msg is Message.Announce) fetch(m)
                }
            }
            is LichenEvent.Location -> {
                positions = core.positions(now())
                val p = e.position
                if (p.sos && !e.end) heardSos(p.from, p.group, p, null) else endSos(p.from)
            }
            // An SOS without GPS shows here (protocol.md §11.1).
            is LichenEvent.Beacon -> if (e.sos) heardSos(e.from, e.gid, null, e.battery) else endSos(e.from)
            LichenEvent.SharesChanged -> {
                shares = core.shares()
                updateLocator()
            }
            is LichenEvent.MediaRequested -> Unit
            is LichenEvent.Searched -> {
                // A member asks where we are and we answer (SOS or this group's consent, §11).
                val chat = ChatId.group(e.gid)
                notify(chat, title(chat), app.getString(R.string.searching_for_you, name(e.by)), actions = false)
                searchedBy = core.searchedBy(now())
            }
            is LichenEvent.SearchUpdated -> {
                searchVersion++
                refreshSearches()
            }
            // lc-ffi keeps the power to restore; we show it while it is lowered.
            is LichenEvent.TxPower -> loweredPower = if (e.previous == null) null else e.dbm
            is LichenEvent.PlainReceived -> {
                // A MeshCore message (§15.1): "Marc · MeshCore", or "Lucie · Public" on a
                // channel. Never like a lichen chat's: a node can send plain text under any
                // name, and its notification must not pass for the encrypted conversation
                // (review of 2026-11-15, SR-30).
                val chat = ChatId.of(e.chat)
                if (!isMuted(chat)) {
                    val title = if (chat.channel || chat.kind == ChatId.Kind.ROOM) listOfNotNull(e.label, title(chat)).joinToString(" · ")
                    else "${e.label ?: title(chat)} · MeshCore"
                    notify(chat, title, e.text)
                }
                if (chat.kind == ChatId.Kind.MESH) {
                    debug.sayPlain?.let { say ->
                        debug = debug.copy(sayPlain = null)
                        send(say, chat)
                    }
                }
            }
            // An archived conversation came back with a message (ADR-0034).
            is LichenEvent.ChatPrefsChanged -> refreshChatPrefs()
            is LichenEvent.ChannelsUpdated -> {
                channels = core.channels()
                refreshChatPrefs()
                debugOpen()
            }
            // A trace or a server's answer (§16).
            is LichenEvent.NodeUpdated -> meshToolsVersion++
        }
    }

    /** A notification for a message, unless its conversation is on screen (not behind the lock's
     * cover); `actions`: Reply and Mark as read (ADR-0034). With the app lock on, it says only
     * "New message": no sender, no group, no text, nothing to answer it with. */
    private fun notify(chat: ChatId, title: String, text: String, actions: Boolean = true) {
        if (inForeground && !lock.locked && visibleChat == chat) return
        if (lock.enabled) {
            Notifications.message(app, chat, app.getString(R.string.app_name), app.getString(R.string.new_message))
        } else {
            Notifications.message(app, chat, title, text, actions)
        }
    }

    /** A text typed in a notification (ADR-0034): a message like any other, in the outbox when
     * the radio is away. Not sent: said in the same place. */
    fun replyFromNotification(chat: ChatId, text: String) {
        val error = send(text, chat)
        if (error == null) {
            markRead(chat)
            Notifications.clear(app, chat)
        } else if (lock.enabled) {
            Notifications.message(app, chat, app.getString(R.string.app_name), app.getString(R.string.answer_not_sent))
        } else {
            Notifications.message(app, chat, app.getString(R.string.not_sent), error)
        }
    }

    /** "Mark as read" on a notification. */
    fun markReadFromNotification(chat: ChatId) {
        markRead(chat)
        Notifications.clear(app, chat)
    }

    /** Debug extra `share`: 100 m with that contact. */
    private fun debugLocation() {
        debug.share?.let { name ->
            val c = contacts.firstOrNull { it.name == name } ?: return@let
            debug = debug.copy(share = null)
            setShare(ChatId.contact(c.key), SharePrecision.M100, 1.25)
        }
    }

    /** Debug extra `media`: a 4 s voice message to Camille, 20 s after she appears. */
    private fun debugVoice() {
        if (!debug.media || debugVoiceSent) return
        val c = contacts.firstOrNull { it.name == "Camille" } ?: return
        debugVoiceSent = true
        val pcm = List(32_000) { i ->
            val t = i / 8_000.0
            (6_000 * sin(2 * PI * (150 + 30 * sin(t * 3)) * t) * (0.6 + 0.4 * sin(t * 9))).toInt().toShort()
        }
        scope.launch {
            delay(20_000)
            sendVoice(prepareVoice(pcm, voiceMode.toUByte()), ChatId.contact(c.key))
        }
    }

    /** Debug extra `open`: the conversation with this contact or group, once it exists. */
    /** Debug extra `fill`: older messages with Camille in the demo, once. */
    private var debugFilled = false

    private fun debugFill() {
        if (!isDemo || debugFilled || debug.fill <= 0) return
        val c = contacts.firstOrNull { it.name == "Camille" } ?: return
        debugFilled = true
        runCatching { core.demoFillHistory(ChatId.contact(c.key).ref, debug.fill.toUInt()) }
        conversations = core.conversations()
    }

    /** Debug `screen invite`: a group of your own, alone, to invite people with a QR code;
     * `screen join`: Camille shows an invitation to a group of hers, and you scan it. */
    private fun debugInvites() {
        if (!isDemo || debugInviteMade) return
        when (debugScreen) {
            "invite" -> {
                val gid = runCatching { core.createGroup(res.getString(R.string.sunday_hike), emptyList(), now()) }.getOrNull() ?: return
                debugInviteMade = true
                groups = core.groups()
                openChat = ChatId.group(gid)
            }
            "join" -> {
                val d = demo ?: return
                if (contacts.none { it.name == "Camille" }) return
                debugInviteMade = true
                scope.launch { debugInvite = withContext(demoContext) { d.camilleInvite() } }
            }
        }
    }

    /** The MeshCore chat the debug extra `open` chose, by hex node key, while it may change. */
    private var debugMesh: String? = null

    private fun debugOpen() {
        val name = debug.open ?: return
        // Someone without lichen (the demo's Marc): their MeshCore chat (§15).
        val chat = contacts.firstOrNull { it.name == name }
            ?.let { if (it.identity == null) ChatId.mesh(it.key) else ChatId.contact(it.key) }
            ?: groups.firstOrNull { it.name == name }?.let { ChatId.group(it.gid) }
            ?: channels.firstOrNull { it.name == name }?.let { ChatId.channel(it.id) }
            ?: return
        debug = debug.copy(open = null)
        if (chat.plain && !chat.channel) debugMesh = chat.hex
        if (debug.timer > 0) setTimer(chat, debug.timer.toUInt())
        openChat = chat
    }

    private fun upsert(m: StoredMessage) {
        val chat = ChatId.of(m.chat)
        val list = chats[chat] ?: return
        val i = list.indexOfFirst { it.id == m.id }
        chats[chat] = when {
            i >= 0 -> list.toMutableList().also { it[i] = m }
            // Rows in history order, even when a reply is sent while the message it answers
            // is still being reported.
            list.isNotEmpty() && m.id < list.last().id -> (list + m).sortedBy { it.id }
            else -> list + m
        }
    }

    fun unread(chat: ChatId): Int = conversation(chat)?.unread?.toInt() ?: 0

    fun last(chat: ChatId): StoredMessage? = conversation(chat)?.last

    private fun conversation(chat: ChatId) = conversations.firstOrNull { ChatId.of(it.chat) == chat }

    val totalUnread: Int get() = conversations.sumOf { it.unread.toInt() }

    // ----- actions -----

    /** Conversations with older messages to load, and those whose history failed to load. */
    val olderAvailable = mutableStateMapOf<ChatId, Boolean>()
    val historyError = mutableStateMapOf<ChatId, String>()

    /** Loads the latest page of `chat` (when its screen opens). */
    fun load(chat: ChatId) {
        try {
            val page = core.history(chat.ref, null, HISTORY_PAGE)
            timers[chat] = runCatching { core.disappearing(chat.ref) }.getOrDefault(0u)
            chats[chat] = page
            olderAvailable[chat] = page.size == HISTORY_PAGE.toInt()
            historyError.remove(chat)
        } catch (_: LichenException) {
            if (chats[chat] == null) chats[chat] = emptyList()
            historyError[chat] = res.getString(R.string.history_unreadable)
        }
    }

    /** The page before the oldest message shown. */
    fun loadOlder(chat: ChatId) {
        val first = chats[chat]?.firstOrNull()?.id ?: return
        try {
            val page = core.history(chat.ref, first, HISTORY_PAGE)
            chats[chat] = page + chats[chat].orEmpty()
            olderAvailable[chat] = page.size == HISTORY_PAGE.toInt()
            historyError.remove(chat)
        } catch (_: LichenException) {
            historyError[chat] = res.getString(R.string.history_unreadable)
        }
    }

    /** The text typed in `chat` and not sent, kept across screens and restarts, for this
     * identity (or the demo) only (recheck R07). */
    fun draft(chat: ChatId): String = prefs.getString(draftKey(chat), "") ?: ""

    fun setDraft(chat: ChatId, text: String) {
        prefs.edit { if (text.isEmpty()) remove(draftKey(chat)) else putString(draftKey(chat), text) }
    }

    private fun draftKey(chat: ChatId): String =
        DRAFT + (if (isDemo) "demo" else core.fingerprint().joinToString("")) + "|" + chat.route

    /** The message the draft of `chat` answers (§6.2), kept with the draft (ADR-0034). */
    fun replyDraft(chat: ChatId): Long? =
        prefs.getLong(draftKey(chat) + "|reply", -1L).takeIf { it >= 0 }

    fun setReplyDraft(chat: ChatId, id: Long?) {
        prefs.edit { if (id == null) remove(draftKey(chat) + "|reply") else putLong(draftKey(chat) + "|reply", id) }
    }

    /** A recording stopped because its conversation was left: offered there again, to listen
     * to, send or discard (ADR-0034). In memory only, like the recording itself. */
    class VoiceDraft(val pcm: ShortArray, val mode: Int)

    val voiceDrafts = mutableMapOf<ChatId, VoiceDraft>()

    /** Conversation `chat` is left: a recording made for it stops and waits for it. */
    fun keepRecording(chat: ChatId) {
        if (recorder.chat != chat) return
        recorder.stop(keep = true)?.let { voiceDrafts[chat] = VoiceDraft(it, recordMode(chat)) }
    }

    /** The Codec2 mode a new recording uses: the chosen one, 700C in a group (ADR-0035). */
    fun recordMode(chat: ChatId): Int = if (chat.group) GROUP_VOICE_MODE else voiceMode

    /**
     * Makes message `id` (a search result, a quoted message) part of the loaded history: what
     * lies between it and the oldest message shown is read at once (recheck R08). False when it
     * is no longer on this phone.
     */
    fun loadUntil(chat: ChatId, id: Long): Boolean {
        if (chats[chat] == null) load(chat)
        val loaded = chats[chat].orEmpty()
        if (loaded.any { it.id == id }) return true
        val first = loaded.firstOrNull()?.id ?: return false
        if (id >= first) return false
        val gap = runCatching { core.historyFrom(chat.ref, id, first) }.getOrNull() ?: return false
        if (gap.firstOrNull()?.id != id) return false
        chats[chat] = gap + loaded
        return true
    }

    // ----- chat essentials (pins, mutes, search) -----

    fun refreshChatPrefs() {
        chatPrefs = core.chatPrefs().associateBy { ChatId.of(it.chat) }
        blocked = core.blocked()
    }

    // ----- blocking and archiving (ADR-0034) -----

    /** People the user blocked, the latest first. */
    var blocked by mutableStateOf(listOf<BlockedContact>())

    fun isBlocked(node: ByteArray): Boolean = blocked.any { it.node.contentEquals(node) }

    /** Blocks contact `node`: local, they are not told. Returns an error to show, or null. */
    fun block(node: ByteArray): String? = try {
        core.block(node, now())
        refreshChatPrefs()
        reloadGroupChats()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    fun unblock(node: ByteArray) {
        runCatching { core.unblock(node) }
        refreshChatPrefs()
        reloadGroupChats()
    }

    /** A block hides (or shows again) a member's posts already in group histories. */
    private fun reloadGroupChats() {
        chats.keys.filter { it.group }.forEach { load(it) }
        conversations = core.conversations()
    }

    // ----- sharing into lichen (ADR-0034) -----

    /** Something another app shared, waiting for the user to choose a conversation. */
    var incomingShare by mutableStateOf<IncomingShare?>(null)

    /** What the user chose to send to a conversation: its composer takes it. */
    val sharedFor = mutableStateMapOf<ChatId, IncomingShare>()

    /** Sends what was shared to `chat`: its conversation opens, the text in the draft, each
     * photo or file on its sheet. */
    fun share(share: IncomingShare, chat: ChatId) {
        incomingShare = null
        sharedFor[chat] = share
        tab = Tab.MESSAGES
        openChat = chat
    }

    // ----- media and storage (ADR-0034) -----

    /** What `chat` holds: photos, files and voice messages, the newest first. */
    fun chatMedia(chat: ChatId): List<StoredMessage> =
        runCatching { core.chatMedia(chat.ref, null, 2_000u) }.getOrDefault(emptyList())

    /** What each conversation takes on this phone (bytes): its messages and the media kept for
     * it, and the files downloaded over the bridge for it. */
    fun storageUsage(): List<Pair<ChatUsage, Long>> = core.storageUsage().map { u ->
        val downloaded = u.announced.sumOf { h -> downloads[h.hex()]?.length() ?: 0L }
        u to (u.bytes.toLong() + downloaded)
    }

    /** Removes photos, files and voice messages older than `days` days from this phone, as
     * "delete for me" does (their downloads go with them, SR-28). Returns how many. */
    fun clearMedia(days: Int): Int {
        val before = System.currentTimeMillis() - days * 86_400_000L
        val n = runCatching { core.clearMediaOlderThan(before) }.getOrDefault(0u)
        chats.keys.toList().forEach { load(it) }
        conversations = core.conversations()
        pump()
        return n.toInt()
    }

    /** Archived: out of the list, under "Archived", until a message comes (unless muted). */
    fun isArchived(chat: ChatId): Boolean = chatPrefs[chat]?.archived == true

    fun setArchived(chat: ChatId, archived: Boolean) {
        runCatching { core.setArchived(chat.ref, archived) }
        refreshChatPrefs()
    }

    fun isPinned(chat: ChatId): Boolean = chatPrefs[chat]?.pinnedAtMs != null

    /** Muted now: its messages post no notification (SOS alerts still do). */
    fun isMuted(chat: ChatId): Boolean = (chatPrefs[chat]?.mutedUntilMs ?: Long.MIN_VALUE) > System.currentTimeMillis()

    fun setPinned(chat: ChatId, pinned: Boolean) {
        runCatching { core.setPinned(chat.ref, pinned) }
        refreshChatPrefs()
    }

    /** Mutes `chat` for `hours`, or until the user turns it back on (null). */
    fun mute(chat: ChatId, hours: Int?) {
        val until = hours?.let { System.currentTimeMillis() + it * 3_600_000L } ?: Long.MAX_VALUE
        runCatching { core.setMuted(chat.ref, until) }
        refreshChatPrefs()
    }

    fun unmute(chat: ChatId) {
        runCatching { core.setMuted(chat.ref, null) }
        refreshChatPrefs()
    }

    /** Text messages containing `query` (case and accents aside), newest first. */
    fun searchMessages(query: String): List<StoredMessage> = core.searchMessages(query, 50u)

    fun markRead(chat: ChatId) {
        runCatching { core.markRead(chat.ref) }
        conversations = core.conversations()
    }

    /** Deletes a conversation's messages; the contact or the group stays. */
    fun deleteConversation(chat: ChatId) {
        runCatching { core.deleteConversation(chat.ref) }
        setDraft(chat, "")
        setReplyDraft(chat, null)
        voiceDrafts.remove(chat)
        chats.remove(chat)
        conversations = core.conversations()
    }

    /** A MeshCore advert (flood), so that radios around add ours as a contact. */
    fun sendAdvert() {
        core.sendAdvert(true, now())
        pump()
    }

    /** What sending `text` to `chat` costs us; for a group, our own floods (both copies). */
    fun estimate(chat: ChatId, text: String): Estimate? {
        if (text.isEmpty()) return null
        val len = text.toByteArray().size.toUInt()
        if (chat.group) return core.estimateGroupText(len)
        if (chat.plain) return runCatching { core.estimatePlain(chat.ref, len) }.getOrNull()
        val key = chat.contactKey ?: return null
        return runCatching { core.estimateText(key, len) }.getOrNull()
    }

    /** Sends `text`; returns an error message to show, or null. */
    fun send(text: String, chat: ChatId): String? {
        val sentAt = (System.currentTimeMillis() / 1000).toUInt()
        return try {
            val m = when (chat.kind) {
                ChatId.Kind.GROUP -> core.sendGroupText(chat.hex.unhex(), text, sentAt, now())
                ChatId.Kind.CONTACT -> core.sendText(chat.hex.unhex(), text, sentAt, now())
                ChatId.Kind.MESH, ChatId.Kind.CHANNEL, ChatId.Kind.ROOM -> core.sendPlain(chat.ref, text, sentAt, now())
            }
            if (chats[chat] == null) load(chat) else upsert(m)
            conversations = core.conversations()
            pump()
            null
        } catch (e: LichenException) {
            e.display(res)
        }
    }

    // ----- stickers and GIFs (protocol.md §6.3) -----

    /** Sends a sticker; returns an error message to show, or null. */
    fun sendSticker(emoji: String, chat: ChatId): String? =
        sendMessage(chat) { at, now -> core.sendSticker(chat.ref, emoji, at, now) }

    /** Sends GIPHY GIF `link` (only its id, shape and title travel). */
    fun sendGif(link: GifLink, size: Pair<UShort, UShort>?, chat: ChatId): String? =
        sendMessage(chat) { at, now ->
            core.sendGif(chat.ref, link.id, size?.first ?: 0u, size?.second ?: 0u, link.title, at, now)
        }

    private fun sendMessage(chat: ChatId, send: (UInt, ULong) -> StoredMessage): String? = try {
        val m = send((System.currentTimeMillis() / 1000).toUInt(), now())
        if (chats[chat] == null) load(chat) else upsert(m)
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    private val gifFolder: File get() = File(app.cacheDir, "gifs").also { it.mkdirs() }

    /** A GIF loaded before (the system may clear the cache). */
    fun cachedGif(id: String): ByteArray? {
        if (id.isEmpty() || !id.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' }) return null
        return File(gifFolder, "$id.gif").takeIf { it.isFile }?.readBytes()
    }

    /** GIPHY GIF `id`, through Tor when Tor is on, directly otherwise; kept in the cache. With
     * Tor on it never goes direct, also while Tor starts (then it fails, and loads once Tor is
     * up): GIPHY never sees the address (§6.3, review of 2026-11-15, SR-26). */
    suspend fun loadGif(id: String): ByteArray? {
        cachedGif(id)?.let { return it }
        val b = bridge ?: return null
        val data = runCatching { b.gif(id, torEnabled) }.getOrNull() ?: return null
        withContext(Dispatchers.IO) {
            runCatching {
                File(gifFolder, "$id.gif").writeBytes(data)
                // Within 50 MB, the oldest dropped first.
                var total = 0L
                gifFolder.listFiles()?.sortedByDescending { it.lastModified() }?.forEach { f ->
                    total += f.length()
                    if (total > 50L shl 20) f.delete()
                }
            }
        }
        return data
    }

    /** Sends a failed text or voice message again: the same message, so that someone who
     * already has it does not get it twice (§6.1). */
    fun retry(m: StoredMessage): String? = try {
        upsert(core.retryMessage(m.id, now()))
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    // ----- replies and reactions (protocol.md §6.2) -----

    /** Answers `target` with `text` (a text that waits for the radio like any other). */
    fun reply(text: String, target: StoredMessage): String? = try {
        val sentAt = (System.currentTimeMillis() / 1000).toUInt()
        upsert(core.sendReply(target.chat, target.id, text, sentAt, now()))
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    // ----- editing and deleting (protocol.md §6.4) -----

    /** Changes the text of our `target`: an error message to show, or null. */
    fun edit(text: String, target: StoredMessage): String? = try {
        upsert(core.sendEdit(target.id, text, now()))
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Takes our message back on the phones that receive it. */
    fun deleteForEveryone(m: StoredMessage): String? = try {
        upsert(core.sendDelete(m.id, now()))
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Forwards `m` to `chat` as a new message of ours (ADR-0027). */
    fun forward(m: StoredMessage, chat: ChatId): String? = try {
        val sent = core.forward(m.id, chat.ref, (System.currentTimeMillis() / 1000).toUInt(), now())
        if (chats[chat] == null) load(chat) else upsert(sent)
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Removes a message from this phone only. */
    fun deleteForMe(m: StoredMessage): String? = try {
        core.deleteForMe(m.id)
        val chat = ChatId.of(m.chat)
        chats[chat]?.let { list -> chats[chat] = list.filter { it.id != m.id } }
        conversations = core.conversations()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Reacts to `m` with `emoji` (empty: removes ours). */
    fun react(m: StoredMessage, emoji: String): String? = try {
        upsert(core.sendReaction(m.id, emoji, now()))
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** What a reaction costs us in `chat`. */
    fun reactionCost(chat: ChatId): Estimate? = runCatching { core.estimateReaction(chat.ref) }.getOrNull()

    /** Cancels a text that is still waiting and none of which went out: it disappears. */
    fun cancel(m: StoredMessage): String? = try {
        core.cancelMessage(m.id)
        val chat = ChatId.of(m.chat)
        chats[chat]?.let { list -> chats[chat] = list.filter { it.id != m.id } }
        conversations = core.conversations()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    // ----- location (protocol.md §12) -----

    /** Location is needed while sharing, during SOS, or with the map on screen; in the demo it
     * comes from the simulated network. */
    fun updateLocator() {
        // A search measures with our position: it must stay fresh, screen off too.
        val sharing = shares.isNotEmpty() || sos || searches.isNotEmpty()
        val needed = sharing || locationUsers.isNotEmpty()
        val d = demo
        if (d != null) {
            locator.stop()
            if (needed && demoFixJob == null) {
                demoFixJob = scope.launch {
                    while (isActive) {
                        gotFix(withContext(demoContext) { d.myPosition() })
                        delay(10_000)
                    }
                }
            } else if (!needed) {
                demoFixJob?.cancel()
                demoFixJob = null
            }
        } else if (needed) {
            refreshLocationAccess()
            locator.start()
        } else {
            locator.stop()
        }
        // Keep the location alive in the background while sharing (the radio's service).
        if (isConnected && !isDemo) RadioService.start(app, nodeName.ifEmpty { "MeshCore" }, sharing)
    }

    /** The location permission was just granted. */
    /** Denied, approximate only, services off: kept apart (audit M10). */
    fun refreshLocationAccess() {
        locationDenied = !locator.permitted()
        locationApproximate = locator.approximate()
        locationServicesOff = !locator.servicesOn()
    }

    fun locationReady() {
        refreshLocationAccess()
        updateLocator()
    }

    /** A fix from the phone: shown at once, handed to the core every 5 seconds at most. */
    private fun gotFix(fix: LocationFix) {
        if (myFix == null) compass.at(fix.lat, fix.lon, fix.altM ?: 0.0)
        myFix = fix
        val t = now()
        lastFed?.let { if (t < it + 5_000uL) return }
        lastFed = t
        core.updateLocation(fix, t)
        pump()
    }

    fun refreshPositions() {
        positions = core.positions(now())
    }

    fun share(chat: ChatId): ShareInfo? = shares.firstOrNull { ChatId.of(it.chat) == chat }

    /** Shares our position with `chat` at `precision` for `hours`, or stops (null). */
    fun setShare(chat: ChatId, precision: SharePrecision?, hours: Double): String? {
        try {
            if (precision != null) {
                val until = (System.currentTimeMillis() / 1000 + (hours * 3600).toLong()).toULong()
                core.startShare(chat.ref, precision, until, now())
            } else {
                core.stopShare(chat.ref, now())
            }
        } catch (e: LichenException) {
            return e.display(res)
        }
        shares = core.shares()
        updateLocator()
        pump()
        return null
    }

    fun stopAllShares() {
        for (s in shares) runCatching { core.stopShare(s.chat, now()) }
        shares = core.shares()
        updateLocator()
        pump()
    }

    fun changeSos(on: Boolean) {
        core.setSos(on, now())
        sos = core.sos()
        updateLocator()
        pump()
    }

    /** Distance (m) and bearing (degrees from north) from us to `p`. */
    fun distanceTo(p: PeerPosition): Pair<Double, Double>? = distanceTo(p.lat, p.lon)

    /** Distance (m) and bearing (degrees from north) from us to a point. */
    fun distanceTo(lat: Double, lon: Double): Pair<Double, Double>? {
        val me = myFix ?: return null
        val p = object {
            val lat = lat
            val lon = lon
        }
        val r = 6_371_000.0
        val la1 = Math.toRadians(me.lat)
        val lo1 = Math.toRadians(me.lon)
        val la2 = Math.toRadians(p.lat)
        val lo2 = Math.toRadians(p.lon)
        val dla = la2 - la1
        val dlo = lo2 - lo1
        val a = kotlin.math.sin(dla / 2).let { it * it } + cos(la1) * cos(la2) * kotlin.math.sin(dlo / 2).let { it * it }
        val d = 2 * r * atan2(sqrt(a), sqrt(1 - a))
        val y = kotlin.math.sin(dlo) * cos(la2)
        val x = cos(la1) * kotlin.math.sin(la2) - kotlin.math.sin(la1) * cos(la2) * cos(dlo)
        val b = (Math.toDegrees(atan2(y, x)) + 360) % 360
        return d to b
    }

    // ----- media (protocol.md §13) -----

    fun estimate(chat: ChatId, datagramLen: UInt): Estimate? {
        val key = chat.contactKey ?: return null
        return runCatching { core.estimateDatagram(key, datagramLen) }.getOrNull()
    }

    /** What a voice message of `datagramLen` bytes costs: on the path to a contact, or our own
     * floods to a group (both copies, §13.1). */
    fun voiceEstimate(chat: ChatId, datagramLen: UInt): Estimate? =
        if (chat.group) core.estimateGroupDatagram(datagramLen) else estimate(chat, datagramLen)

    fun sendVoice(voice: PreparedVoice, chat: ChatId): String? {
        if (chat.group) return sendMedia(chat) { at, now -> core.sendGroupVoice(chat.hex.unhex(), voice, at, now) }
        val key = chat.contactKey ?: return LichenException.MediaInGroup().display(res)
        return sendMedia(chat) { at, now -> core.sendVoice(key, voice, at, now) }
    }

    /** A small file over LoRa (§13.2, ADR-0022), compressed when smaller (ADR-0023): up to 8 KB on the air, to a contact. */
    fun sendFile(data: ByteArray, name: String, chat: ChatId): String? {
        val key = chat.contactKey ?: return LichenException.MediaInGroup().display(res)
        return sendMedia(chat) { at, now -> core.sendFile(key, name, data, at, now) }
    }

    /**
     * How a small file would go over LoRa to a contact: its size on the air (compressed when
     * smaller), fragments and airtime; null for a group, or when it does not fit.
     */
    fun loraFile(chat: ChatId, name: String, data: ByteArray): LoraFile? {
        val key = chat.contactKey ?: return null
        return runCatching { core.loraFile(key, name, data) }.getOrNull()
    }

    fun sendImage(image: PreparedImage, chat: ChatId): String? {
        val key = chat.contactKey ?: return LichenException.MediaInGroup().display(res)
        return sendMedia(chat) { at, now -> core.sendImage(key, image, at, now) }
    }

    private fun sendMedia(chat: ChatId, send: (UInt, ULong) -> StoredMessage): String? = try {
        val m = send((System.currentTimeMillis() / 1000).toUInt(), now())
        if (chats[chat] == null) load(chat) else upsert(m)
        conversations = core.conversations()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    fun requestFull(m: StoredMessage): String? = try {
        core.requestFullImage(m.id, now())
        requestedFull = requestedFull + m.id
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** The announcement that completes the LoRa preview `mediaId` in `chat`, if any. */
    fun announcement(mediaId: ByteArray, chat: ChatId): StoredMessage? =
        chats[chat]?.lastOrNull { (it.message as? Message.Announce)?.mediaId?.contentEquals(mediaId) == true }

    /** The LoRa preview an announcement completes, if it is in `chat`. */
    fun preview(mediaId: ByteArray, chat: ChatId): StoredMessage? =
        chats[chat]?.firstOrNull { (it.message as? Message.Image)?.mediaId?.contentEquals(mediaId) == true }

    // ----- Internet bridge (ADR-0015) -----

    private fun folder(name: String): File = File(dataDirectory() ?: app.filesDir, name).also { it.mkdirs() }

    /** This phone's bridge, keyed from the identity: iroh and Tor as the settings say. */
    private fun startBridge() {
        bridge = identitySeed?.let { seed -> runCatching { LichenBridge(seed, folder("offers").path) }.getOrNull() }
        applyIroh()
        applyTor()
    }

    /** A new identity, new bridge keys. */
    private fun restartBridge() {
        val old = bridge
        bridge = null
        bridgeOnline = false
        torStatus = TorStatus.OFF
        onionId = null
        scope.launch {
            old?.let {
                runCatching { it.stopIroh() }
                runCatching { it.stopTor() }
                it.close()
            }
            startBridge()
        }
    }

    fun chooseIroh(on: Boolean) {
        irohEnabled = on
        prefs.edit { putBoolean(IROH, on) }
        applyIroh()
    }

    fun chooseRelayOnly(on: Boolean) {
        relayOnly = on
        prefs.edit { putBoolean(RELAY_ONLY, on) }
        applyIroh(restart = true)
    }

    fun chooseTor(on: Boolean) {
        torEnabled = on
        prefs.edit { putBoolean(TOR, on) }
        applyTor()
    }

    /** Starts or stops iroh as the settings say; `bridgeOnline` once a relay answers.
     *
     * iroh runs only while it is needed: something we offer is still served (7 days), a send
     * sheet is open, or a download goes through it. Otherwise its relays would see the phone's
     * address and its lasting endpoint id at every launch, and anyone who once got an
     * announcement could see whether it is online (review of 2026-11-15, SR-29). */
    private fun applyIroh(restart: Boolean = false) {
        val b = bridge ?: return
        val wanted = irohEnabled && (bridgeDemand > 0 || runCatching { b.hasOffers() }.getOrDefault(false))
        val relays = relayOnly
        scope.launch {
            if (!wanted || restart) {
                runCatching { b.stopIroh() }
                bridgeOnline = false
            }
            if (!wanted) return@launch
            if (b.endpointId() == null) runCatching { b.startIroh(relays) }
            bridgeOnline = runCatching { b.online(30u) }.getOrDefault(false)
        }
    }

    /** Who needs iroh now: open send sheets and downloads through it. */
    private var bridgeDemand = 0

    /** A send sheet opened: iroh starts, so that it can be offered. */
    fun needBridge() {
        bridgeDemand++
        applyIroh()
    }

    /** The sheet closed: iroh stops unless something is still offered. */
    fun releaseBridge() {
        bridgeDemand = maxOf(0, bridgeDemand - 1)
        applyIroh()
    }

    /** Connects to Tor and publishes our onion service, or stops Tor, as the settings say. */
    private fun applyTor() {
        val b = bridge ?: return
        if (!torEnabled) {
            if (torStatus != TorStatus.OFF) {
                torStatus = TorStatus.OFF
                onionId = null
                scope.launch { runCatching { b.stopTor() } }
            }
            return
        }
        if (torStatus != TorStatus.OFF && torStatus != TorStatus.FAILED) return
        torStatus = TorStatus.CONNECTING
        demo?.let { runCatching { it.camilleOverTor(folder("demo-tor-state").path, folder("demo-tor-cache").path) } }
        scope.launch {
            try {
                val id = b.startTor(folder("tor-state").path, folder("tor-cache").path, true, 180u)
                if (!torEnabled) return@launch
                onionId = id
                torStatus = TorStatus.PUBLISHING
                val reachable = b.onionReachable(300u)
                if (!torEnabled) return@launch
                torStatus = if (reachable) TorStatus.READY else TorStatus.FAILED
            } catch (_: Exception) {
                if (torEnabled) torStatus = TorStatus.FAILED
            }
        }
    }

    private fun loadDownloads() {
        folder("downloads").listFiles()?.forEach { f ->
            val h = f.nameWithoutExtension
            if (h.length == 64) downloads[h] = f
        }
    }

    /** Where content of `hash` named `name` is kept: `downloads/<hash>.<extension>`. */
    private fun downloadFile(hash: ByteArray, name: String): File {
        // The name comes from whoever offered the file: only a plain extension is kept, so a
        // separator or a control character can never reach the path (review of 2026-11-15, SR-24).
        val ext = name.substringAfterLast('.', "").takeIf { e -> e.length in 1..10 && e.all { it.isLetterOrDigit() && it.code < 128 } }
            ?.lowercase() ?: "bin"
        return File(folder("downloads"), "${hash.hex()}.$ext")
    }

    private fun keep(bytes: ByteArray, hash: ByteArray, name: String): File {
        val f = downloadFile(hash, name)
        f.writeBytes(bytes)
        downloads[hash.hex()] = f
        return f
    }

    /** The providers we can put in an announcement: iroh unless `anonymous`, Tor when our
     * onion service is reachable. */
    fun bridgeProviders(anonymous: Boolean): List<Provider> = buildList {
        val id = bridge?.endpointId()
        if (!anonymous && bridgeOnline && id != null) add(Provider(1u, id))
        val onion = onionId
        if (torStatus == TorStatus.READY && onion != null) add(Provider(2u, onion))
    }

    /** Offers a photo on the bridge and announces it in one LoRa frame (§14.1); `anonymous`:
     * over Tor only, so the recipient never learns our IP address. */
    /** Offers `original` over the bridge and announces it over LoRa (§14.1): `kind` 1 a
     * photo, 3 any file (ADR-0022). */
    fun sendOverBridge(original: ByteArray, name: String, chat: ChatId, anonymous: Boolean, kind: UByte = 1u): String? {
        val providers = bridgeProviders(anonymous)
        val b = bridge
        if (b == null || providers.isEmpty()) return res.getString(R.string.no_internet_for_photo)
        val fitted = fitFileName(name, 32u)
        return try {
            val offered = b.offer(original)
            keep(original, offered.hash, fitted)
            val m = core.sendAnnounce(
                chat.ref, ByteArray(0), kind, offered.hash, offered.size, providers, fitted,
                (System.currentTimeMillis() / 1000).toUInt(), now(),
            )
            if (chats[chat] == null) load(chat) else upsert(m)
            conversations = core.conversations()
            pump()
            null
        } catch (e: LichenException) {
            e.display(res)
        } catch (e: BridgeException) {
            e.display(res)
        }
    }

    /** The providers of an announcement we would try, in order: iroh (when on), then Tor. */
    fun fetchRoute(providers: List<Provider>): List<Provider> = listOfNotNull(
        // iroh starts for the download when it is not running (SR-29).
        providers.firstOrNull { it.transport.toInt() == 1 }?.takeIf { irohEnabled },
        providers.firstOrNull { it.transport.toInt() == 2 },
    )

    /** Fetches announced content, checked against its hash: over iroh first, over Tor when
     * iroh cannot reach the sender (Tor connects on demand). */
    fun fetch(m: StoredMessage) {
        val a = m.message as? Message.Announce ?: return
        val b = bridge ?: return
        val h = a.hash.hex()
        if (fetching.containsKey(h)) return
        val route = fetchRoute(a.providers)
        val first = route.firstOrNull()
        if (first == null) {
            fetchErrors[h] = res.getString(R.string.no_transport_reaches)
            return
        }
        fetching[h] = first.transport.toInt()
        fetchErrors.remove(h)
        fetchJobs[h] = scope.launch {
            var failure = res.getString(R.string.download_failed)
            bridgeDemand++
            try {
                for (p in route) {
                    fetching[h] = p.transport.toInt()
                    try {
                        if (p.transport.toInt() == 1 && b.endpointId() == null) {
                            b.startIroh(relayOnly)
                            bridgeOnline = runCatching { b.online(30u) }.getOrDefault(false)
                        }
                        if (p.transport.toInt() == 2) {
                            b.startTor(folder("tor-state").path, folder("tor-cache").path, false, 180u)
                        }
                        // Straight to the file, verified before it appears: the content never
                        // sits in memory or crosses the bindings (review of 2026-11-15, SR-33).
                        val f = downloadFile(a.hash, a.name)
                        b.fetchToFile(p.transport, p.key, a.hash, a.size.toULong(), f.path)
                        downloads[h] = f
                        return@launch
                    } catch (e: BridgeException) {
                        failure = e.display(res)
                        // Only an unreachable sender is worth another transport.
                        if (e is BridgeException.Unreachable || e is BridgeException.Timeout) continue
                        break
                    }
                }
                fetchErrors[h] = failure
            } finally {
                fetching.remove(h)
                fetchJobs.remove(h)
                releaseBridge()
            }
        }
    }

    /** Stops a download: the original is offered again, without an error. */
    fun cancelFetch(m: StoredMessage) {
        val a = m.message as? Message.Announce ?: return
        val h = a.hash.hex()
        fetchJobs.remove(h)?.cancel()
        fetching.remove(h)
        fetchErrors.remove(h)
    }

    /** Airtime left this hour, milliseconds. */
    val airtimeLeftMs: ULong get() = budget.limitMs - minOf(budget.usedMs, budget.limitMs)

    // ----- lookups for the views -----

    fun contact(node: ByteArray): ContactInfo? = contacts.firstOrNull { it.key.contentEquals(node) }

    fun name(key: ByteArray): String {
        if (nodeKey?.contentEquals(key) == true) return res.getString(R.string.you)
        return contact(key)?.name ?: key.copyOf(4).hex()
    }

    fun title(chat: ChatId): String = when (chat.kind) {
        ChatId.Kind.GROUP -> groups.firstOrNull { it.gid.hex() == chat.hex }?.name ?: res.getString(R.string.group)
        ChatId.Kind.CHANNEL -> channel(chat)?.name ?: res.getString(R.string.channel)
        ChatId.Kind.CONTACT, ChatId.Kind.MESH -> name(chat.hex.unhex())
        ChatId.Kind.ROOM -> node(chat.hex.unhex())?.name ?: res.getString(R.string.room_server)
    }

    fun channel(chat: ChatId): MeshChannel? =
        if (chat.channel) channels.firstOrNull { it.id.hex() == chat.hex } else null

    // ----- MeshCore chats (protocol.md §15) -----

    /** The longest text for a plain chat, in bytes. */
    fun plainTextMax(chat: ChatId): Int = runCatching { core.plainTextMax(chat.ref).toInt() }.getOrDefault(160)

    /** Joins Public, a hashtag or a private channel (`secret`: 32 hex digits): the channel, or
     * an error message. */
    fun joinChannel(kind: ChannelKind, name: String = "", secret: String = ""): Result<ChatId> = try {
        val ref = when (kind) {
            ChannelKind.PUBLIC_CHANNEL -> core.joinPublicChannel()
            ChannelKind.HASHTAG_CHANNEL -> core.joinHashtagChannel(name)
            ChannelKind.PRIVATE_CHANNEL -> core.joinPrivateChannel(name, secret)
        }
        channels = core.channels()
        refreshChatPrefs()
        pump()
        Result.success(ChatId.of(ref))
    } catch (e: LichenException) {
        Result.failure(IllegalArgumentException(res.getString(when (e) {
            is LichenException.BadCode ->
                if (kind == ChannelKind.HASHTAG_CHANNEL) R.string.hashtag_rules else R.string.channel_secret_rules
            is LichenException.NoIdentity -> R.string.channel_needs_radio
            is LichenException.TooLarge -> R.string.channel_no_slot
            else -> R.string.channel_join_failed
        })))
    }

    fun leaveChannel(id: ByteArray) {
        runCatching { core.leaveChannel(id) }
        channels = core.channels()
        pump()
    }

    /** Our node's name as MeshCore users see it (§15.4); an error message, or null. */
    fun setMeshName(name: String): String? = try {
        core.setMeshName(name, now())
        nodeName = core.meshName()
        pump()
        null
    } catch (e: LichenException) {
        res.getString(R.string.mesh_name_rules)
    }

    fun group(chat: ChatId): GroupInfo? = if (chat.group) groups.firstOrNull { it.gid.hex() == chat.hex } else null

    /** Names a contact on this phone only; null or blank: its node's name again. */
    fun renameContact(node: ByteArray, name: String?) {
        runCatching { core.renameContact(node, name?.trim()?.ifEmpty { null }) }
        contacts = people()
        groups = core.groups()
        pump()
    }

    // ----- identity (ADR-0013) -----

    fun changeName(name: String) {
        myName = name
        prefs.edit { putString(MY_NAME, name) }
    }

    fun markBackedUp() {
        phraseBackedUp = true
        prefs.edit { putBoolean(BACKED_UP, true) }
    }

    val fingerprint: List<String> get() = core.fingerprint()

    fun recoveryPhrase(): List<String> = core.recoveryPhrase()

    /** Our contact card for the QR code; null until the radio is connected. */
    fun myCard(): String? = runCatching { core.contactCard(myName.ifEmpty { nodeName }) }.getOrNull()

    /** Adds the contact of a card: verified when scanned in person, not when pasted (§4.7); or
     * an error to show. */
    fun addCard(text: String, scanned: Boolean): Pair<ContactInfo?, String?> = try {
        val c = core.addContactCard(text.trim(), scanned)
        contacts = people()
        pump()
        c to null
    } catch (e: LichenException) {
        null to e.display(res)
    }

    /** The safety number with an identity (the contact's, or a new one it presented). */
    fun safetyNumberWith(identity: ByteArray): List<String>? = runCatching { core.safetyNumberWith(identity) }.getOrNull()

    /** Verifies (or not) `node`, whose identity `identity` the user compared; false when its
     * identity changed meanwhile (nothing verified, review PR-07). */
    fun setVerified(node: ByteArray, identity: ByteArray, verified: Boolean): Boolean {
        val done = runCatching { core.setVerified(node, identity, verified) }.getOrDefault(false)
        contacts = people()
        return done
    }

    /** Accepts the new identity `expected` the user compared, verified; false when another one
     * arrived meanwhile (compare again). */
    fun acceptIdentity(node: ByteArray, expected: ByteArray): Boolean {
        val done = runCatching { core.acceptIdentity(node, expected, true) }.getOrDefault(false)
        updateConflict(node)
        contacts = people()
        pump()
        return done
    }

    fun rejectIdentity(node: ByteArray, expected: ByteArray) {
        runCatching { core.rejectIdentity(node, expected) }
        updateConflict(node)
    }

    private fun updateConflict(node: ByteArray) {
        val pending = runCatching { core.pendingIdentity(node) }.getOrNull()
        conflicts = if (pending != null) conflicts + (node.hex() to pending) else conflicts - node.hex()
    }

    /** Restores an identity from its 24 words: the core restarts with it. */
    fun restore(words: List<String>): String? = try {
        val seed = seedFromPhrase(words)
        // Saved and read back before the app switches to it.
        identityStore.replace(seed)
        markBackedUp()
        useIdentity(seed)
        null
    } catch (e: LichenException) {
        e.display(res)
    } catch (_: IdentityWriteException) {
        res.getString(R.string.identity_not_saved)
    }

    // ----- encrypted backup (ADR-0037, docs/spec/backup.md) -----

    /** What making a backup came to. */
    sealed interface BackupOutcome {
        data class Made(val summary: BackupSummary) : BackupOutcome
        data class Failed(val reason: String) : BackupOutcome
    }

    /** The name the save dialog proposes. */
    fun backupFileName(): String = "lichen-${java.time.LocalDate.now()}.lichen-backup"

    /** Whether there is something of ours to back up (not the demo, an identity in use). */
    val canBackUp: Boolean get() = !isDemo && identityProblem == null

    /** The settings a backup carries (backup.md §4), under the names both apps use; the
     * appearance and the map style in lower case, as iOS writes them. */
    private fun backupSettings(): ByteArray = JSONObject().apply {
        put("lichen.myName", myName)
        put("lichen.tor", torEnabled)
        put("lichen.iroh", irohEnabled)
        put("lichen.relayOnly", relayOnly)
        put("lichen.gifAuto", gifAutoLoad)
        put("lichen.voiceMode", voiceMode)
        put("appearance", appearance.name.lowercase())
        prefs.getString(MAP_STYLE, null)?.let { put("map.style", it.lowercase()) }
    }.toString().toByteArray()

    /** The settings of a restored backup: the keys this app knows, with values of the right type. */
    private fun applyBackupSettings(bytes: ByteArray) {
        if (bytes.isEmpty()) return
        val o = runCatching { JSONObject(String(bytes)) }.getOrNull() ?: return
        (o.opt("lichen.myName") as? String)?.let { changeName(it) }
        (o.opt("lichen.tor") as? Boolean)?.let { chooseTor(it) }
        (o.opt("lichen.iroh") as? Boolean)?.let { chooseIroh(it) }
        (o.opt("lichen.relayOnly") as? Boolean)?.let { chooseRelayOnly(it) }
        (o.opt("lichen.gifAuto") as? Boolean)?.let { chooseGifAutoLoad(it) }
        (o.opt("lichen.voiceMode") as? Int)?.takeIf { it in listOf(0, 2, 3) }?.let { chooseVoiceMode(it) }
        (o.opt("appearance") as? String)?.let { v ->
            Appearance.entries.firstOrNull { it.name.equals(v, ignoreCase = true) }?.let { chooseAppearance(it) }
        }
        (o.opt("map.style") as? String)?.uppercase()?.takeIf { it in MAP_STYLES }?.let { v ->
            prefs.edit { putString(MAP_STYLE, v) }
        }
    }

    /** Writes a backup into [uri], the document the user created with the system's picker: made
     * in the cache first (the core writes a path), copied, then removed. Off the main thread; the
     * app keeps working meanwhile. A failed backup removes the document. */
    suspend fun exportBackup(uri: android.net.Uri): BackupOutcome {
        if (!canBackUp) return BackupOutcome.Failed(res.getString(R.string.backup_nothing))
        val c = core
        val settings = backupSettings()
        val tmp = File(app.cacheDir, "export.lichen-backup")
        return withContext(Dispatchers.IO) {
            val made = try {
                val summary = c.exportBackup(tmp.path, settings)
                app.contentResolver.openOutputStream(uri, "wt")?.use { out -> tmp.inputStream().use { it.copyTo(out) } }
                    ?: throw java.io.IOException("no output stream")
                BackupOutcome.Made(summary)
            } catch (e: LichenException) {
                BackupOutcome.Failed(e.display(res))
            } catch (_: java.io.IOException) {
                BackupOutcome.Failed(LichenException.Storage().display(res))
            } finally {
                tmp.delete()
            }
            if (made is BackupOutcome.Failed) {
                runCatching { android.provider.DocumentsContract.deleteDocument(app.contentResolver, uri) }
            }
            made
        }
    }

    /** Restores the backup at [uri] with its 24 words: its identity, messages, contacts, groups,
     * settings and media replace this phone's. Copied to the cache first, removed after. Null once
     * done, otherwise why not (nothing was changed). */
    suspend fun restoreBackup(uri: android.net.Uri, words: List<String>): String? {
        val dir = dataDirectory() ?: return LichenException.Storage().display(res)
        val c = core
        val copy = File(app.cacheDir, "restore.lichen-backup")
        val read: Result<RestoredBackup> = withContext(Dispatchers.IO) {
            try {
                val opened = app.contentResolver.openInputStream(uri)?.use { input ->
                    copy.outputStream().use { input.copyTo(it) }
                }
                if (opened == null) Result.failure(LichenException.NotABackup())
                else Result.success(c.restoreBackup(copy.path, words, dir.path))
            } catch (e: LichenException) {
                Result.failure(e)
            } catch (_: java.io.IOException) {
                Result.failure(LichenException.Storage())
            } finally {
                copy.delete()
            }
        }
        val restored = read.getOrElse { e -> return (e as? LichenException ?: LichenException.Storage()).display(res) }
        try {
            identityStore.replace(restored.identitySeed)
        } catch (_: IdentityWriteException) {
            // The data is in place, but the identity is not saved: the app goes on with its own,
            // its store open again.
            identitySeed?.let { useIdentity(it) }
            return res.getString(R.string.identity_not_saved)
        }
        markBackedUp()
        useIdentity(restored.identitySeed)
        applyBackupSettings(restored.settings)
        loadDownloads()
        return null
    }

    // ----- groups -----

    fun createGroup(name: String, members: List<ByteArray>): String? = try {
        core.createGroup(name, members, now())
        groups = core.groups()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    // ----- invitations by QR code (protocol.md §10.5) -----

    /** An invitation to a group we administer, for its QR code: 10 minutes, any number of
     * people, until revoked; or an error to show. */
    fun createInvite(gid: ByteArray): Pair<InviteCode?, String?> = try {
        val code = core.createInvite(gid, now())
        // Debug extra `inviteDemo`: Léa, next to you on the demo network, scans it.
        val d = demo
        if (debug.inviteDemo && d != null) scope.launch(demoContext) { d.guestScans(code.text) }
        code to null
    } catch (e: LichenException) {
        null to e.display(res)
    }

    /** The invitation's screen closed: its code admits no one any more. */
    fun revokeInvite(token: ByteArray) = core.revokeInvite(token)

    fun parseInvite(text: String): Pair<InviteInfo?, String?> = try {
        core.parseInvite(text.trim(), now()) to null
    } catch (e: LichenException) {
        null to if (e is LichenException.BadCode) res.getString(R.string.invite_unreadable) else e.display(res)
    }

    /** Asks the admin of an invitation to add us; the group arrives as `GroupJoined`. Only a
     * `scanned` invitation verifies the admin (second read of 2026-11-17, N1). */
    fun acceptInvite(text: String, info: InviteInfo, scanned: Boolean): String? = try {
        core.acceptInvite(text.trim(), scanned, now())
        joining = joining + (info.admin.hex() to info.groupName)
        contacts = people()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Invites contacts into a group we administer (ADR-0017). */
    fun addMembers(members: List<ByteArray>, gid: ByteArray): String? = try {
        core.addMembers(gid, members, now())
        groups = core.groups()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Leaves a group (closes it when we are its admin); the conversation stays. */
    fun leaveGroup(gid: ByteArray): String? = try {
        core.leaveGroup(gid, now())
        groups = core.groups()
        shares = core.shares()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /**
     * Hands a group we run to `member` (protocol.md §10.8): "Make admin", or, with `leave`, how
     * the admin leaves without closing the group.
     */
    fun handOver(gid: ByteArray, member: ByteArray, leave: Boolean): String? = try {
        core.handOver(gid, member, leave, now())
        groups = core.groups()
        if (leave) shares = core.shares()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    /** Renames a group we administer; its members see the change. */
    fun renameGroup(gid: ByteArray, name: String): String? = try {
        core.renameGroup(gid, name.trim(), now())
        groups = core.groups()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    fun removeMember(member: ByteArray, gid: ByteArray): String? = try {
        core.removeMember(gid, member, now())
        groups = core.groups()
        pump()
        null
    } catch (e: LichenException) {
        e.display(res)
    }

    companion object {
        /** A group's voice notes (ADR-0035): Codec2 700C, 6 seconds at most (5 fragments). */
        const val GROUP_VOICE_MODE = 0
        const val GROUP_VOICE_SECONDS = 6
        private const val APPEARANCE = "appearance"
        private const val MY_NAME = "lichen.myName"
        private const val BACKED_UP = "lichen.phraseBackedUp"
        private const val TCP_ADDRESS = "lichen.tcpAddress"
        private const val TCP_ACTIVE = "lichen.tcpActive"
        private const val IROH = "lichen.iroh"
        private const val RELAY_ONLY = "lichen.relayOnly"
        private const val TOR = "lichen.tor"
        private const val GIF_AUTO = "lichen.gifAuto"
        private const val ONBOARDED = "onboarded"
        private const val VOICE_MODE = "lichen.voiceMode"
        private const val MAP_STYLE = "map.style"
        /** MapStyle's names (ui/LichenMap.kt), for a restored setting. */
        private val MAP_STYLES = setOf("AUTO", "DARK", "LIBERTY", "POSITRON", "BRIGHT")
        /** Drafts by identity; the older ones (`lichen.draft.<route>`) are left unread. */
        private const val DRAFT = "lichen.draftById."
        private const val HISTORY_PAGE = 100u

        /** What the timer menu offers: off, 5 minutes, 1 hour, 8 hours, 1 day, 1 week. */
        val TIMER_CHOICES = listOf(0u, 300u, 3_600u, 28_800u, 86_400u, 604_800u)
    }
}
