package com.tduhamel.lichen

import com.tduhamel.lichen.core.NodeInfo
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.SignalRating
import com.tduhamel.lichen.model.dbText
import com.tduhamel.lichen.model.heardWithin
import com.tduhamel.lichen.model.nodeSections
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

/** The Nodes tab's sections and its "Heard in 2 h" filter (ADR-0049), on nodes made up here. */
class NodeSectionsTest {
    private val now = 1_800_000_000L

    private fun node(
        name: String,
        hops: Int? = 1,
        heardAgo: Long? = 60,
        signal: SignalRating = SignalRating.NONE,
        snr: Float? = null,
        readAgo: Long? = null,
    ) = NodeInfo(
        byteArrayOf(name.length.toByte()), name, NodeKind.REPEATER, hops?.toUByte(), heardAgo?.let { (now - it).toULong() },
        null, null, true, false, snr, null, readAgo?.let { (now - it).toULong() }, signal,
    )

    @Test
    fun inReachByRatingThenSnr() {
        val (reach, _) = nodeSections(
            listOf(
                node("bad", signal = SignalRating.BAD, snr = -9.5f, readAgo = 60),
                node("fair", signal = SignalRating.FAIR, snr = -4f, readAgo = 60),
                node("good low", signal = SignalRating.GOOD, snr = 1f, readAgo = 60),
                node("good high", signal = SignalRating.GOOD, snr = 7.5f, readAgo = 60),
            ),
            now,
        )
        assertEquals(listOf("good high", "good low", "fair", "bad"), reach.map { it.name })
    }

    @Test
    fun throughRepeatersByHopsThenLastHeardPathUnknownLast() {
        val (reach, through) = nodeSections(
            listOf(
                node("unknown path", hops = null, heardAgo = 10),
                node("two hops", hops = 2, heardAgo = 10),
                node("one hop, older", hops = 1, heardAgo = 600),
                node("one hop", hops = 1, heardAgo = 30),
                // A reading a day and more old: no longer in reach.
                node("read yesterday", hops = 3, signal = SignalRating.GOOD, snr = 5f, readAgo = 90_000),
            ),
            now,
        )
        assertTrue(reach.isEmpty())
        assertEquals(listOf("one hop", "one hop, older", "two hops", "read yesterday", "unknown path"), through.map { it.name })
    }

    @Test
    fun heardInTheLastTwoHours() {
        assertTrue(node("now", heardAgo = 60).heardWithin(now, 7_200))
        assertTrue(node("two hours", heardAgo = 7_200).heardWithin(now, 7_200))
        assertFalse(node("three hours", heardAgo = 10_800).heardWithin(now, 7_200))
        assertFalse(node("never", heardAgo = null).heardWithin(now, 7_200))
    }

    @Test
    fun decibelsWithATrueMinusAndTheLanguagesDecimal() {
        val before = Locale.getDefault()
        try {
            Locale.setDefault(Locale.ENGLISH)
            assertEquals("7.5 dB", dbText(7.5f))
            assertEquals("−3.0 dB", dbText(-3f))
            assertEquals("0.0 dB", dbText(-0.01f))
            Locale.setDefault(Locale.FRENCH)
            assertEquals("7,5 dB", dbText(7.5f))
            assertEquals("−9,5 dB", dbText(-9.5f))
        } finally {
            Locale.setDefault(before)
        }
    }
}
