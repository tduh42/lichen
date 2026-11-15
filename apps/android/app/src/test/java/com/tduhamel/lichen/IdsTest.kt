package com.tduhamel.lichen

import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.isHex
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Review of 2026-11-15, SR-25: the activity is exported, so its extras can come from any app. */
class IdsTest {
    private val key = "ab".repeat(32)

    @Test
    fun routesAreChecked() {
        assertEquals(ChatId(ChatId.Kind.CONTACT, key), ChatId.fromRoute("c$key"))
        assertEquals(ChatId(ChatId.Kind.CONTACT, key), ChatId.fromRoute("c" + key.uppercase()))
        assertEquals(ChatId.Kind.GROUP, ChatId.fromRoute("g" + "01".repeat(16))?.kind)
        assertEquals(ChatId.Kind.CHANNEL, ChatId.fromRoute("h" + "01".repeat(8))?.kind)
        for (bad in listOf("", "c", "cxyz", "c" + "zz".repeat(32), "c" + key.drop(2), "g$key", "x$key", "c${key}00")) {
            assertNull(bad, ChatId.fromRoute(bad))
        }
    }

    @Test
    fun hex() {
        assertTrue(key.isHex(32))
        assertFalse("ab".isHex(32))
        assertFalse("g".repeat(64).isHex(32))
    }
}
