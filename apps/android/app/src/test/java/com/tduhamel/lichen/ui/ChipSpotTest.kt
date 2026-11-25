package com.tduhamel.lichen.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Where a route's leg chip goes on the map (MP-3): never over a marker or its label. */
class ChipSpotTest {
    private val chip = Size(60f, 20f)

    /** A marker at [p]: its avatar and the label under it. */
    private fun marker(p: Offset) = Rect(p.x - 40f, p.y - 20f, p.x + 40f, p.y + 40f)

    @Test
    fun aLongLegHasItsChipAtItsMiddle() {
        val a = Offset(0f, 0f)
        val b = Offset(600f, 0f)
        assertEquals(Offset(300f, 0f), chipSpot(a, b, chip, listOf(marker(a), marker(b)), 3f))
    }

    @Test
    fun aChipMovesAlongItsLegAwayFromAMarkerAtTheMiddle() {
        val a = Offset(0f, 0f)
        val b = Offset(600f, 0f)
        val spot = chipSpot(a, b, chip, listOf(marker(a), marker(b), marker(Offset(300f, 0f))), 3f)
        assertNotNull(spot)
        assertTrue("off the middle marker: $spot", spot!!.x < 220f || spot.x > 380f)
        assertEquals(0f, spot.y)
    }

    @Test
    fun aLegShorterThanItsTwoMarkersHasNoChip() {
        // The hub and our dot, 100 px apart, as on the emulator: the chip went over the hub's name.
        val a = Offset(0f, 0f)
        val b = Offset(70f, 70f)
        assertNull(chipSpot(a, b, chip, listOf(marker(a), marker(b)), 3f))
    }
}
