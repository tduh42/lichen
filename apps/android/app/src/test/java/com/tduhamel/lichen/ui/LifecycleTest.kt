package com.tduhamel.lichen.ui

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.ServerLogin
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.Notifications
import com.tduhamel.lichen.model.server
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Area 10 of docs/testing.md: lifecycle. */
@RunWith(AndroidJUnit4::class)
class LifecycleTest : UiTest() {
    private fun notes(): List<Notification> =
        shadowOf(context.getSystemService(NotificationManager::class.java)).allNotifications

    private fun route(n: Notification): String? =
        n.contentIntent?.let { shadowOf(it).savedIntent.getStringExtra(Notifications.EXTRA_CHAT) }

    private fun noteFor(chat: ChatId): Notification? = notes().lastOrNull { route(it) == chat.route }

    private fun allowNotifications() =
        shadowOf(context as Application).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)

    /** A row of Messages, held: its menu. */
    private fun hold(row: String) {
        compose.onAllNodesWithText(row).onFirst().performTouchInput { longClick() }
        compose.waitForIdle()
    }

    private fun top(t: String) = compose.onAllNodesWithText(t, useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot.top

    // The demo's conversations (null until there).
    private fun camille() = model.contacts.firstOrNull { it.name == "Camille" }?.let { ChatId.contact(it.key) }

    private fun marc() = model.contacts.firstOrNull { it.name == "Marc" }?.let { ChatId.mesh(it.key) }

    private fun public() = model.channels.firstOrNull { it.name == "Public" }?.let { ChatId.channel(it.id) }

    private fun group() = model.groups.firstOrNull { it.name == "Rando Vercors" }?.let { ChatId.group(it.gid) }

    private fun last(chat: ChatId?) = chat?.let { model.last(it) }

    /** After a restart: the demo radio back, with its contacts. */
    private fun reconnected() = advanceUntil("the radio back", 60) { model.isConnected && model.contacts.isNotEmpty() }

    private fun texts(chat: ChatId?) = chat?.let { model.chats[it] }.orEmpty().mapNotNull { (it.message as? Message.Text)?.text }

    /** Logged in to the demo's board (the Nodes tab), back on Messages. */
    private fun logInToTheBoard(): ChatId {
        advanceUntil("the room server") { model.nodes.any { it.name == "Refuge board" } }
        val board = model.nodes.first { it.name == "Refuge board" }
        tab(R.string.tab_nodes)
        tap("Refuge board")
        scrollTo(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("hello")
        tap(R.string.log_in)
        advanceUntil("logged in", 30) { model.server(board.key)?.login == ServerLogin.READ_WRITE }
        tapDesc(R.string.back)
        tab(R.string.tab_messages)
        return ChatId.room(board.key)
    }

    /** LC-1: each kind of conversation's notification opens that conversation: lichen, MeshCore,
     * channel, room, group. */
    @Test
    fun LC_1_notificationOpensTheRightChat() = notifications()

    @Test
    @Config(qualifiers = "+fr")
    fun LC_1_notificationOpensTheRightChat_fr() = notifications()

    private fun notifications() {
        allowNotifications()
        launch("demo" to true)
        // Public, if muted (a channel the app joins starts muted): on.
        advanceUntil("Public", 60) { public() != null && has("Public") }
        if (model.isMuted(public()!!)) {
            hold("Public")
            tapInDialog(R.string.unmute)
        }
        val room = logInToTheBoard()
        leave()
        val kinds = mutableMapOf<String, ChatId>()
        advanceUntil("a notification of each kind", 600, { notes().map { route(it) } }) {
            camille()?.let { kinds["lichen"] = it }
            marc()?.let { kinds["MeshCore"] = it }
            public()?.let { kinds["channel"] = it }
            group()?.let { kinds["group"] = it }
            kinds["room"] = room
            kinds.size == 5 && kinds.values.all { noteFor(it) != null }
        }
        for ((kind, chat) in kinds) {
            launchWith(shadowOf(noteFor(chat)!!.contentIntent).savedIntent)
            assertEquals(kind, chat, model.visibleChat)
            text(R.string.message_placeholder).assertIsDisplayed()
            text(model.title(chat)).assertIsDisplayed()
        }
    }

    /** LC-2: muted, no notification; archived, back with a message unless muted; pinned, on top. */
    @Test
    fun LC_2_muteArchivePin() = muteArchivePin()

    @Test
    @Config(qualifiers = "+fr")
    fun LC_2_muteArchivePin_fr() = muteArchivePin()

    private fun muteArchivePin() {
        allowNotifications()
        launch("demo" to true)
        // Public muted: its next posts come without a notification.
        advanceUntil("Public's first post", 60) { last(public()) != null && has("Public") }
        val public = public()!!
        hold("Public")
        tap(R.string.mute)
        tapInDialog(R.string.mute_always)
        assertTrue(model.isMuted(public))
        // The shade cleared: only what comes from now on.
        val shade = context.getSystemService(NotificationManager::class.java)
        shade.cancelAll()
        // Its posts come without a notification, except one that mentions our radio (§15.9,
        // Lucie's at 110 s), notified even muted.
        var seen = last(public)?.id
        var quiet = false
        advanceUntil("a Public post that does not mention us", 600) {
            val post = last(public)
            if (post != null && post.id != seen) {
                seen = post.id
                if (post.mentionsUs) {
                    assertNotNull("a mention, muted: still notified", noteFor(public))
                } else {
                    assertNull("no notification", noteFor(public))
                    quiet = true
                }
                shade.cancelAll()
            }
            quiet
        }
        // The group archived: back in Messages with its next post.
        advanceUntil("Julien's post", 300) { last(group()) != null }
        val group = group()!!
        hold("Rando Vercors")
        tap(R.string.archive)
        assertTrue(model.chatPrefs[group]?.archived == true)
        see(R.string.archived)
        // Public archived too, muted: it stays archived with its posts.
        hold("Public")
        tap(R.string.archive)
        seen = last(public)?.id
        advanceUntil("another Public post", 400) { last(public)?.id != seen }
        assertTrue(model.chatPrefs[public]?.archived == true)
        advanceUntil("the group back", 600) { model.chatPrefs[group]?.archived != true }
        // Marc pinned: on top, above newer conversations.
        hold("Marc")
        tap(R.string.pin)
        assertTrue(top("Marc") < top("Rando Vercors"))
        assertTrue(top("Marc") < top("Camille"))
    }

    /** LC-3: a conversation deleted: gone with its messages, the contact kept; her next message
     * starts it again, without the old ones. */
    @Test
    fun LC_3_deleteAConversation() = delete()

    @Test
    @Config(qualifiers = "+fr")
    fun LC_3_deleteAConversation_fr() = delete()

    private fun delete() {
        launch("demo" to true)
        advanceUntil("her hello", 60) { last(camille()) != null }
        val chat = camille()!!
        hold("Camille")
        tap(R.string.delete_conversation)
        tapInDialog(R.string.delete)
        assertFalse("her row gone: ${shown()}", has("Camille"))
        assertNull("her messages gone", model.last(chat))
        assertTrue("the contact kept", model.contacts.any { it.name == "Camille" })
        // Her next message (her photo) starts it again, alone.
        advanceUntil("her next message", 400) { model.last(chat) != null }
        see("Camille")
        tap("Camille")
        assertTrue(texts(chat).none { it.startsWith("Hi! I'm at the refuge") })
    }

    /** LC-4: a restart keeps messages, contacts, groups, preferences and a waiting request. On a
     * radio over TCP: the demo starts afresh at each launch, by design. */
    @Test
    fun LC_4_restartKeepsEverything(): Unit = FakeTcpRadio().use { radio ->
        launch("onboarding" to false, "tcp" to radio.address)
        waitReal("the radio's contacts") { model.contacts.any { it.name == "Marc" } }
        // Ours: a message to Marc, a group, Marc's chat pinned.
        assertEquals(null, model.send("Two of us tonight", marc()!!))
        model.createGroup("Ridge walk", emptyList())
        model.setPinned(marc()!!, true)
        // Camille's first message: a request (requests are on outside the demo).
        waitReal("Camille's request", 90_000) { model.requests.isNotEmpty() }
        val contacts = model.contacts.map { it.name }.sorted()
        val hisLast = last(marc())?.id
        val request = model.requests.single().name
        // The process killed: its link goes with it (closed here as Disconnect closes it).
        model.disconnect()
        waitReal("the link closed") { !model.isConnected }
        restart("onboarding" to false, "tcp" to radio.address)
        // Before the radio is back: the conversations and the request.
        assertEquals(hisLast, model.conversations.firstOrNull { ChatId.of(it.chat) == marc() }?.last?.id)
        assertEquals(request, model.requests.singleOrNull()?.name)
        waitReal("connected again") { model.isConnected }
        assertEquals(contacts, model.contacts.map { it.name }.sorted())
        assertEquals(hisLast, last(marc())?.id)
        assertTrue(model.groups.any { it.name == "Ridge walk" })
        assertNotNull(model.chatPrefs[marc()!!]?.pinnedAtMs)
        see(str(R.string.request_title, request))
        assertTrue(top("Marc") < top("Ridge walk"))
        Unit
    }

    /** LC-5: away for minutes: what came meanwhile is there, once. */
    @Test
    fun LC_5_backgroundAndBack() = away()

    @Test
    @Config(qualifiers = "+fr")
    fun LC_5_backgroundAndBack_fr() = away()

    private fun away() {
        launch("demo" to true, "open" to "Camille")
        advanceUntil("her first message", 60) { last(camille()) != null }
        leave()
        advance(240)
        comeBack()
        val got = model.chats[camille()!!].orEmpty()
        assertTrue("more came meanwhile", got.size > 1)
        assertEquals("each once", got.map { it.id }.distinct(), got.map { it.id })
        for (t in texts(camille())) {
            assertEquals(t, 1, compose.onAllNodesWithText(t, useUnmergedTree = true).fetchSemanticsNodes().size)
        }
    }

    /** LC-6: Marc, whom the radio files as a repeater (not among its contacts), keeps his row
     * in Messages. */
    @Test
    fun LC_6_everyConversationListed() {
        launch("demo" to true, "marcRepeater" to true)
        advanceUntil("Marc's message", 60) { has("Marc from the refuge", substring = true) }
        text("Marc").assertIsDisplayed()
        assertFalse(model.contacts.any { it.name == "Marc" })
    }
}
