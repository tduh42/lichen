package com.tduhamel.lichen

import com.tduhamel.lichen.core.DemoNetwork
import com.tduhamel.lichen.core.LichenCore
import com.tduhamel.lichen.core.LichenEvent
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.UsbDecoder
import com.tduhamel.lichen.core.decodeVoice
import com.tduhamel.lichen.core.prepareVoice
import com.tduhamel.lichen.core.usbEncode
import com.tduhamel.lichen.model.ChatId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Kotlin bindings against the host build of the core (target/release, through JNA):
 * the demo network from Kotlin, as the app drives it, in simulated time.
 */
class CoreBindingsTest {
    @Test
    fun demoNetworkFromKotlin() {
        val demo = DemoNetwork()
        val core = LichenCore("test", ByteArray(32) { 7 }, ByteArray(32) { 107 }, null)
        // The demo's people are friends, as in the app's demo (§10.1, ADR-0046).
        core.setGroupConsent(false)
        core.setRequestConsent(false)
        val wall = 1_793_000_000_000uL
        core.setClock(wall, 0uL)
        core.connect(0uL)
        val texts = mutableListOf<String>()
        var joined = false
        var delivered = false
        var sent = false
        var now = 0uL
        while (now < 10uL * 60_000uL) {
            for (f in demo.read(now)) core.onCompanionFrame(f, now)
            core.setClock(wall + now, now)
            core.tick(now)
            while (true) demo.write(core.pollCompanion(now) ?: break, now)
            while (true) {
                when (val e = core.pollEvent() ?: break) {
                    is LichenEvent.Received -> (e.message as? Message.Text)?.let { texts += it.text }
                    is LichenEvent.GroupJoined -> joined = true
                    is LichenEvent.Delivered -> delivered = true
                    else -> Unit
                }
            }
            val camille = core.contacts().firstOrNull { it.name == "Camille" }
            if (!sent && camille != null && texts.isNotEmpty()) {
                core.sendText(camille.key, "On my way", 1u, now)
                sent = true
            }
            now += 200uL
        }
        assertTrue("Camille wrote", texts.isNotEmpty())
        assertTrue("our answer was delivered", delivered)
        assertTrue("joined the demo group", joined)
        assertTrue(core.conversations().isNotEmpty())
        core.close()
        demo.close()
    }

    @Test
    fun voiceRoundTrip() {
        // Two seconds of a voiced, speech-like signal (a gliding pitch and its harmonics; Codec2
        // models speech, a lone sine comes out flat), through each mode the apps offer and back.
        var phase = 0.0
        val pcm = List(16_000) { i ->
            val t = i / 8_000.0
            phase += 2 * Math.PI * (140 + 40 * kotlin.math.sin(t * 2.1)) / 8_000
            val env = maxOf(0.05, 0.5 + 0.5 * kotlin.math.sin(t * 2 * Math.PI * 3))
            val s = (1 until 8).sumOf { h -> kotlin.math.sin(phase * h) / h }
            (s * env * 6_000).toInt().toShort()
        }
        for (mode in listOf(0, 2, 3)) {
            val v = prepareVoice(pcm, mode.toUByte())
            assertEquals(mode, v.mode.toInt())
            assertTrue("about 2 s", v.durationMs.toInt() in 1_900..2_100)
            val back = decodeVoice(v.mode, v.frames, v.data)!!
            assertTrue("decoded samples", back.size > 15_000)
            val energy = back.sumOf { it.toLong() * it }.toDouble() / back.size
            assertTrue("mode $mode: not silence", energy > 1e5)
        }
    }

    @Test
    fun usbFraming() {
        val frame = byteArrayOf(0x84.toByte(), 1, 2, 3)
        val wire = usbEncode(frame)
        assertEquals('<'.code.toByte(), wire[0])
        val d = UsbDecoder()
        val out = d.push(byteArrayOf('>'.code.toByte(), 4, 0) + frame)
        assertEquals(1, out.size)
        assertTrue(out[0].contentEquals(frame))
    }

    @Test
    fun chatIdsCompareByContent() {
        val a = ChatId.contact(ByteArray(32) { it.toByte() })
        val b = ChatId.of(a.ref)
        assertEquals(a, b)
        assertEquals(a, ChatId.fromRoute(a.route))
        assertEquals(setOf(a), setOf(b))
    }
}
