package com.tduhamel.lichen.ui

import com.tduhamel.lichen.model.nodeRoute
import com.tduhamel.lichen.model.dbText
import com.tduhamel.lichen.core.SignalRating
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.SharePrecision
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.NodeFilter
import com.tduhamel.lichen.model.heardWithin
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.wallNow
import com.tduhamel.lichen.model.measurements
import com.tduhamel.lichen.model.search
import com.tduhamel.lichen.model.searchEstimate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Area 8 of docs/testing.md: location, map, Locate and SOS. The JVM has no MapLibre: its map
 * lists the markers' labels where the map would be and reports a camera around them (MP-4's
 * download itself, over the Internet, is the emulator's). */
@RunWith(AndroidJUnit4::class)
class LocationTest : UiTest() {
    private fun count(t: String, substring: Boolean = false) =
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).fetchSemanticsNodes().size

    /** Simulated time for the demo and for the screen's own clocks (Compose's delays) together. */
    private fun withScreen(seconds: Int) = repeat(seconds) {
        compose.mainClock.advanceTimeBy(1_000)
        advance(1)
    }

    /** MP-1: our position to Camille: a precision chosen, shown in the panel, changed, stopped. */
    @Test
    fun MP_1_shareMyPositionWithAContact() = share()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_1_shareMyPositionWithAContact_fr() = share()

    private fun share() {
        launch("demo" to true)
        val camille = ChatId.contact(contact("Camille").key)
        tab(R.string.tab_map)
        see(R.string.location_sharing)
        see(R.string.off)
        tap(R.string.share_my_position)
        // Camille, not shared yet: exact, for an hour.
        tap(R.string.not_shared)
        tapInDialog(R.string.exact)
        assertEquals(SharePrecision.EXACT, model.share(camille)?.precision)
        back()
        // The panel: her row, its precision, the time left.
        see("Camille")
        see(R.string.exact)
        see(str(R.string.active_ends_in, "").trim(), substring = true)
        // Changed, from the panel's row: 1 km, the same share.
        tap("Camille")
        // The sheet's "Exact" (its window comes after the screen's, which shows "Exact" too).
        compose.onAllNodesWithText(str(R.string.exact)).onLast().performClick()
        tapInDialog(R.string.km1)
        assertEquals(SharePrecision.KM1, model.share(camille)?.precision)
        assertEquals(1, model.shares.size)
        back()
        // Positions go while it lasts (the demo's phone has a fix).
        val before = model.budget.usedMs
        advance(90)
        assertTrue("positions went", model.budget.usedMs > before)
        // Stopped by the panel's switch.
        toggle(R.string.location_sharing)
        assertNull(model.share(camille))
        see(R.string.off)
    }

    /** MP-1 by the debug extra `share`: Camille at 100 m from the start. */
    @Test
    fun MP_1_shareExtra() {
        launch("demo" to true, "share" to "Camille", "tab" to "map")
        advanceUntil("the share", 30) { model.shares.isNotEmpty() }
        assertEquals(SharePrecision.M100, model.shares.single().precision)
        see(R.string.m100)
    }

    /** MP-2: Camille's exact position and Julien's 1 km cell, one marker each. */
    @Test
    fun MP_2_positionsReceivedOnTheMap() = positions()

    @Test
    @Config(qualifiers = "+fr-night")
    fun MP_2_positionsReceivedOnTheMap_frDark() = positions()

    private fun positions() {
        launch("demo" to true)
        val camille = contact("Camille").key.hex()
        advanceUntil("Julien's key", 300) { model.contacts.any { it.name == "Julien" } }
        val julien = contact("Julien").key.hex()
        tab(R.string.tab_map)
        advanceUntil("both on the map", 900, { model.positions.map { it.from.hex() } }) {
            model.positions.any { it.from.hex() == camille } && model.positions.any { it.from.hex() == julien }
        }
        advance(6)
        val c = model.positions.single { it.from.hex() == camille }
        val j = model.positions.single { it.from.hex() == julien }
        assertTrue("Camille exact: $c", c.radiusM < 70)
        assertTrue("Julien coarse: $j", j.radiusM >= 700)
        // Their markers: Camille's with its age, Julien's with his precision.
        assertEquals(1, count("Camille · ", substring = true))
        see("Julien · ±" + str(R.string.km1))
        assertEquals(1, count("Julien · ", substring = true))
    }

    /** MP-3: the nodes layer: the repeaters and the room where they advertise, off and on. */
    @Test
    fun MP_3_nodesLayer() = nodes()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_3_nodesLayer_fr() = nodes()

    private fun nodes() {
        launch("demo" to true)
        advanceUntil("the radio's nodes", 30) { model.nodes.size >= 5 }
        tab(R.string.tab_map)
        for (n in listOf("hub", "crest", "Refuge board")) see(n)
        // Lucie's advert has no position: not on the map.
        assertEquals(0, count("Lucie"))
        // Off, then on again.
        tapDesc(R.string.map_layers)
        tapInDialog(R.string.map_nodes)
        assertEquals(0, count("hub"))
        tapDesc(R.string.map_layers)
        tapInDialog(R.string.map_nodes)
        see("hub")
    }

    /** MP-3: the nodes layer's filters, in the layers menu: the Nodes tab's chips with their
     * counts; Repeaters leaves only the repeaters; Heard in 2 h hides a node last heard 3 h ago;
     * All brings every node back. The choice is the map's own (the Nodes tab keeps All), kept
     * while the app runs; a route's stops stay whatever it leaves. */
    @Test
    fun MP_3_nodeFiltersOnTheMap() = nodeFilters()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_3_nodeFiltersOnTheMap_fr() = nodeFilters()

    private fun nodeFilters() {
        launch("demo" to true)
        advanceUntil("the radio's nodes", 30) {
            model.nodes.count { it.lat != null } >= 3 && model.nodes.any { it.name == "crest" } && model.nodes.any { it.name == "hub" && it.signal != SignalRating.NONE }
        }
        tab(R.string.tab_map)
        fun placed() = model.nodes.filter { it.lat != null && it.lon != null }
        for (n in placed()) see(n.name)
        fun choose(label: String) {
            tapDesc(R.string.map_layers)
            tapInDialog(label)
            // The menu stays open, to show what each leaves.
            assertTrue(compose.onAllNodes(hasText(str(R.string.filter_heard_2h)) and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty())
            back()
        }
        // Repeaters: hub, crest and the other repeaters alone.
        val repeaters = model.nodes.count { it.kind == NodeKind.REPEATER }
        choose("${str(R.string.filter_repeaters)} $repeaters")
        assertEquals(NodeFilter.REPEATERS, model.mapNodeFilter)
        for (n in placed()) assertEquals(n.name, if (n.kind == NodeKind.REPEATER) 1 else 0, count(n.name))
        assertEquals(1, count("hub"))
        assertEquals(0, count("Refuge board"))
        // Apart from the Nodes tab's, which still shows everyone; kept on the way back.
        tab(R.string.tab_nodes)
        text("Lucie").assertIsDisplayed()
        tab(R.string.tab_map)
        assertEquals(0, count("Refuge board"))
        // A route's stops whatever the filter: Rooms, with the crest's route on.
        val crest = model.nodes.first { it.name == "crest" }
        choose("${str(R.string.filter_rooms)} ${model.nodes.count { it.kind == NodeKind.ROOM_SERVER }}")
        assertEquals(0, count("crest"))
        model.mapRouteTo = crest.key.hex()
        compose.waitForIdle()
        for (n in listOf("hub", "crest", "Refuge board")) assertEquals(n, 1, count(n))
        tapDesc(R.string.close_route)
        assertEquals(0, count("hub"))
        // Heard in 2 h: the hub, as if last heard 3 h ago, leaves.
        val nowS = wallNow() / 1000
        model.nodes = model.nodes.map { if (it.name == "hub") it.copy(lastHeardS = (nowS - 3 * 3_600).toULong()) else it }
        choose(str(R.string.filter_heard_2h))
        for (n in placed()) assertEquals(n.name, if (n.heardWithin(nowS, 7_200)) 1 else 0, count(n.name))
        assertEquals(0, count("hub"))
        assertTrue(count("crest") == 1)
        // All: every node again.
        choose("${str(R.string.filter_all)} ${model.nodes.size}")
        for (n in placed()) assertEquals(n.name, 1, count(n.name))
    }

    /** MP-3: a route shown fades the markers that are not its stops (P-route-dim): with the
     * crest's route, the Refuge board, between the hub and the crest, is "not on this route",
     * the hub and the crest are not; the node filters still apply to the rest; Close the route
     * brings it back. */
    @Test
    fun MP_3_routeFadesTheOthers() = routeFades()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_3_routeFadesTheOthers_fr() = routeFades()

    private fun faded(label: String) = compose.onAllNodes(
        hasText(label) and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.not_on_route)),
        useUnmergedTree = true,
    ).fetchSemanticsNodes().isNotEmpty()

    private fun routeFades() {
        launch("demo" to true)
        advanceUntil("the hub's signal and the crest", 30) {
            model.nodes.any { it.name == "hub" && it.signal != SignalRating.NONE } && model.nodes.any { it.name == "crest" }
        }
        tab(R.string.tab_map)
        for (n in listOf("hub", "crest", "Refuge board")) assertFalse(n, faded(n))
        val crest = model.nodes.first { it.name == "crest" }
        tab(R.string.tab_nodes)
        tap("crest")
        tap(R.string.show_on_map)
        assertEquals(crest.key.hex(), model.mapRouteTo)
        // Its stops as they were; the board, not on it, faded but there.
        assertEquals(1, count("Refuge board"))
        assertTrue(faded("Refuge board"))
        assertFalse(faded("hub"))
        assertFalse(faded("crest"))
        // The filters still apply to the rest: Repeaters leaves the board out, the stops stay.
        tapDesc(R.string.map_layers)
        tapInDialog("${str(R.string.filter_repeaters)} ${model.nodes.count { it.kind == NodeKind.REPEATER }}")
        back()
        assertEquals(0, count("Refuge board"))
        assertEquals(1, count("hub"))
        tapDesc(R.string.map_layers)
        tapInDialog("${str(R.string.filter_all)} ${model.nodes.size}")
        back()
        assertTrue(faded("Refuge board"))
        // Closed: nothing faded.
        tapDesc(R.string.close_route)
        assertNull(model.mapRouteTo)
        assertEquals(1, count("Refuge board"))
        for (n in listOf("hub", "crest", "Refuge board")) assertFalse(n, faded(n))
    }

    /** MP-3: an SOS never fades: with crest's route shown, Camille's SOS marker (`sosdemo2`,
     * her position with it) stays at full strength while the Refuge board, off the route, fades. */
    @Test
    fun MP_3_routeNeverFadesAnSos() {
        launch("demo" to true, "sosdemo2" to true)
        val camille = contact("Camille").key.hex()
        advanceUntil("Camille's SOS with her position", 600) { model.positions.any { it.from.hex() == camille && it.sos } }
        // Their alerts out of the way (Julien's, then hers).
        repeat(3) { if (model.sosAlert != null) tap(R.string.dismiss) }
        tab(R.string.tab_map)
        val sos = "SOS · Camille"
        assertEquals(1, count(sos))
        advanceUntil("the crest", 30) { model.nodes.any { it.name == "crest" } }
        model.mapRouteTo = model.nodes.first { it.name == "crest" }.key.hex()
        compose.waitForIdle()
        assertTrue(faded("Refuge board"))
        assertEquals(1, count(sos))
        assertFalse(faded(sos))
    }

    /** MP-3: a node's route on the map (§16.3): from the crest's page, Show on the map draws
     * You → hub → crest, each leg readable ("hub to crest, not measured"); its card says how
     * far and through which repeaters, with the legend; a node tapped on the map shows its
     * own; closed, the share panel is back. The layers menu has the legend too. */
    @Test
    fun MP_3_routeOnTheMap() = routeOnTheMap()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_3_routeOnTheMap_fr() = routeOnTheMap()

    private fun routeOnTheMap() {
        launch("demo" to true)
        advanceUntil("the hub's signal and the crest", 30) {
            model.nodes.any { it.name == "hub" && it.signal != SignalRating.NONE } && model.nodes.any { it.name == "crest" }
        }
        val crest = model.nodes.first { it.name == "crest" }
        tab(R.string.tab_nodes)
        tap("crest")
        tap(R.string.show_on_map)
        assertEquals(AppModel.Tab.MAP, model.tab)
        assertEquals(crest.key.hex(), model.mapRouteTo)
        // The card: to the crest, one hop, via the hub; the legend.
        see(str(R.string.route_to, "crest"))
        see(plural(R.plurals.hops_n, 1, 1) + " · " + str(R.string.via, "hub"), substring = true)
        for (l in listOf(R.string.legend_good, R.string.legend_fair, R.string.legend_weak, R.string.legend_note)) see(l)
        assertFalse(has(R.string.location_sharing))
        // Each leg, as TalkBack reads it: the hub's measured, the crest's not.
        val route = model.nodeRoute(crest.key)!!
        assertTrue(hasDesc(str(R.string.leg_said, str(R.string.you), "hub", dbText(route.legs[0].snrDb!!))))
        assertTrue(hasDesc(str(R.string.leg_said, "hub", "crest", str(R.string.not_measured))))
        // The hub tapped on the map: its own route.
        tap("hub")
        see(str(R.string.route_to, "hub"))
        assertTrue(hasDesc(str(R.string.leg_said, str(R.string.you), "hub", dbText(model.nodeRoute(model.nodes.first { it.name == "hub" }.key)!!.legs[0].snrDb!!))))
        // Closed: the share panel is back.
        tapDesc(R.string.close_route)
        assertNull(model.mapRouteTo)
        see(R.string.location_sharing)
        // The legend in the layers menu too.
        tapDesc(R.string.map_layers)
        assertTrue(compose.onAllNodes(hasText(str(R.string.legend_note)) and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty())
    }

    /** MP-4: an area of the map to keep offline is offered: Map layers › Download this area,
     * once the map shows one, with what it is for; no offline map kept yet. (The download itself
     * is MapLibre's, over the Internet: asked for here, not run.) */
    @Test
    fun MP_4_offlineMapsOffered() = offlineOffered()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_4_offlineMapsOffered_fr() = offlineOffered()

    private fun offlineOffered() {
        launch("demo" to true, "tab" to "map")
        advanceUntil("the nodes on the map", 30) { has("hub") }
        tapDesc(R.string.map_layers)
        val download = compose.onNode(hasText(str(R.string.download_area)) and hasAnyAncestor(isPopup()))
        download.assertIsDisplayed()
        download.assertIsEnabled()
        see(R.string.offline_note)
        assertFalse("no offline map kept yet", has(str(R.string.offline_maps_count, 0, "").substringBefore("("), substring = true))
        // Asked for: the menu closes, the map stays (the download goes on in MapLibre).
        download.performClick()
        compose.waitForIdle()
        assertFalse(has(R.string.offline_note))
        assertTrue(has("hub"))
    }

    /** MP-5: our SOS: on, sent to the group, stopped. */
    @Test
    fun MP_5_sendAndStopAnSos() = ourSos()

    @Test
    @Config(qualifiers = "+fr-night")
    fun MP_5_sendAndStopAnSos_frDark() = ourSos()

    private fun ourSos() {
        launch("demo" to true)
        advanceUntil("the demo group", 300) { model.groups.isNotEmpty() }
        tab(R.string.tab_map)
        tap(R.string.sos)
        // Held three seconds; its accessibility action (TalkBack's double tap) fires at once.
        compose.onAllNodesWithContentDescription(str(R.string.send_sos)).onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        assertTrue(model.sos)
        see(R.string.stop_sos)
        advanceUntil("sent", 60) { model.sosStatus?.lastQueuedS != null }
        see(str(R.string.sos_sent_to, "", 0).substringBefore(" "), substring = true)
        tap(R.string.stop_sos)
        assertFalse(model.sos)
        see(R.string.hold_3s)
    }

    /** MP-5: Julien's SOS (no GPS) and Camille's (with GPS, five minutes): the alert for each;
     * Camille's ends. */
    @Test
    fun MP_5_receiveSos() = theirSos()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_5_receiveSos_fr() = theirSos()

    private fun theirSos() {
        launch("demo" to true, "sosdemo2" to true)
        val julien = { model.contacts.firstOrNull { it.name == "Julien" }?.key?.hex() }
        advanceUntil("Julien's SOS", 900) { julien()?.let { model.incidents[it] } != null }
        advanceUntil("his alert", 10) { model.sosAlert == julien() }
        see("Julien")
        see(R.string.no_gps)
        see(str(R.string.battery_suffix, 64).trim(), substring = true)
        tap(R.string.dismiss)
        // Camille's, with her position: where she is, in figures any reader takes.
        val camille = contact("Camille").key.hex()
        advanceUntil("Camille's SOS", 600) { model.incidents[camille]?.position != null && model.sosAlert == camille }
        see("Camille")
        see(R.string.last_gps)
        val where = Regex("""\d+\.\d{4}, \d+\.\d{4}""")
        assertTrue("her coordinates: ${shown()}", shown().any { where.containsMatchIn(it) })
        tap(R.string.dismiss)
        // Her SOS ends at 540 s; her marker is no longer an SOS.
        advanceUntil("Camille's SOS over", 600) { model.positions.any { it.from.hex() == camille && !it.sos } }
    }

    /** MP-5: the SOS button in the Map's Locate mode too: on, sent to the group, stopped; the
     * Map still in Locate mode after. */
    @Test
    fun MP_5_sosFromLocateMode() = sosFromLocate()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_5_sosFromLocateMode_fr() = sosFromLocate()

    private fun sosFromLocate() {
        launch("demo" to true, "tab" to "map", "mapMode" to "locate")
        advanceUntil("the demo group", 300) { model.groups.isNotEmpty() }
        see(R.string.no_search)
        tap(R.string.sos)
        compose.onAllNodesWithContentDescription(str(R.string.send_sos)).onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        assertTrue(model.sos)
        advanceUntil("sent", 60) { model.sosStatus?.lastQueuedS != null }
        tap(R.string.stop_sos)
        assertFalse(model.sos)
        tapDesc(R.string.close)
        assertEquals(AppModel.MapMode.LOCATE, model.mapMode)
        see(R.string.no_search)
    }

    /** MP-6: Locate is a mode of the Map: the debug extra `tab locate` opens it; Julien's SOS
     * counts on the Map tab; Locate shows his search, Positions the map; the two switch. */
    @Test
    fun MP_6_locateIsAModeOfTheMap() = locateMode()

    @Test
    @Config(qualifiers = "+fr-night")
    fun MP_6_locateIsAModeOfTheMap_frDark() = locateMode()

    private fun locateMode() {
        launch("demo" to true, "sosdemo" to true, "tab" to "locate")
        see(R.string.no_search)
        val julien = { model.contacts.firstOrNull { it.name == "Julien" }?.key?.hex() }
        advanceUntil("his alert", 900) { julien() != null && model.sosAlert == julien() }
        tap(R.string.dismiss)
        // His SOS on the Map tab's badge, said with its name (there is no Locate tab any more).
        compose.onNode(hasText(str(R.string.tab_map)) and isTab)
            .assert(hasContentDescription(str(R.string.tab_map) + ", " + plural(R.plurals.sos_active, 1, 1)))
        see(str(R.string.search_name, "Julien"))
        see(R.string.start_search)
        // Positions: the map and its share panel, not the search.
        tab(R.string.map_positions)
        see(R.string.location_sharing)
        assertFalse(has(str(R.string.search_name, "Julien")))
        tab(R.string.map_locate)
        see(str(R.string.search_name, "Julien"))
        assertFalse(has(R.string.location_sharing))
    }

    private val isTab = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)

    /** MP-6: Julien in SOS without GPS: located from his alert; pinged; the group's readings;
     * an estimate. */
    @Test
    fun MP_6_locateAMemberWithoutGps() = locate()

    @Test
    @Config(qualifiers = "+fr")
    fun MP_6_locateAMemberWithoutGps_fr() = locate()

    private fun locate() {
        launch("demo" to true, "sosdemo" to true)
        val julien = { model.contacts.firstOrNull { it.name == "Julien" }?.key?.hex() }
        advanceUntil("his alert", 900) { julien() != null && model.sosAlert == julien() }
        val key = contact("Julien").key
        tap(R.string.start_search)
        assertNotNull(model.search(julien()!!))
        // The Map, in its Locate mode.
        assertEquals(AppModel.MapMode.LOCATE, model.mapMode)
        see(str(R.string.search_name, "Julien"))
        // Ten minutes, pinged whenever the button allows ("Ping in 12 s" counts down on the
        // screen's own clock, B-and-21; hearing him starts it again).
        scrollTo(hasText(str(R.string.ping_in, 0).substringBefore(" "), substring = true))
        var pings = 0
        repeat(600) {
            if (has(R.string.ping)) {
                tap(R.string.ping)
                pings++
            }
            withScreen(1)
        }
        assertTrue("pinged: $pings", pings >= 2)
        val ms = model.measurements(key)
        // Readings shared by the group (ours too when our radio hears him directly).
        assertTrue("Camille's: ${ms.map { it.mine }}", ms.any { !it.mine })
        assertNotNull("an estimate", model.searchEstimate(key))
        see(str(R.string.measures_count, ms.size).substringAfter(" "), substring = true)
        tap(R.string.stop_search)
        assertNull(model.search(julien()!!))
    }
}
