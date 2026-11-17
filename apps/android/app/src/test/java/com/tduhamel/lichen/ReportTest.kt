package com.tduhamel.lichen

import com.tduhamel.lichen.core.LichenCore
import com.tduhamel.lichen.core.ReportApp
import com.tduhamel.lichen.core.linkRetryDelayMs
import com.tduhamel.lichen.core.tcpAddress
import com.tduhamel.lichen.model.AppLog
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Problem reports (ADR-0045) and the TCP link's rules, through the Kotlin bindings. */
class ReportTest {
    @Test
    fun theReportMasksWhatTheAppAdds() {
        val core = LichenCore("test", ByteArray(32) { 3 }, ByteArray(32) { 103 }, null)
        val key = core.key()!!.joinToString("") { "%02x".format(it) }
        val log = AppLog { 1_000uL }
        log.note("Bluetooth: connected to Morgan's radio")
        log.note("key $key at 45.123456,6.654321")
        val app = ReportApp(
            appVersion = "0.9.0 (1)",
            platform = "Android 16 (API 36)",
            device = "Google Pixel 9",
            link = "TCP",
            settings = listOf("Tor: off"),
            log = log.lines(),
            privateWords = listOf("Morgan"),
        )
        val report = core.problemReport(app, 2_000uL)
        assertTrue(report, report.contains("Bluetooth: connected to [name]'s radio"))
        assertTrue(report, report.contains("Setting:   Tor: off"))
        for (secret in listOf("Morgan", key, key.take(8), "45.123", "6.654")) {
            assertFalse("the report shows $secret", report.contains(secret, ignoreCase = true))
        }
        core.close()
    }

    @Test
    fun theAppLogKeepsItsLastLines() {
        val log = AppLog { 0uL }
        repeat(AppLog.MAX + 5) { log.note("line $it") }
        val lines = log.lines()
        assertEquals(AppLog.MAX, lines.size)
        assertEquals("line 5", lines.first().text)
    }

    @Test
    fun tcpAddressesAndRetries() {
        assertEquals(5000, tcpAddress("10.0.2.2")?.port?.toInt())
        assertEquals("fe80::1", tcpAddress("[fe80::1]:5001")?.host)
        assertNull(tcpAddress("radio:0"))
        assertEquals(listOf(1_000uL, 2_000uL, 4_000uL, 8_000uL, 16_000uL, 30_000uL), (1u..6u).map { linkRetryDelayMs(it) })
    }
}
