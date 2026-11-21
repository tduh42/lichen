package com.tduhamel.lichen.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.ServerLogin
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.heard
import com.tduhamel.lichen.model.hopsBadge
import com.tduhamel.lichen.model.server
import com.tduhamel.lichen.model.trace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Area 3 of docs/testing.md: contacts, nodes, rooms, repeaters, requests, blocks. */
@RunWith(AndroidJUnit4::class)
class ContactsTest : UiTest() {
    /** Where a name is in the New message sheet (not in Messages behind it). */
    private fun top(t: String): Float {
        val sheet = hasScrollAction() and hasAnyDescendant(hasText(str(R.string.contacts)))
        return compose.onAllNodes(hasText(t) and hasAnyAncestor(sheet), useUnmergedTree = true).onFirst()
            .fetchSemanticsNode().boundsInRoot.top
    }

    /** Long-presses the row of that text (Messages' menu). */
    private fun longPress(t: String) {
        compose.onAllNodesWithText(t).onFirst().performTouchInput { longClick() }
        compose.waitForIdle()
    }

    /** CT-1: the people, lichen first, each with its kind and hops; sorted as people read. */
    @Test
    fun CT_1_contactsList() = contactsList()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_1_contactsList_fr() = contactsList()

    private fun contactsList() {
        launch("demo" to true)
        // Their identities known (their first lichen messages): lichen contacts from then on.
        advanceUntil("Camille's and Julien's identities", 180) {
            model.contacts.count { (it.name == "Camille" || it.name == "Julien") && it.identity != null } == 2
        }
        val camille = contact("Camille")
        val julien = contact("Julien")
        val marc = contact("Marc")
        tapDesc(R.string.new_message)
        text(R.string.contacts).assertIsDisplayed()
        // People only: no repeater, no room server.
        assertFalse(has("hub"))
        assertFalse(has("crest"))
        assertFalse(has("Refuge board"))
        val res = context.resources
        see(str(R.string.lichen_contact, hopsBadge(res, camille.hops)))
        see(str(R.string.lichen_contact, hopsBadge(res, julien.hops)))
        see(str(R.string.meshcore_contact, hopsBadge(res, marc.hops)))
        // lichen people first, by name; then the MeshCore ones.
        assertTrue(top("Camille") < top("Julien"))
        assertTrue(top("Julien") < top("Marc"))
    }

    /** CT-1: names sorted as people read them, accents and case aside. */
    @Test
    fun CT_1_contactsSortedAsPeopleReadThem() {
        launch("demo" to true)
        model.renameContact(contact("Camille").key, "zoé")
        model.renameContact(contact("Julien").key, "Émile")
        compose.waitForIdle()
        tapDesc(R.string.new_message)
        assertTrue("Émile before zoé", top("Émile") < top("zoé"))
    }

    /** CT-2: a nickname shows in Messages, the chat, the contacts and notifications; reset, the
     * node's name again. */
    @Test
    fun CT_2_renameAContact() = rename()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_2_renameAContact_fr() = rename()

    private fun rename() {
        launch("demo" to true)
        val camille = contact("Camille")
        advanceUntil("Camille's first message") { model.last(ChatId.contact(camille.key)) != null }
        longPress("Camille")
        tapInDialog(R.string.rename)
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("Cam from the refuge")
        tapInDialog(R.string.save)
        // Messages, the chat, the contacts, the name notifications take.
        text("Cam from the refuge").assertIsDisplayed()
        assertFalse(has("Camille"))
        assertEquals("Cam from the refuge", model.name(camille.key))
        tap("Cam from the refuge")
        assertTrue(has("Cam from the refuge"))
        tapDesc(R.string.back)
        tapDesc(R.string.new_message)
        assertTrue(has("Cam from the refuge"))
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("Cam")
        tap("Cam from the refuge")
        tapDesc(R.string.back)

        // Reset: the node's name again.
        longPress("Cam from the refuge")
        tapInDialog(R.string.rename)
        tapInDialog(R.string.use_node_name, "Camille")
        text("Camille").assertIsDisplayed()
        assertFalse(has("Cam from the refuge"))
        assertEquals("Camille", model.name(camille.key))
    }

    /** CT-3: the nodes the radio knows or heard, each with its kind, position, last heard. */
    @Test
    fun CT_3_nodes() {
        // The crest is one hop away: singular (B-and-4).
        assertEquals("1 hop", plural(R.plurals.hops_n, 1, 1))
        nodes()
    }

    @Test
    @Config(qualifiers = "+fr-night")
    fun CT_3_nodes_frDark() {
        assertEquals("1 saut", plural(R.plurals.hops_n, 1, 1))
        assertEquals("1 connu", plural(R.plurals.nodes_known, 1, 1))
        nodes()
    }

    private fun nodes() {
        launch("demo" to true)
        advanceUntil("the radio's nodes and Lucie's advert", 20) {
            model.nodes.any { it.name == "hub" } && model.nodes.any { it.name == "Lucie" }
        }
        tab(R.string.tab_settings)
        see(pluralSafe(model.nodes.size))
        tap(R.string.nodes)
        text("hub").assertIsDisplayed()
        text("crest").assertIsDisplayed()
        val crest = model.nodes.first { it.name == "crest" }
        if (crest.hops?.toInt() == 1) see(plural(R.plurals.hops_n, 1, 1), substring = true)
        text("Refuge board").assertIsDisplayed()
        text("Lucie").assertIsDisplayed()
        // Kinds in the rows; Lucie heard, not added.
        assertTrue(has(str(R.string.node_kind_repeater), substring = true))
        assertTrue(has(str(R.string.node_kind_room), substring = true))
        text(R.string.heard_badge).assertIsDisplayed()
        // The hub: a repeater, its advertised position, when last heard.
        val hub = model.nodes.first { it.name == "hub" }
        assertEquals(NodeKind.REPEATER, hub.kind)
        tap("hub")
        text(R.string.advertised_position).assertIsDisplayed()
        text("%.5f, %.5f".format(hub.lat, hub.lon)).assertIsDisplayed()
        text(str(R.string.last_heard, model.heard(hub.lastHeardS)), substring = true).assertIsDisplayed()
    }

    private fun pluralSafe(n: Int) = plural(R.plurals.nodes_known, n, n)

    /** The MeshCore public key row of a contact or node: the key in groups of 8; tapped, its
     * 64 hex digits on the clipboard and "Copied" (F-1). */
    private fun copiesKey(key: ByteArray) {
        val hex = key.joinToString("") { "%02x".format(it) }
        see(hex.chunked(8).joinToString(" "))
        tap(R.string.meshcore_public_key)
        assertEquals(hex, clipboardText())
        see(R.string.copied)
    }

    /** CT-2 (F-1): a contact's MeshCore public key, copied from their screen: Camille's (her
     * key's sheet) and Marc's (his MeshCore chat's details). */
    @Test
    fun CT_2_contactsMeshcorePublicKey() = contactKeys()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_2_contactsMeshcorePublicKey_fr() = contactKeys()

    private fun contactKeys() {
        launch("demo" to true)
        advanceUntil("Camille's key and Marc's hello", 120) {
            model.contacts.any { it.name == "Camille" && it.identity != null } && has("Marc")
        }
        tap("Camille")
        tapDesc(R.string.verify_their_key)
        copiesKey(contact("Camille").key)
        back()
        tapDesc(R.string.back)
        tap("Marc")
        tapDesc(R.string.about_meshcore_chats)
        copiesKey(contact("Marc").key)
    }

    /** CT-3 (F-1): a node's MeshCore public key, copied from its screen (Settings › Nodes). */
    @Test
    fun CT_3_nodeMeshcorePublicKey() = nodeKey()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_3_nodeMeshcorePublicKey_fr() = nodeKey()

    private fun nodeKey() {
        launch("demo" to true)
        advanceUntil("the radio's nodes") { model.nodes.any { it.name == "hub" } }
        tab(R.string.tab_settings)
        tap(R.string.nodes)
        tap("hub")
        copiesKey(model.nodes.first { it.name == "hub" }.key)
    }

    /** CT-4: the room server: logged in, its posts read, a post sent; its row in Messages. */
    @Test
    fun CT_4_roomLogInReadPost() = room()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_4_roomLogInReadPost_fr() = room()

    private fun room() {
        launch("demo" to true)
        advanceUntil("the room server") { model.nodes.any { it.name == "Refuge board" } }
        val board = model.nodes.first { it.name == "Refuge board" }
        tab(R.string.tab_settings)
        tap(R.string.nodes)
        tap("Refuge board")
        scrollTo(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("hello")
        tap(R.string.log_in)
        advanceUntil("logged in to the board", 30) { model.server(board.key)?.login?.let { it != ServerLogin.PENDING && it != ServerLogin.OUT } == true }
        assertEquals(ServerLogin.READ_WRITE, model.server(board.key)?.login)
        see(R.string.login_read_write)
        tap(R.string.open_room)
        // The board pushes its posts one after the other.
        advanceUntil("the board's two posts", 60) {
            has("the spring is flowing again", substring = true) && has("Left a pair of gloves on the bench", substring = true)
        }
        see("Left a pair of gloves on the bench", substring = true)

        // A post of ours goes out.
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("Two of us tonight")
        tapDesc(R.string.send)
        advanceUntil("our post sent", 30) {
            model.last(ChatId.room(board.key))?.let { it.outgoing && it.status !is com.tduhamel.lichen.core.MessageStatus.Waiting } == true
        }
        text("Two of us tonight").assertIsDisplayed()
        // Back to the node, the nodes, Settings: then Messages.
        repeat(3) { tapDesc(R.string.back) }
        tab(R.string.tab_messages)
        text("Refuge board").assertIsDisplayed()
    }

    /** CT-5: a wrong room password: refused, and said so. */
    @Test
    fun CT_5_roomWrongPassword() {
        launch("demo" to true)
        advanceUntil("the room server") { model.nodes.any { it.name == "Refuge board" } }
        val board = model.nodes.first { it.name == "Refuge board" }
        tab(R.string.tab_settings)
        tap(R.string.nodes)
        tap("Refuge board")
        scrollTo(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("wrong")
        tap(R.string.log_in)
        // A room server does not answer a wrong password (MeshCore): no answer, and why.
        advanceUntil("the login given up", 120) { model.server(board.key)?.login.let { it != null && it != ServerLogin.PENDING && it != ServerLogin.OUT } }
        assertEquals(ServerLogin.NO_ANSWER, model.server(board.key)?.login)
        see(R.string.login_no_answer)
    }

    /** CT-6: the hub's admin: logged in, its status read, a console command answered. */
    @Test
    fun CT_6_repeaterAdmin() = admin()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_6_repeaterAdmin_fr() = admin()

    private fun admin() {
        launch("demo" to true)
        advanceUntil("the hub") { model.nodes.any { it.name == "hub" } }
        val hub = model.nodes.first { it.name == "hub" }
        tab(R.string.tab_settings)
        tap(R.string.nodes)
        tap("hub")
        scrollTo(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("password")
        tap(R.string.log_in)
        advanceUntil("logged in as admin", 30) { model.server(hub.key)?.login == ServerLogin.ADMIN }
        see(R.string.login_admin)
        tap(R.string.read_status)
        advanceUntil("the hub's status", 30) { model.server(hub.key)?.stats != null }
        see(R.string.battery)
        tap(R.string.console)
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("ver")
        tapDesc(R.string.send)
        advanceUntil("the hub's answer", 30) { has("v1.18.0", substring = true) }
        text("v1.18.0 (Build: 15 Oct 2026)", substring = true).assertIsDisplayed()
    }

    /** CT-7: a trace to the crest, two hops away: each hop's SNR, out and back. */
    @Test
    fun CT_7_traceToANode() {
        launch("demo" to true)
        advanceUntil("the crest") { model.nodes.any { it.name == "crest" } }
        val crest = model.nodes.first { it.name == "crest" }
        tab(R.string.tab_settings)
        tap(R.string.nodes)
        tap("crest")
        tap(R.string.trace_path)
        advanceUntil("the trace back", 30) { model.trace(crest.key)?.state == com.tduhamel.lichen.core.TraceState.DONE }
        val trace = model.trace(crest.key)!!
        assertTrue("legs out and back", trace.legs.size >= 2)
        for (leg in trace.legs) leg.snrDb?.let { see("%.1f dB".format(it)) }
        see(str(R.string.trace_done, "%.1f".format((trace.rttMs ?: 0uL).toDouble() / 1000)))
    }

    /** CT-8: Camille's first message, a request: accepted, her chat opens with it. */
    @Test
    fun CT_8_requestAccepted() = requestAccepted()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_8_requestAccepted_fr() = requestAccepted()

    private fun requestAccepted() {
        launch("demo" to true, "requestDemo" to true)
        advanceUntil("Camille's request and what she wrote", 60) { model.requests.firstOrNull()?.messages?.isNotEmpty() == true }
        see(R.string.request_title, "Camille")
        see("I'm at the refuge", substring = true)
        // Each button's word whole, in French too.
        for (b in listOf(R.string.accept, R.string.block, R.string.delete)) assertEquals(str(b), 1, lines(str(b)))
        tap(R.string.accept)
        assertTrue(model.requests.isEmpty())
        // Her chat, with what she sent.
        desc(R.string.back).assertIsDisplayed()
        text("Hi! I'm at the refuge, the mobile network doesn't reach up here.").assertIsDisplayed()
        tapDesc(R.string.back)
        assertFalse(has(str(R.string.request_title, "Camille")))
        text("Camille").assertIsDisplayed()
    }

    /** CT-9: a request blocked: nothing more from her, she is listed as blocked; another
     * deleted: gone. */
    @Test
    fun CT_9_requestBlockedThenDeleted() {
        launch("demo" to true, "requestDemo" to true)
        advanceUntil("Camille's request", 60) { model.requests.isNotEmpty() }
        val identity = model.requests.first().identity
        tap(R.string.block)
        assertTrue(model.requests.isEmpty())
        assertFalse(has(str(R.string.request_title, "Camille")))
        // Her later messages (her photo at 150 s) do not come in.
        advance(200)
        assertTrue(model.requests.isEmpty())
        assertFalse(has("I'm at the refuge", substring = true))
        assertTrue(model.blocked.isNotEmpty())
        tab(R.string.tab_settings)
        tap(R.string.blocked)
        text(R.string.unblock).assertIsDisplayed()
        assertTrue(identity.isNotEmpty())
    }

    @Test
    fun CT_9_requestDeleted() {
        launch("demo" to true, "requestDemo" to true)
        advanceUntil("Camille's request", 60) { model.requests.isNotEmpty() }
        tap(R.string.delete)
        assertTrue(model.requests.isEmpty())
        assertFalse(has(str(R.string.request_title, "Camille")))
        assertFalse(has("I'm at the refuge", substring = true))
        assertTrue(model.blocked.isEmpty())
    }

    /** CT-10: Camille blocked: her messages stop, Settings › Blocked lists her; unblocked, they
     * come back. */
    @Test
    fun CT_10_blockAndUnblock() = blockAndUnblock()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_10_blockAndUnblock_fr() = blockAndUnblock()

    private fun blockAndUnblock() {
        // Her chat from New message (the debug extra `open` would open it again later, when her
        // identity arrives).
        launch("demo" to true)
        val camille = contact("Camille")
        tapDesc(R.string.new_message)
        tap("Camille")
        advanceUntil("her chat", 10) { has(R.string.message_placeholder) }
        tapDesc(R.string.more)
        tapInDialog(R.string.block)
        tapInDialog(R.string.block)
        text(R.string.you_blocked, "Camille").assertIsDisplayed()
        assertTrue(model.isBlocked(camille.key))
        // Her first message (30 s) does not come in.
        advance(60)
        assertFalse(has("I'm at the refuge", substring = true))
        tapDesc(R.string.back)
        tab(R.string.tab_settings)
        tap(R.string.blocked)
        text("Camille").assertIsDisplayed()
        tap(R.string.unblock)
        assertFalse(model.isBlocked(camille.key))
        // Unblocked: what she sends from now on comes in: her photo's caption (150 s), once her
        // phone gave up the hello it was still trying (declined, a few minutes). The hello,
        // written while she was blocked, stays out.
        advanceUntil("her photo's caption", 900) {
            model.conversations.any { ChatId.of(it.chat) == ChatId.contact(camille.key) && !it.last.outgoing }
        }
        tapDesc(R.string.back)
        tab(R.string.tab_messages)
        tap("Camille")
        advanceUntil("the caption in her chat", 30) { has("The view from the refuge", substring = true) }
        assertFalse(has("I'm at the refuge", substring = true))
    }

    /** CT-11: Marc, filed by the radio as a repeater: his chat stays in Messages, under his name. */
    @Test
    fun CT_11_contactFiledAsNonPerson() {
        launch("demo" to true, "marcRepeater" to true)
        advanceUntil("Marc's message", 60) { has("Marc from the refuge", substring = true) }
        text("Marc").assertIsDisplayed()
        assertFalse(model.contacts.any { it.name == "Marc" })
        tapDesc(R.string.new_message)
        assertFalse(has(str(R.string.meshcore_contact, hopsBadge(context.resources, 1u)), substring = true) && has("Marc"))
    }
}
