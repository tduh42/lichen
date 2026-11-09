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
import com.tduhamel.lichen.core.InviteCode
import com.tduhamel.lichen.core.Budget
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.core.Conversation
import com.tduhamel.lichen.core.DemoNetwork
import com.tduhamel.lichen.core.Estimate
import com.tduhamel.lichen.core.GroupInfo
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
import java.io.File
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
    )

    /** Why our identity cannot be used: [IdentityProblemScreen] shows instead of the app. */
    enum class IdentityProblem {
        /** The Keystore cannot be used now: nothing was changed, try again. */
        UNAVAILABLE,

        /** What is stored is not an identity. */
        CORRUPT,

        /** A first identity could not be saved. */
        NOT_SAVED,
    }

    enum class TorStatus { OFF, CONNECTING, PUBLISHING, READY, FAILED }

    private val res get() = app.resources
    private val prefs = app.getSharedPreferences("lichen", Context.MODE_PRIVATE)
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
    var groups by mutableStateOf(listOf<GroupInfo>())

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

    /** The app is on screen: received messages need no notification. */
    var inForeground = false

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
    val tcp = TcpCompanion(::tcpChanged, ::frame)

    /** The last TCP companion address, to offer again. */
    var tcpAddress by mutableStateOf(prefs.getString(TCP_ADDRESS, null) ?: "10.0.2.2:5000")
        private set
    val ble = BleCompanion(app, ::bleChanged, { found = it }, ::frame)

    private var demo: DemoNetwork? = null
    private var demoJob: Job? = null
    private var wake: Job? = null
    private var debug = DebugOptions()

    /** Debug extra `joinNow`: a scanned invitation is joined at once. */
    val debugJoinNow: Boolean get() = debug.joinNow
    private var debugApplied = false
    private var debugVoiceSent = false

    init {
        refreshAll()
        Notifications.channels(app)
        if (ble.permitted()) ble.start()
        loadDownloads()
        startBridge()
        shares = core.shares()
        positions = core.positions(now())
        refreshLocationAccess()
        updateLocator()
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
        identityProblem = null
        return makeCore(seed, persistent = true)
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
        identityProblem = null
        replaceCore(makeCore(seed, persistent = true))
        refreshAll()
        shares = core.shares()
        positions = core.positions(now())
        restartBridge()
        updateLocator()
        radioChanged(radioState)
        pump()
    }

    private fun makeCore(seed: ByteArray, persistent: Boolean): LichenCore = LichenCore(
        "lichen-android",
        IdentityStore.randomBytes(32),
        seed,
        if (persistent) dataDirectory()?.path else null,
    )

    /** Per-node databases live in the app's private files. */
    private fun dataDirectory(): File? = File(app.filesDir, "lichen").takeIf { it.isDirectory || it.mkdirs() }

    private fun replaceCore(next: LichenCore) {
        val old = core
        core = next
        old.close()
    }

    private fun refreshAll() {
        contacts = people()
        groups = core.groups()
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
        if (usb.device != null || tcp.connectedTo != null) return
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
        if (s is RadioState.Connected || s is RadioState.Connecting) {
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

    fun connectTcp(address: String) {
        tcpAddress = address
        prefs.edit { putString(TCP_ADDRESS, address) }
        tcp.connect(address)
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
            tcp.connectedTo != null -> tcp.close()
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
                // The ongoing notification names the node, not the link's address.
                if (!isDemo) RadioService.start(app, e.name, shares.isNotEmpty() || sos)
            }
            is LichenEvent.IdentityLearned -> contacts = people()
            is LichenEvent.IdentityConflict -> conflicts = conflicts + (e.node.hex() to e.identity)
            LichenEvent.ContactsUpdated -> {
                contacts = people()
                debugInvites()
                debugFill()
                debugOpen()
                debugVoice()
                debugLocation()
            }
            is LichenEvent.SessionEstablished -> secured = secured + e.peer.hex()
            is LichenEvent.Received -> {
                lastSnr = lastSnr + (e.from.hex() to e.snrDb)
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
            is LichenEvent.GroupJoined, is LichenEvent.GroupUpdated, is LichenEvent.GroupLeft -> {
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
                val chat = ChatId.group(e.gid)
                if (!isMuted(chat)) notify(chat, title(chat), "${name(e.from)}: ${e.message.display(res)}")
            }
            is LichenEvent.MessageChanged -> {
                upsert(e.message)
                conversations = core.conversations()
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
                notify(chat, title(chat), app.getString(R.string.searching_for_you, name(e.by)))
                searchedBy = core.searchedBy(now())
            }
            is LichenEvent.SearchUpdated -> {
                searchVersion++
                refreshSearches()
            }
            // lc-ffi keeps the power to restore; we show it while it is lowered.
            is LichenEvent.TxPower -> loweredPower = if (e.previous == null) null else e.dbm
        }
    }

    /** A notification for a message received while the app is not on screen. */
    /** A notification for a message, unless its conversation is on screen. */
    private fun notify(chat: ChatId, title: String, text: String) {
        if (!(inForeground && visibleChat == chat)) Notifications.message(app, chat, title, text)
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

    private fun debugOpen() {
        val name = debug.open ?: return
        val chat = contacts.firstOrNull { it.name == name }?.let { ChatId.contact(it.key) }
            ?: groups.firstOrNull { it.name == name }?.let { ChatId.group(it.gid) }
            ?: return
        debug = debug.copy(open = null)
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

    /** The text typed in `chat` and not sent, kept across screens and restarts. */
    fun draft(chat: ChatId): String = prefs.getString(DRAFT + chat.route, "") ?: ""

    fun setDraft(chat: ChatId, text: String) {
        prefs.edit { if (text.isEmpty()) remove(DRAFT + chat.route) else putString(DRAFT + chat.route, text) }
    }

    /** Loads earlier pages until message `id` is there (a search result), 20 pages at most. */
    fun loadUntil(chat: ChatId, id: Long) {
        if (chats[chat] == null) load(chat)
        var pages = 0
        while (chats[chat]?.none { it.id == id } == true && olderAvailable[chat] == true && pages < 20) {
            loadOlder(chat)
            pages++
        }
    }

    // ----- chat essentials (pins, mutes, search) -----

    fun refreshChatPrefs() {
        chatPrefs = core.chatPrefs().associateBy { ChatId.of(it.chat) }
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
        val key = chat.contactKey ?: return null
        return runCatching { core.estimateText(key, len) }.getOrNull()
    }

    /** Sends `text`; returns an error message to show, or null. */
    fun send(text: String, chat: ChatId): String? {
        val sentAt = (System.currentTimeMillis() / 1000).toUInt()
        return try {
            val m = if (chat.group) {
                core.sendGroupText(chat.hex.unhex(), text, sentAt, now())
            } else {
                core.sendText(chat.hex.unhex(), text, sentAt, now())
            }
            if (chats[chat] == null) load(chat) else upsert(m)
            conversations = core.conversations()
            pump()
            null
        } catch (e: LichenException) {
            e.display(res)
        }
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

    fun sendVoice(voice: PreparedVoice, chat: ChatId): String? {
        val key = chat.contactKey ?: return LichenException.MediaInGroup().display(res)
        return sendMedia(chat) { at, now -> core.sendVoice(key, voice, at, now) }
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

    /** Starts or stops iroh as the settings say; `bridgeOnline` once a relay answers. */
    private fun applyIroh(restart: Boolean = false) {
        val b = bridge ?: return
        val on = irohEnabled
        val relays = relayOnly
        scope.launch {
            if (!on || restart) {
                runCatching { b.stopIroh() }
                bridgeOnline = false
            }
            if (!on) return@launch
            if (b.endpointId() == null) runCatching { b.startIroh(relays) }
            bridgeOnline = runCatching { b.online(30u) }.getOrDefault(false)
        }
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

    private fun keep(bytes: ByteArray, hash: ByteArray, name: String): File {
        val ext = name.substringAfterLast('.', "").ifEmpty { "bin" }
        val f = File(folder("downloads"), "${hash.hex()}.$ext")
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
    fun sendOverBridge(original: ByteArray, name: String, chat: ChatId, anonymous: Boolean): String? {
        val providers = bridgeProviders(anonymous)
        val b = bridge
        if (b == null || providers.isEmpty()) return res.getString(R.string.no_internet_for_photo)
        return try {
            val offered = b.offer(original)
            keep(original, offered.hash, name)
            val m = core.sendAnnounce(
                chat.ref, ByteArray(0), 1u, offered.hash, offered.size, providers, name,
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
        providers.firstOrNull { it.transport.toInt() == 1 }?.takeIf { irohEnabled && bridgeOnline },
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
            try {
                for (p in route) {
                    fetching[h] = p.transport.toInt()
                    try {
                        if (p.transport.toInt() == 2) {
                            b.startTor(folder("tor-state").path, folder("tor-cache").path, false, 180u)
                        }
                        val bytes = b.fetch(p.transport, p.key, a.hash, a.size.toULong())
                        keep(bytes, a.hash, a.name)
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

    fun title(chat: ChatId): String = if (chat.group) {
        groups.firstOrNull { it.gid.hex() == chat.hex }?.name ?: res.getString(R.string.group)
    } else {
        name(chat.hex.unhex())
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

    /** Adds the contact of a scanned card (verified: scanned in person); or an error to show. */
    fun addCard(text: String): Pair<ContactInfo?, String?> = try {
        val c = core.addContactCard(text.trim())
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

    /** Asks the admin of a scanned invitation to add us (they become a verified contact); the
     * group arrives as `GroupJoined`. */
    fun acceptInvite(text: String, info: InviteInfo): String? = try {
        core.acceptInvite(text.trim(), now())
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
        private const val APPEARANCE = "appearance"
        private const val MY_NAME = "lichen.myName"
        private const val BACKED_UP = "lichen.phraseBackedUp"
        private const val TCP_ADDRESS = "lichen.tcpAddress"
        private const val IROH = "lichen.iroh"
        private const val RELAY_ONLY = "lichen.relayOnly"
        private const val TOR = "lichen.tor"
        private const val ONBOARDED = "onboarded"
        private const val VOICE_MODE = "lichen.voiceMode"
        private const val DRAFT = "lichen.draft."
        private const val HISTORY_PAGE = 100u
    }
}
