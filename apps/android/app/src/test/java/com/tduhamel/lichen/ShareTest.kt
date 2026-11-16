package com.tduhamel.lichen

import com.tduhamel.lichen.model.IncomingShare
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** ADR-0034: what another app shares is checked before anything is read (SR-25). */
class ShareTest {
    private val own = "com.tduhamel.lichen"

    @Test
    fun onlyOtherAppsContentUris() {
        assertTrue(IncomingShare.acceptable("content", "com.android.providers.media.documents", own))
        assertFalse(IncomingShare.acceptable("file", "", own))
        assertFalse(IncomingShare.acceptable("file", null, own))
        assertFalse(IncomingShare.acceptable("content", "com.tduhamel.lichen.files", own))
        assertFalse(IncomingShare.acceptable("content", own, own))
        assertFalse(IncomingShare.acceptable("content", "", own))
        assertFalse(IncomingShare.acceptable("https", "example.org", own))
        assertFalse(IncomingShare.acceptable(null, "example.org", own))
    }
}
