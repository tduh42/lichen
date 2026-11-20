package com.tduhamel.lichen.ui

import android.app.Application
import android.app.KeyguardManager
import android.content.Intent
import android.content.res.Configuration
import android.hardware.biometrics.BiometricManager
import android.net.Uri
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.BuildConfig
import com.tduhamel.lichen.R
import com.tduhamel.lichen.lockPromptForTests
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.model.AppLock
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.problemReport
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowBiometricManager
import java.io.File
import java.util.Locale

/** Area 9 of docs/testing.md: settings. */
@RunWith(AndroidJUnit4::class)
class SettingsTest : UiTest() {
    /** The system's lock prompt, which Robolectric cannot show (it never goes idle): stood in
     * for, and answered by [authenticate]. */
    @Before
    fun prompt() {
        lockPromptForTests = {}
    }

    @After
    fun noPrompt() {
        lockPromptForTests = null
    }

    /** This phone has a screen lock and a fingerprint, as the app lock needs. */
    private fun secure() {
        shadowOf(context.getSystemService(KeyguardManager::class.java)).setIsDeviceSecure(true)
        Shadow.extract<ShadowBiometricManager>(context.getSystemService(BiometricManager::class.java)).apply {
            setCanAuthenticate(true)
            setAuthenticatorType(BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
        }
    }

    /** This phone has no screen lock (nor fingerprint). */
    private fun insecure() {
        shadowOf(context.getSystemService(KeyguardManager::class.java)).setIsDeviceSecure(false)
        Shadow.extract<ShadowBiometricManager>(context.getSystemService(BiometricManager::class.java)).setCanAuthenticate(false)
    }

    /** The system's prompt (fingerprint, face or screen lock) answered by its owner. */
    private fun authenticate(purpose: AppLock.Purpose) {
        if (model.lock.pending == null) model.lock.started(purpose)
        model.lock.finished(true)
        compose.waitForIdle()
    }

    private fun settings() {
        tab(R.string.tab_settings)
    }

    /** ST-1: Privacy: the lock refused on a phone without a screen lock; on a phone with one,
     * on, its delay chosen, both kept; off again, kept off. */
    @Test
    fun ST_1_privacySettings() = privacy()

    @Test
    @Config(qualifiers = "+fr")
    fun ST_1_privacySettings_fr() = privacy()

    private fun privacy() {
        insecure()
        launch("demo" to true)
        settings()
        see(R.string.privacy)
        // No screen lock: said so, the lock stays off.
        toggle(R.string.lock_lichen)
        text(R.string.lock_stays_off).assertIsDisplayed()
        tapInDialog(R.string.ok)
        assertFalse(model.lock.enabled)
        // A screen lock: on after the owner's say-so; 15 minutes.
        secure()
        toggle(R.string.lock_lichen)
        authenticate(AppLock.Purpose.ENABLE)
        assertTrue(model.lock.enabled)
        tap(R.string.lock_after)
        tapInDialog(R.string.lock_15_minutes)
        assertEquals(900, model.lock.delay)
        // Kept: the next start is locked, with the same delay.
        restart("demo" to true)
        assertTrue(model.lock.enabled && model.lock.locked)
        authenticate(AppLock.Purpose.UNLOCK)
        assertEquals(900, model.lock.delay)
        settings()
        see(R.string.lock_15_minutes)
        // Off, and kept off.
        toggle(R.string.lock_lichen)
        assertFalse(model.lock.enabled)
        restart("demo" to true)
        assertFalse(model.lock.enabled || model.lock.locked)
    }

    /** ST-2: the app lock: the cover at launch, gone once unlocked; back after its delay away,
     * not before. */
    @Test
    fun ST_2_appLock() = appLock()

    @Test
    @Config(qualifiers = "+fr-night")
    fun ST_2_appLock_frDark() = appLock()

    private fun appLock() {
        secure()
        launch("demo" to true)
        settings()
        toggle(R.string.lock_lichen)
        authenticate(AppLock.Purpose.ENABLE)
        tap(R.string.lock_after)
        tapInDialog(R.string.lock_1_minute)
        // A cold start: the cover, nothing of the app behind it.
        restart("demo" to true)
        see(R.string.unlock)
        assertTrue(model.lock.locked)
        authenticate(AppLock.Purpose.UNLOCK)
        assertFalse(has(R.string.unlock))
        // Away 30 s: still open. Away 61 s: locked.
        awayFor(30)
        assertFalse(model.lock.locked)
        awayFor(61)
        assertTrue(model.lock.locked)
        see(R.string.unlock)
        authenticate(AppLock.Purpose.UNLOCK)
        assertFalse(has(R.string.unlock))
    }

    /** ST-3: a backup exported from a phone on a radio (a message and a group of its own, its
     * radio's contacts); another phone restores it with the 24 words: all of it. */
    @Test
    fun ST_3_backupExportAndRestore(): Unit = FakeTcpRadio().use { radio ->
        launch("onboarding" to false, "tcp" to radio.address)
        waitReal("the radio's contacts") { model.contacts.any { it.name == "Camille" } }
        // Her MeshCore chat (her lichen identity comes with her first message).
        val camille = ChatId.mesh(model.contacts.first { it.name == "Camille" }.key)
        assertEquals(null, model.send("Leaving at dawn", camille))
        model.createGroup("Ridge walk", emptyList())
        val contacts = model.contacts.map { it.name }.sorted()
        val words = model.recoveryPhrase()
        val fingerprint = model.fingerprint
        val file = File(context.cacheDir, "test.lichen-backup").apply { delete() }
        // Exported: the system asks where; the file written there.
        settings()
        tap(R.string.export_backup)
        answerNextActivity(Intent().setData(Uri.fromFile(file)))
        waitReal("the backup", 30_000) { has(R.string.backup_saved) }
        tapInDialog(R.string.ok)
        assertTrue(file.length() > 0)

        // Another phone: restored from the welcome screen.
        newPhone()
        assertTrue(model.contacts.isEmpty())
        tap(R.string.restore_backup)
        tap(R.string.choose_backup_file)
        answerNextActivity(Intent().setData(Uri.fromFile(file)))
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(words.joinToString(" "))
        tap(R.string.restore)
        waitReal("restored", 30_000) { model.contacts.isNotEmpty() }
        assertEquals(fingerprint, model.fingerprint)
        assertEquals(contacts, model.contacts.map { it.name }.sorted())
        assertTrue(model.groups.any { it.name == "Ridge walk" })
        val chat = ChatId.mesh(model.contacts.first { it.name == "Camille" }.key)
        assertEquals("Leaving at dawn", (model.last(chat)?.message as? Message.Text)?.text)
        file.delete()
        Unit
    }

    /** ST-4: the problem report: built, shared as a file, without names or keys in it. */
    @Test
    fun ST_4_problemReport() {
        launch("demo" to true)
        advanceUntil("the demo's people", 300) { model.contacts.size >= 3 && model.groups.isNotEmpty() }
        settings()
        tap(R.string.report_problem)
        see(R.string.report_share)
        val report = model.problemReport()
        for (name in model.contacts.map { it.name } + model.groups.map { it.name } + listOf(model.nodeName)) {
            assertFalse("$name in the report", name.isNotEmpty() && report.contains(name))
        }
        for (key in model.contacts.map { it.key.hex() } + listOfNotNull(model.nodeKey?.hex())) {
            assertFalse("a key in the report", report.contains(key.take(12)))
        }
        for (word in model.recoveryPhrase().take(6)) assertFalse("a phrase word", Regex("\\b$word\\b").containsMatchIn(report.lowercase()))
        val app = shadowOf(context as Application)
        while (app.nextStartedActivity != null) Unit
        tap(R.string.report_share)
        val sent = app.nextStartedActivity
        assertEquals(Intent.ACTION_CHOOSER, sent?.action)
    }

    /** ST-5: About: the version and build of this release. */
    @Test
    fun ST_5_aboutAndVersion() {
        launch("demo" to true)
        settings()
        see("${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})")
    }

    /** ST-6: French throughout: no English string of the app on the main screens. (A tall screen,
     * so that every list shows all of itself.) */
    @Test
    @Config(qualifiers = "+fr-h6000dp")
    fun ST_6_frenchThroughout() {
        val english = englishOnly()
        val found = mutableMapOf<String, List<String>>()
        fun check(screen: String) {
            compose.waitForIdle()
            val left = shown().filter { it in english }
            if (left.isNotEmpty()) found[screen] = left
        }
        launch("demo" to true)
        advanceUntil("the demo group", 300) { model.groups.isNotEmpty() }
        check("Messages")
        for (t in listOf(R.string.tab_map, R.string.tab_search, R.string.tab_settings)) {
            tab(t)
            check(str(t))
        }
        for (screen in listOf("radio", "identity", "contact", "pairing", "file", "media")) {
            restart("demo" to true, "screen" to screen, "open" to "Camille")
            advance(5)
            check(screen)
        }
        restart("demo" to true, "open" to "Rando Vercors")
        advanceUntil("the group's chat", 300) { has(R.string.message_placeholder) }
        check("group")
        restart("demo" to true, "open" to "Marc")
        advanceUntil("Marc's chat", 120) { has(R.string.message_placeholder) }
        check("Marc")
        assertEquals(emptyMap<String, List<String>>(), found)
    }

    /** ST-6: figures in French: the radio's frequency and its sub-band both with a decimal
     * comma (B-and-25). */
    @Test
    @Config(qualifiers = "+fr")
    fun ST_6_frenchFigures_fr() {
        launch("demo" to true)
        advanceUntil("the radio's settings") { model.radioSettings != null }
        settings()
        see("869,618 MHz", substring = true)
        see("869,4–869,65 MHz", substring = true)
    }

    /** The app's English strings that French writes otherwise. */
    private fun englishOnly(): Set<String> {
        val en = context.createConfigurationContext(Configuration(context.resources.configuration).apply { setLocale(Locale.ENGLISH) }).resources
        val fr = context.resources
        return R.string::class.java.fields.mapNotNull { f ->
            val id = f.getInt(null)
            val e = en.getText(id).toString()
            e.takeIf { it.length >= 4 && it != fr.getText(id).toString() }
        }.toSet()
    }

    /** ST-8: bridge settings: iroh and relay only, applied and kept. (Tor reaches the Internet:
     * the emulator's.) */
    @Test
    fun ST_8_bridgeSettings() = bridge()

    @Test
    @Config(qualifiers = "+fr")
    fun ST_8_bridgeSettings_fr() = bridge()

    private fun bridge() {
        launch("demo" to true)
        settings()
        see(R.string.internet_transports)
        assertTrue(model.irohEnabled)
        toggle(R.string.relay_only)
        assertTrue(model.relayOnly)
        toggle(R.string.iroh_direct)
        assertFalse(model.irohEnabled)
        restart("demo" to true)
        assertFalse(model.irohEnabled)
        assertTrue(model.relayOnly)
        settings()
        toggle(R.string.iroh_direct)
        toggle(R.string.relay_only)
        restart("demo" to true)
        assertTrue(model.irohEnabled)
        assertFalse(model.relayOnly)
    }

    /** ST-9: accessibility: on the main screens, everything that can be tapped says what it is
     * and is at least 48 dp square. */
    @Test
    fun ST_9_accessibility() {
        val problems = mutableListOf<String>()
        fun audit(screen: String) {
            compose.waitForIdle()
            val density = context.resources.displayMetrics.density
            for (root in compose.onAllNodes(isRoot()).fetchSemanticsNodes()) {
                fun walk(n: SemanticsNode) {
                    val c = n.config
                    val tappable = c.getOrNull(SemanticsActions.OnClick) != null && c.getOrNull(SemanticsProperties.Disabled) == null
                    // Not on screen (no size): nothing to reach.
                    if (tappable && n.touchBoundsInRoot.width > 0f && n.touchBoundsInRoot.height > 0f) {
                        val merged = n.config
                        val label = (merged.getOrNull(SemanticsProperties.ContentDescription).orEmpty() +
                            merged.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
                            listOfNotNull(merged.getOrNull(SemanticsActions.OnClick)?.label)).joinToString(" ").trim()
                        // The area a finger reaches (Compose widens small ones to 48 dp).
                        val w = n.touchBoundsInRoot.width / density
                        val h = n.touchBoundsInRoot.height / density
                        if (label.isEmpty()) {
                            val inside = n.children.flatMap { c -> c.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } }
                            problems += "$screen: an unlabelled button ${w.toInt()}×${h.toInt()} dp holding $inside"
                        }
                        if (w < 47.5f || h < 47.5f) problems += "$screen: \"$label\" is ${w.toInt()}×${h.toInt()} dp"
                    }
                    n.children.forEach(::walk)
                }
                walk(root)
            }
        }
        launch("demo" to true)
        advanceUntil("the demo group", 300) { model.groups.isNotEmpty() }
        audit("Messages")
        for (t in listOf(R.string.tab_map, R.string.tab_search, R.string.tab_settings)) {
            tab(t)
            audit(str(t))
        }
        restart("demo" to true, "open" to "Camille")
        advanceUntil("her chat", 120) { hasDesc(str(R.string.attach)) }
        audit("chat")
        restart("demo" to true, "open" to "Rando Vercors")
        advanceUntil("the group's chat", 300) { has(R.string.message_placeholder) }
        audit("group")
        assertEquals(emptyList<String>(), problems)
    }
}
