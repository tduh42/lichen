package com.tduhamel.lichen.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.SharePrecision
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hex
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
 * lists the markers' labels where the map would be (MP-4, offline maps, is the emulator's). */
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
