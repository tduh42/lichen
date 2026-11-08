package com.tduhamel.lichen.model

import android.app.Application
import android.content.Context
import android.os.SystemClock
import androidx.core.content.edit
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.tduhamel.lichen.R
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
import com.tduhamel.lichen.core.StoredMessage
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** The app's appearance setting: the system's by default. */
enum class Appearance { SYSTEM, DARK, LIGHT }

/** The app side of the lichen core: runs it (frames in and out, timer), keeps the UI state.
 * The Kotlin counterpart of the iOS app's `AppModel`; everything runs on the main thread. */
class AppModel(private val app: Application) {
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
    )

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
    var tab by mutableStateOf(Tab.MESSAGES)

    /** A chat to open (debug extra `open`, notifications later). */
    var openChat by mutableStateOf<ChatId?>(null)
    var debugScreen by mutableStateOf<String?>(null)

    var appearance by mutableStateOf(
        runCatching { Appearance.valueOf(prefs.getString(APPEARANCE, null) ?: "") }.getOrDefault(Appearance.SYSTEM),
    )
        private set

    var core: LichenCore = makeCore(identityStore.seed(), persistent = true)
        private set
    var link: RadioLink? = null

    private var demo: DemoNetwork? = null
    private var demoJob: Job? = null
    private var wake: Job? = null
    private var debug = DebugOptions()
    private var debugApplied = false

    init {
        refreshAll()
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
        when (options.tab) {
            "map" -> tab = Tab.MAP
            "search" -> tab = Tab.SEARCH
            "settings" -> tab = Tab.SETTINGS
        }
        if (options.demo) startDemo()
    }

    // ----- demo network -----

    /** Runs the app on a simulated MeshCore network (two simulated users, a group), in real time. */
    fun startDemo() {
        if (isDemo) return
        resetState()
        replaceCore(makeCore(IdentityStore.randomBytes(32), persistent = false))
        demo = DemoNetwork()
        isDemo = true
        radioState = RadioState.Connected("ThinkNode M5")
        core.connect(now())
        demoJob = scope.launch {
            while (isActive) {
                demoStep()
                delay(200)
            }
        }
        pump()
    }

    fun stopDemo() {
        if (!isDemo) return
        demoJob?.cancel()
        demoJob = null
        demo?.close()
        demo = null
        isDemo = false
        resetState()
        replaceCore(makeCore(identityStore.seed(), persistent = true))
        radioState = RadioState.Idle
        refreshAll()
    }

    private fun demoStep() {
        val d = demo ?: return
        val t = now()
        for (f in d.read(t)) core.onCompanionFrame(f, t)
        pump()
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
    }

    val isConnected: Boolean get() = radioState is RadioState.Connected

    // ----- runner -----

    fun now(): ULong = (SystemClock.elapsedRealtime() - started).toULong()

    /** The radio link changed state (BLE or USB). */
    fun radioChanged(state: RadioState) {
        if (isDemo) return
        radioState = state
        if (state is RadioState.Connected) {
            core.connect(now())
            pump()
        }
    }

    /** A frame from the radio. */
    fun frame(data: ByteArray) {
        core.onCompanionFrame(data, now())
        pump()
    }

    /** Writes due frames, reads events, and sets the timer for the next wake-up. */
    fun pump() {
        val t = now()
        core.setClock(System.currentTimeMillis().toULong(), t)
        core.tick(t)
        while (true) {
            val f = core.pollCompanion(t) ?: break
            val d = demo
            if (d != null) d.write(f, t) else link?.write(f)
        }
        while (true) {
            handle(core.pollEvent() ?: break)
        }
        // Sender keys arrive without an event: refresh the groups when they changed.
        val g = core.groups()
        if (signature(g) != signature(groups)) groups = g
        budget = core.budget(t)
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
            }
            is LichenEvent.IdentityLearned -> contacts = people()
            is LichenEvent.IdentityConflict -> conflicts = conflicts + (e.node.hex() to e.identity)
            LichenEvent.ContactsUpdated -> {
                contacts = people()
                debugOpen()
            }
            is LichenEvent.SessionEstablished -> secured = secured + e.peer.hex()
            is LichenEvent.Received -> {
                lastSnr = lastSnr + (e.from.hex() to e.snrDb)
                debug.say?.let { say ->
                    debug = debug.copy(say = null)
                    send(say, ChatId.contact(e.from))
                }
            }
            is LichenEvent.Progress, is LichenEvent.Delivered, is LichenEvent.Failed -> Unit
            is LichenEvent.GroupJoined, is LichenEvent.GroupUpdated, is LichenEvent.GroupLeft -> {
                groups = core.groups()
                contacts = people()
                debugOpen()
            }
            is LichenEvent.GroupMessage -> lastSnr = lastSnr + (e.from.hex() to e.snrDb)
            is LichenEvent.MessageChanged -> {
                upsert(e.message)
                conversations = core.conversations()
            }
            is LichenEvent.Location, LichenEvent.SharesChanged, is LichenEvent.MediaRequested -> Unit
        }
    }

    /** Debug extra `open`: the conversation with this contact or group, once it exists. */
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

    /** Loads the history of `chat` (when its screen opens). */
    fun load(chat: ChatId) {
        chats[chat] = runCatching { core.history(chat.ref, null, 200u) }.getOrDefault(emptyList())
    }

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

    fun estimate(chat: ChatId, text: String): Estimate? {
        val key = chat.contactKey ?: return null
        if (text.isEmpty()) return null
        return runCatching { core.estimateText(key, text.toByteArray().size.toUInt()) }.getOrNull()
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

    /** Sends a failed text again (a new message). */
    fun resend(m: StoredMessage): String? = when (val msg = m.message) {
        is com.tduhamel.lichen.core.Message.Text -> send(msg.text, ChatId.of(m.chat))
        else -> null
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

    companion object {
        private const val APPEARANCE = "appearance"
    }
}
