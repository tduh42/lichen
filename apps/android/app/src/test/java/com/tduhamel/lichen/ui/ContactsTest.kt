package com.tduhamel.lichen.ui

import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasClickAction
import com.tduhamel.lichen.model.nodeRoute
import org.junit.Assert.assertNull
import com.tduhamel.lichen.core.SignalRating
import com.tduhamel.lichen.model.dbText
import com.tduhamel.lichen.model.heardWithin
import com.tduhamel.lichen.model.nodeSignalHistory
import com.tduhamel.lichen.model.wallNow
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
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

    /** CT-3: the nodes the radio knows or heard, each with its kind, position, last heard; in
     * reach (heard directly, ADR-0049) with their signal, by rating: the hub Good, Lucie Fair, the
     * board Bad; through repeaters, the crest one hop away. */
    @Test
    fun CT_3_nodes() {
        // The crest is one hop away: singular (B-and-4).
        assertEquals("1 hop", plural(R.plurals.hops_n, 1, 1))
        assertEquals("7.5 dB", dbText(7.5f))
        nodes()
    }

    @Test
    @Config(qualifiers = "+fr-night")
    fun CT_3_nodes_frDark() {
        assertEquals("1 saut", plural(R.plurals.hops_n, 1, 1))
        assertEquals("7,5 dB", dbText(7.5f))
        nodes()
    }

    /** The demo's nodes, the hub, Lucie and the board with a reading of their own. */
    private fun nodesHeard() {
        advanceUntil("the radio's nodes, their signals and Lucie's advert", 30, { model.nodes.map { it.name to it.signal } }) {
            listOf("hub", "Lucie", "Refuge board").all { n -> model.nodes.any { it.name == n && it.signal != SignalRating.NONE } } &&
                model.nodes.any { it.name == "crest" }
        }
    }

    private fun nodes() {
        launch("demo" to true)
        nodesHeard()
        tab(R.string.tab_nodes)
        // How many it knows, how many it heard lately.
        val nowS = wallNow() / 1000
        val heard = model.nodes.count { it.heardWithin(nowS, 7_200) }
        see(plural(R.plurals.nodes_known, model.nodes.size, model.nodes.size) + " · " + plural(R.plurals.nodes_heard_2h, heard, heard))
        // In reach, by rating, each with its meter and its last SNR (the demo's links vary a
        // little around 7.5, −4.0 and −9.5 dB): the hub Good, Lucie Fair, the board Bad.
        see(R.string.nodes_in_reach)
        val signal = { name: String -> model.nodes.first { it.name == name } }
        assertEquals(SignalRating.GOOD, signal("hub").signal)
        assertEquals(SignalRating.FAIR, signal("Lucie").signal)
        assertEquals(SignalRating.BAD, signal("Refuge board").signal)
        desc(str(R.string.signal_good, dbText(signal("hub").snrDb!!))).assertIsDisplayed()
        desc(str(R.string.signal_fair, dbText(signal("Lucie").snrDb!!))).assertIsDisplayed()
        desc(str(R.string.signal_bad, dbText(signal("Refuge board").snrDb!!))).assertIsDisplayed()
        val top = { name: String -> text(name).fetchSemanticsNode().boundsInRoot.top }
        assertTrue("hub, Lucie, the board in that order", top("hub") < top("Lucie") && top("Lucie") < top("Refuge board"))
        // Through repeaters: the crest, with its hops.
        see(R.string.nodes_through_repeaters)
        assertTrue("the crest below the board", top("Refuge board") < top("crest"))
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
        text(str(R.string.heard_cap, model.heard(hub.lastHeardS)), substring = true).assertIsDisplayed()
    }

    /** CT-3: a lichen user's row: once Camille's identity is known, the lichen badge, not "MeshCore
     * app" (the nodes are read again when an identity is learned). */
    @Test
    fun CT_3_lichenUserInTheNodes() {
        launch("demo" to true)
        advanceUntil("Camille's identity", 120) { model.contacts.any { it.name == "Camille" && it.identity != null } }
        assertTrue("her node is a lichen user", model.nodes.first { it.name == "Camille" }.lichen)
        tab(R.string.tab_nodes)
        val row = compose.onNode(hasText("Camille") and hasClickAction())
        row.assert(hasText(str(R.string.app_name)))
        row.assert(!hasText(str(R.string.node_kind_stock), substring = true))
    }

    /** CT-3: the filters, each with its count; Heard in 2 h keeps the nodes heard lately. */
    @Test
    fun CT_3_nodeFilters() = filters()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_3_nodeFilters_fr() = filters()

    private fun filters() {
        launch("demo" to true)
        nodesHeard()
        tab(R.string.tab_nodes)
        val repeaters = model.nodes.count { it.kind == NodeKind.REPEATER }
        tap("${str(R.string.filter_repeaters)} $repeaters")
        text("hub").assertIsDisplayed()
        assertFalse(has("Lucie"))
        tap(R.string.filter_heard_2h)
        val nowS = wallNow() / 1000
        for (n in model.nodes) assertEquals(n.name, n.heardWithin(nowS, 7_200), has(n.name))
        tap("${str(R.string.filter_all)} ${model.nodes.size}")
        text("Lucie").assertIsDisplayed()
    }

    /** CT-3: the hub's page: its large meter, its signal over the last 24 h (a reading at least),
     * now. */
    @Test
    fun CT_3_hubPage() = hubPage()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_3_hubPage_fr() = hubPage()

    private fun hubPage() {
        launch("demo" to true)
        nodesHeard()
        tab(R.string.tab_nodes)
        tap("hub")
        val hub = model.nodes.first { it.name == "hub" }
        desc(str(R.string.signal_good, dbText(hub.snrDb!!))).assertIsDisplayed()
        see(R.string.signal_24h)
        val readings = model.nodeSignalHistory(hub.key)
        assertTrue("a reading at least", readings.isNotEmpty())
        val last = readings.last()
        assertTrue(hasDesc(plural(R.plurals.chart_said, readings.size, readings.size, dbText(last.snrDb))))
        see(str(R.string.signal_now, dbText(hub.snrDb!!)))
        // A week, then a day again.
        tab(R.string.range_7d)
        see(R.string.signal_7d)
        tab(R.string.range_24h)
        assertFalse(has(R.string.no_own_signal))
    }

    /** CT-3: the Nodes tab opens on the node list (from the debug extra `tab nodes` and from the
     * tab bar), a node's page pushes from it and comes back to it; Settings no longer lists the
     * nodes. */
    @Test
    fun CT_3_nodesTabOpensOnTheList() = nodesTab()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_3_nodesTabOpensOnTheList_fr() = nodesTab()

    private fun nodesTab() {
        launch("demo" to true, "tab" to "nodes")
        advanceUntil("the radio's nodes", 20) { model.nodes.any { it.name == "hub" } }
        // Its title and its list at once; a tab's root has no way back.
        assertTrue(has(R.string.tab_nodes))
        see("hub")
        assertFalse(hasDesc(str(R.string.back)))
        tap("hub")
        see(R.string.advertised_position)
        tapDesc(R.string.back)
        see("crest")
        // Settings: no Nodes row, only the tab of that name.
        tab(R.string.tab_settings)
        see(R.string.radio_settings)
        assertEquals(0, compose.onAllNodes(hasText(str(R.string.tab_nodes)) and !isTab).fetchSemanticsNodes().size)
        tab(R.string.tab_nodes)
        see("hub")
    }

    private val isTab = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)

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

    /** CT-3 (F-1): a node's MeshCore public key, copied from its screen (the Nodes tab). */
    @Test
    fun CT_3_nodeMeshcorePublicKey() = nodeKey()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_3_nodeMeshcorePublicKey_fr() = nodeKey()

    private fun nodeKey() {
        launch("demo" to true)
        advanceUntil("the radio's nodes") { model.nodes.any { it.name == "hub" } }
        tab(R.string.tab_nodes)
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
        tab(R.string.tab_nodes)
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
        // Back to the node, then the Nodes tab: then Messages.
        repeat(2) { tapDesc(R.string.back) }
        tab(R.string.tab_messages)
        text("Refuge board").assertIsDisplayed()
    }

    /** CT-5: a wrong room password: refused, and said so. */
    @Test
    fun CT_5_roomWrongPassword() {
        launch("demo" to true)
        advanceUntil("the room server") { model.nodes.any { it.name == "Refuge board" } }
        val board = model.nodes.first { it.name == "Refuge board" }
        tab(R.string.tab_nodes)
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
        tab(R.string.tab_nodes)
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
    /** CT-7: the crest's route, You → hub → crest: the hub's leg from its signal, the crest's not
     * measured until a trace measures it; no signal of its own; Show on the map. */
    @Test
    fun CT_7_traceToANode() = traceToCrest()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_7_traceToANode_fr() = traceToCrest()

    private fun traceToCrest() {
        launch("demo" to true)
        nodesHeard()
        val crest = model.nodes.first { it.name == "crest" }
        tab(R.string.tab_nodes)
        tap("crest")
        see(R.string.how_it_reaches_you)
        val you = str(R.string.you)
        // You → hub → crest: the hub's leg from its own signal (about 7.5 dB, Good), the crest's
        // not measured (it is heard only through the hub).
        val route = model.nodeRoute(crest.key)!!
        assertEquals(listOf("hub"), route.stops[1].names)
        assertEquals(SignalRating.GOOD, route.legs[0].signal)
        assertNull(route.legs[1].snrDb)
        desc(str(R.string.leg_said, you, "hub", dbText(route.legs[0].snrDb!!))).assertIsDisplayed()
        desc(str(R.string.leg_said, "hub", "crest", str(R.string.not_measured))).assertIsDisplayed()
        see(R.string.not_traced)
        // Heard only through the hub: no signal of its own.
        see(R.string.no_own_signal)
        tap(R.string.trace_path)
        advanceUntil("the trace back", 30) { model.trace(crest.key)?.state == com.tduhamel.lichen.core.TraceState.DONE }
        val trace = model.trace(crest.key)!!
        assertTrue("legs out and back", trace.legs.size >= 2)
        // The crest's leg, measured now (about −3.0 dB).
        val traced = model.nodeRoute(crest.key)!!.legs[1]
        assertTrue("traced", traced.traced)
        desc(str(R.string.leg_said, "hub", "crest", dbText(traced.snrDb!!))).assertIsDisplayed()
        see(str(R.string.traced_summary, "", "", "").substringBefore(" "), substring = true)
        see(R.string.trace_again)
        // The map, with its route (MP-3).
        tap(R.string.show_on_map)
        assertEquals(com.tduhamel.lichen.model.AppModel.Tab.MAP, model.tab)
        assertEquals(com.tduhamel.lichen.model.AppModel.MapMode.POSITIONS, model.mapMode)
        see(str(R.string.route_to, "crest"))
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

    // CT-12: a MeshCore contact from a link (F-2, ADR-0047), on the demo radio.

    /** Add a contact › Scan, with [text] pasted and Add tapped. */
    private fun pasteLink(text: String) {
        tap(R.string.scan)
        compose.onAllNodes(hasSetTextAction()).onLast().performTextReplacement(text)
        tap(R.string.add)
    }

    /** CT-12: a signed link pasted shows its card (name, kind, key in groups of 8, signed); Add
     * puts the contact among the people and opens their MeshCore chat; pasted again, it is
     * already on the radio, and its chat opens. */
    @Test
    fun CT_12_addFromASignedLink() = addFromASignedLink()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_12_addFromASignedLink_fr() = addFromASignedLink()

    private fun addFromASignedLink() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        // A node the sharer's radio heard through 2 repeaters: said on its card; Cancel.
        pasteLink(TWO_HOP_LINK)
        see("crest")
        see(plural(R.plurals.heard_through_repeaters, 2, 2))
        tap(R.string.cancel)
        pasteLink(SIGNED_LINK)
        see("Hut radio")
        see(R.string.heard_directly)
        see(R.string.node_kind_companion)
        see("83b6ede3 fdf630ce afd3fc18", substring = true)
        see(R.string.link_signed)
        assertFalse(model.contacts.any { it.name == "Hut radio" })
        tap(R.string.add_to_radio)
        advanceUntil("added", 30) { has(R.string.added_to_radio) }
        val hut = model.contacts.first { it.name == "Hut radio" }
        assertTrue(hut.identity == null)
        tap(R.string.open_the_chat)
        advanceUntil("its MeshCore chat", 10) { hasDesc(str(R.string.about_meshcore_chats)) }
        see("Hut radio")
        // Among the people, as a MeshCore chat.
        tapDesc(R.string.back)
        tapDesc(R.string.new_message)
        see(str(R.string.meshcore_contact, hopsBadge(context.resources, model.contacts.first { it.name == "Hut radio" }.hops)))
        see("Hut radio")
        back()

        // Pasted again: already on the radio, and its chat opens.
        tapDesc(R.string.add_contact)
        pasteLink(SIGNED_LINK)
        tap(R.string.add_to_radio)
        see(R.string.already_on_radio)
        tap(R.string.open_the_chat)
        advanceUntil("its MeshCore chat", 10) { hasDesc(str(R.string.about_meshcore_chats)) }
        assertEquals(1, model.contacts.count { it.name == "Hut radio" })
    }

    /** CT-12: a signed link with one hex digit changed (its timestamp) is refused as changed or
     * made up: no card, nothing added. */
    @Test
    fun CT_12_changedLinkRefused() = changedLinkRefused()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_12_changedLinkRefused_fr() = changedLinkRefused()

    private fun changedLinkRefused() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        val changed = SIGNED_LINK.replace("4002df6a", "4102df6a")
        assertTrue(changed != SIGNED_LINK)
        pasteLink(changed)
        see(R.string.err_bad_signature)
        assertFalse(has(R.string.add_to_radio))
        assertFalse(model.contacts.any { it.name == "Hut radio" })
    }

    /** CT-12: a contact code is read as unsigned, and added among the people. */
    @Test
    fun CT_12_contactCodeAddedUnsigned() = contactCodeAdded()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_12_contactCodeAddedUnsigned_fr() = contactCodeAdded()

    private fun contactCodeAdded() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        pasteLink(CONTACT_CODE)
        see("Example Contact")
        see(R.string.link_unsigned)
        assertFalse(has(R.string.heard_directly))
        tap(R.string.add_to_radio)
        advanceUntil("added", 30) { has(R.string.added_to_radio) }
        assertTrue(model.contacts.any { it.name == "Example Contact" })
        // Close leaves Add a contact; the contact is among the people.
        tap(R.string.close)
        tapDesc(R.string.new_message)
        see("Example Contact")
    }

    /** CT-12: with the radio gone, the link's card says to connect it first. */
    @Test
    fun CT_12_linkWithNoRadio() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        model.setDemoRadio(false)
        advanceUntil("the radio off", 20) { !model.isConnected }
        pasteLink(SIGNED_LINK)
        see("Hut radio")
        tap(R.string.add_to_radio)
        see(R.string.connect_radio_first)
    }

    private companion object {
        /** A companion's signed advert, as the stock app shares it (testdata/meshcore/mesh.toml,
         * `link-companion`: test keys). */
        const val SIGNED_LINK = "meshcore://110083b6ede3fdf630ceafd3fc187fb096c9cc228e15f1f88749c1efd4e963e7b2724002df6a7e1bae75dd2c45634dddd2c8bc8358fbcaec698e8de8bce1772a5e92d9e3484adb0cf917bddc6c591c3c5113bffdeaa95f4653ab8ec4c9b7424b4717a383180a8148757420726164696f"

        /** A repeater's signed advert that came to the sharing radio through 2 repeaters
         * (mesh.toml, `link-with-2-hop-path`). */
        const val TWO_HOP_LINK = "meshcore://11025a779b3307f6ab98064f997635bf58e102adaceee753b26d5372c328d234a2a855af4602df6a778ef066d76f220fc37acd532b34c9956551adc36e4e0d48dbc53714291572dabd9426998b59a773c6c9fda79d93f6d3098bf1483e165433463b2ac9659feb0a826372657374"

        /** A contact code (mesh.toml, the F-1 vectors): unsigned. */
        const val CONTACT_CODE = "meshcore://contact/add?name=Example+Contact&public_key=9cd8fcf22a47333b591d96a2b848b73f457b1bb1a3ea2453a885f9e5787765b1&type=1"
    }

    // CT-13: New conversation when the radio knows many people (F-4, `manyContacts`).

    /** The New conversation sheet's list. */
    private val sheet get() = hasScrollAction() and hasAnyDescendant(hasText(str(R.string.contacts)))

    /** That text in the sheet's list (not in Messages behind it). */
    private fun inSheet(t: String) = compose.onAllNodes(hasText(t) and hasAnyAncestor(sheet), useUnmergedTree = true)

    private fun shownInSheet(t: String) = inSheet(t).fetchSemanticsNodes().isNotEmpty()

    /** The screen as it is, the sheet on top, saved as `<en|fr>[-dark]-name.png` in the folder
     * `LICHEN_SHOTS` names, when it is set (screenshots for review). */
    private fun shot(name: String) {
        val dir = System.getenv("LICHEN_SHOTS") ?: return
        val config = context.resources.configuration
        val look = config.locales[0].language + if (config.isNightModeActive) "-dark" else ""
        // The sheet's own window when it is open (a dialog: capturing its node gives the
        // activity's pixels under Robolectric), else the activity's.
        val dialog = org.robolectric.shadows.ShadowDialog.getLatestDialog()?.takeIf { it.isShowing }
        val image = dialog?.window?.decorView?.let { view ->
            android.graphics.Bitmap.createBitmap(view.width, view.height, android.graphics.Bitmap.Config.ARGB_8888)
                .also { view.draw(android.graphics.Canvas(it)) }
        } ?: compose.onAllNodes(isRoot()).onFirst().captureToImage().asAndroidBitmap()
        java.io.File(dir).mkdirs()
        java.io.FileOutputStream(java.io.File(dir, "$look-$name.png")).use {
            image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    /** The demo radio with 200 more MeshCore people, and Camille's identity known. */
    private fun launchMany() {
        launch("demo" to true, "manyContacts" to 200)
        advanceUntil("the 200 people", 60) { model.contacts.count { it.name.startsWith("Radio ") } == 200 }
        advanceUntil("Camille's identity", 120) { model.contacts.any { it.name == "Camille" && it.identity != null } }
    }

    /** CT-13: with 200 people, the actions and the channels come first, on the first screen;
     * the filter shows MeshCore people only, then lichen people only. */
    @Test
    fun CT_13_newConversationWithManyPeople() = manyPeople()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_13_newConversationWithManyPeople_fr() = manyPeople()

    @Test
    @Config(qualifiers = "+night")
    fun CT_13_newConversationWithManyPeople_dark() = manyPeople()

    @Test
    @Config(qualifiers = "+fr-night")
    fun CT_13_newConversationWithManyPeople_frDark() = manyPeople()

    private fun manyPeople() {
        launchMany()
        tapDesc(R.string.new_message)
        // Its title, then, on the first screen, without scrolling: the actions and Public.
        text(R.string.new_conversation).assertIsDisplayed()
        for (t in listOf(str(R.string.new_group), str(R.string.join_a_channel), str(R.string.add_contact), "Public")) {
            inSheet(t).onFirst().assertIsDisplayed()
        }
        shot("new-conversation")
        val lichen = model.contacts.count { it.identity != null }
        see(str(R.string.people_all, model.contacts.size))
        // MeshCore people only.
        tap(str(R.string.people_meshcore, model.contacts.size - lichen))
        inSheet("Radio 001").onFirst().assertIsDisplayed()
        shot("new-conversation-meshcore")
        assertFalse("Camille among the MeshCore people", shownInSheet("Camille"))
        // lichen people only.
        tap(str(R.string.people_lichen, lichen))
        inSheet("Camille").onFirst().assertIsDisplayed()
        assertFalse("a MeshCore person among the lichen ones", shownInSheet("Radio 001"))
    }

    /** CT-13: `#tag` typed: "Join #tag" joins the hashtag channel and opens it; typed again, the
     * row is that channel. */
    @Test
    fun CT_13_hashtagJoinsAndOpens() = hashtagJoins()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_13_hashtagJoinsAndOpens_fr() = hashtagJoins()

    private fun hashtagJoins() {
        launch("demo" to true)
        // The radio's channels read (joining needs them): Public listed.
        advanceUntil("the radio's channels") { model.channels.isNotEmpty() }
        tapDesc(R.string.new_message)
        inSheet("Public").onFirst().assertIsDisplayed()
        // Not a hashtag (a space): no row to join.
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("#two words")
        assertFalse(has(R.string.join_tag_channel, "#two"))
        assertFalse(has(R.string.join_tag_channel, "#two words"))
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextReplacement("#Ridge")
        tap(R.string.join_tag_channel, "#ridge")
        advanceUntil("the channel joined", 10) { model.channels.any { it.name == "#ridge" } }
        advanceUntil("its chat", 10) { has(R.string.message_placeholder) }
        see("#ridge")
        tapDesc(R.string.back)
        tapDesc(R.string.new_message)
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("#ridge")
        assertFalse("offered to join it again", has(R.string.join_tag_channel, "#ridge"))
        // Once: on top, not again under the channels.
        assertEquals(1, inSheet("#ridge").fetchSemanticsNodes().size)
        shot("new-conversation-hashtag")
        assertEquals(1, model.channels.count { it.name == "#ridge" })
    }

    /** CT-13: New group is its own screen: a name, a lichen contact picked from its search,
     * Create; the group's chat opens with them in it. */
    @Test
    fun CT_13_newGroupScreen() = newGroupScreen()

    @Test
    @Config(qualifiers = "+fr")
    fun CT_13_newGroupScreen_fr() = newGroupScreen()

    private fun newGroupScreen() {
        launchMany()
        tapDesc(R.string.new_message)
        tap(R.string.new_group)
        // Its own screen: the sheet is gone.
        assertFalse(has(str(R.string.people_all, model.contacts.size)))
        compose.onNode(hasSetTextAction() and hasText(str(R.string.group_name))).performTextInput("Summit team")
        // MeshCore people cannot be members: only lichen ones are listed.
        assertFalse(has("Radio 001"))
        compose.onNode(hasSetTextAction() and hasText(str(R.string.search_contacts))).performTextInput("Cam")
        assertFalse(has("Julien"))
        tap("Camille")
        shot("new-group")
        tap(R.string.create_group)
        val g = model.groups.single { it.name == "Summit team" }
        assertEquals(2, g.members.size)
        assertTrue(g.members.any { it.name == "Camille" })
        text(R.string.message_placeholder).assertIsDisplayed()
        text("Summit team").assertIsDisplayed()
    }
}
