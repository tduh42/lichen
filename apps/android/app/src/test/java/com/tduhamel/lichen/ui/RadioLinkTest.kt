package com.tduhamel.lichen.ui

import android.content.res.Configuration
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.FailReason
import com.tduhamel.lichen.core.LinkKind
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.model.ChatId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Area 2 of docs/testing.md: the radio link and the radio's settings. */
@RunWith(AndroidJUnit4::class)
class RadioLinkTest : UiTest() {
    /** RL-1: the demo radio connects; its contacts and channels arrive. */
    @Test
    fun RL_1_demoRadioConnects() = demoRadioConnects()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_1_demoRadioConnects_fr() {
        assertEquals("Nouveau message", str(R.string.new_message))
        demoRadioConnects()
    }

    @Test
    @Config(qualifiers = "+fr-night")
    fun RL_1_demoRadioConnects_frDark() {
        val night = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        assertEquals(Configuration.UI_MODE_NIGHT_YES, night)
        demoRadioConnects()
    }

    private fun demoRadioConnects() {
        launch("demo" to true)
        text(R.string.radio_connected, "ThinkNode M5").assertIsDisplayed()
        // The radio's channel in Messages, once the radio's channels are read (about 3 s), then
        // what Lucie posts on it at 12 s.
        advance(5)
        text("Public").assertIsDisplayed()
        advance(10)
        text("Fog on the Pas de la Ville", substring = true).assertIsDisplayed()
        // Its contacts: the people one can write to.
        tapDesc(R.string.new_message)
        text("Camille").assertIsDisplayed()
        text("Julien").assertIsDisplayed()
    }

    /** RL-2: the radio drops with texts waiting, then comes back: they go, once each. */
    @Test
    fun RL_2_radioDropsAndComesBack() = dropsAndComesBack()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_2_radioDropsAndComesBack_fr() = dropsAndComesBack()

    private fun dropsAndComesBack() {
        launch("demo" to true, "offline" to 120, "online" to 180)
        advanceUntil("the demo group") { model.groups.isNotEmpty() }
        val camille = ChatId.contact(contact("Camille").key)
        val group = ChatId.group(model.groups.first().gid)
        advanceUntil("the radio drops", 130) { !model.isConnected }
        text(R.string.no_radio).assertIsDisplayed()
        text(R.string.radio_disconnected).assertIsDisplayed()
        text(R.string.messages_wait_for_radio).assertIsDisplayed()
        text(R.string.debug_offline_text).assertIsDisplayed()
        text(str(R.string.debug_offline_group), substring = true).assertIsDisplayed()
        assertTrue(model.last(camille)!!.status is MessageStatus.Waiting)

        advanceUntil("the radio is back", 70) { model.isConnected }
        text(R.string.radio_connected, "ThinkNode M5").assertIsDisplayed()
        assertFalse(has(R.string.radio_disconnected))
        advanceUntil("the text to Camille delivered", 400, { model.chats[camille].orEmpty().filter { it.outgoing }.map { it.status } + " last=" + model.last(camille)?.status }) {
            model.chats[camille].orEmpty().any { it.outgoing && it.status == MessageStatus.Delivered } ||
                model.last(camille)?.status == MessageStatus.Delivered
        }
        advanceUntil("the group post sent") {
            model.chats[group].orEmpty().any { it.outgoing && it.status == MessageStatus.Flooded } ||
                (model.last(group)?.let { it.outgoing && it.status == MessageStatus.Flooded } == true)
        }
        // Once each, in their chats.
        tap("Camille")
        assertEquals(1, compose.onAllNodesWithText(str(R.string.debug_offline_text)).fetchSemanticsNodes().size)
        see(str(R.string.state_delivered), substring = true)
    }

    /** RL-2: in the demo, the banner's button switches the simulated radio back on. */
    @Test
    fun RL_2_bannerConnectsTheDemoRadioAgain() {
        launch("demo" to true, "offline" to 10)
        advanceUntil("the radio drops", 20) { !model.isConnected }
        tap(R.string.connect)
        assertTrue(model.isConnected)
        assertFalse(has(R.string.radio_disconnected))
    }

    /** RL-2 (D-1): a radio that was connected and drops says so ("Radio disconnected"), and the
     * banner goes when the link is back. */
    @Test
    fun RL_2_pairedRadioDropsSaysDisconnected() = FakeTcpRadio().use { radio ->
        launch("onboarding" to false, "tcp" to radio.address)
        waitReal("connected over TCP") { model.isConnected }
        waitReal("the radio saved") { model.savedRadios.isNotEmpty() }
        radio.drop()
        waitReal("the link dropped") { !model.isConnected }
        text(R.string.radio_disconnected).assertIsDisplayed()
        text(R.string.messages_wait_for_radio).assertIsDisplayed()
        assertFalse(has(R.string.no_radio_yet))
        waitReal("the link back") { model.isConnected }
        assertFalse(has(R.string.radio_disconnected))
    }

    /** RL-3: a text that waits longer than its expiry is marked so, and can be sent again. */
    @Test
    fun RL_3_textExpiresWhileOffline() = expires()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_3_textExpiresWhileOffline_fr() = expires()

    private fun expires() {
        launch("demo" to true, "offline" to 40, "online" to 120, "expiry" to 30)
        val camille = ChatId.contact(contact("Camille").key)
        advanceUntil("the radio drops", 50) { !model.isConnected }
        advanceUntil("the text expired", 60) {
            (model.last(camille)?.status as? MessageStatus.Failed)?.reason == FailReason.EXPIRED
        }
        tap("Camille")
        text(R.string.expired).assertIsDisplayed()
        text(R.string.expired_note).assertIsDisplayed()
        compose.onAllNodesWithText(str(R.string.state_not_delivered), substring = true).onFirst().assertIsDisplayed()

        advanceUntil("the radio is back", 90) { model.isConnected }
        tap(R.string.resend)
        advanceUntil("the text delivered") { model.last(camille)?.status == MessageStatus.Delivered }
        assertFalse(has(R.string.expired))
        assertEquals(1, compose.onAllNodesWithText(str(R.string.debug_offline_text)).fetchSemanticsNodes().size)
    }

    /** RL-4: a radio paired over TCP is saved; forgotten, it goes (and the link with it);
     * paired again, it comes back. */
    @Test
    fun RL_4_savedRadiosListForgetReconnect() = savedRadios()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_4_savedRadiosListForgetReconnect_fr() {
        // One radio: singular (B-and-4).
        assertEquals("1 enregistrée", plural(R.plurals.radios_saved, 1, 1))
        savedRadios()
    }

    private fun savedRadios() = FakeTcpRadio().use { radio ->
        launch("onboarding" to false, "tcp" to radio.address)
        waitReal("connected over TCP") { model.isConnected }
        waitReal("the radio saved") { model.savedRadios.isNotEmpty() }
        val saved = model.savedRadios.single()
        assertEquals(LinkKind.TCP, saved.link)
        assertTrue(saved.current)

        tab(R.string.tab_settings)
        text(plural(R.plurals.radios_saved, 1, 1)).assertIsDisplayed()
        tap(R.string.radios)
        text(saved.name).assertIsDisplayed()
        text(R.string.connected).assertIsDisplayed()
        text(str(R.string.link_tcp), substring = true).assertIsDisplayed()

        // Forgotten: off the list, and the link closed.
        tapDesc(str(R.string.radio_actions, saved.name))
        tapInDialog(R.string.forget)
        text(R.string.forget_named, saved.name).assertIsDisplayed()
        tapInDialog(R.string.forget)
        waitReal("forgotten") { model.savedRadios.isEmpty() && !model.isConnected }
        text(R.string.radios_none).assertIsDisplayed()

        // Paired again over TCP: connected, saved again.
        tap(R.string.pair_another_radio)
        scrollTo(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextReplacement(radio.address)
        compose.onAllNodesWithText(str(R.string.connect)).onLast().performClick()
        waitReal("connected again") { model.isConnected }
        waitReal("saved again") { model.savedRadios.size == 1 }
    }

    /** RL-5: the radio's settings read, one changed, read back. */
    @Test
    fun RL_5_radioSettingsReadAndChanged() = settings()

    @Test
    @Config(qualifiers = "+fr-night")
    fun RL_5_radioSettingsReadAndChanged_frDark() = settings()

    private fun settings() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings != null }
        val r = model.radioSettings!!
        text(R.string.radio_settings).assertIsDisplayed()
        text("SF${r.sf} · CR 4/${r.cr}", substring = true).assertIsDisplayed()
        see("${r.txDbm} dBm")
        // One dB less, applied by the radio and read back.
        tap("−")
        advanceUntil("the new power read back", 10) { model.radioSettings?.txDbm == (r.txDbm - 1).toByte() }
        see("${r.txDbm - 1} dBm")
        assertEquals(null, model.radioError)
    }

    /** RL-5: the radio's position put in its adverts, after the warning: this phone's when the
     * radio has no GPS (B-and-22), shown in figures any reader takes, "45.12345, 5.12345",
     * whatever the language (B-and-20). */
    @Test
    fun RL_5_advertisedPositionInFigures() = advertisedPosition()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_5_advertisedPositionInFigures_fr() = advertisedPosition()

    private fun advertisedPosition() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings != null }
        toggle(R.string.position_in_adverts)
        tapInDialog(R.string.show_it)
        advanceUntil("in its adverts", 30) { model.radioSettings?.positionInAdverts == true }
        // Where it is: its GPS's fix, or this phone's.
        if (model.radioSettings?.latE6 == 0 && has(R.string.use_phone_position)) tap(R.string.use_phone_position)
        advanceUntil("its position", 30, { model.radioSettings }) { model.radioSettings?.latE6 != 0 }
        see(R.string.position_advertised)
        val where = Regex("""^\d+\.\d{5}, \d+\.\d{5}$""")
        assertTrue("its coordinates: ${shown()}", shown().any { where.matches(it) })
    }

    /** RL-5 (D-9): a radio slow to answer (its replies held for 7 s): its settings screen says
     * they are being read, neither "Connect a radio" nor zeros as values; then its values. */
    @Test
    fun RL_5_settingsBeingRead(): Unit = beingRead()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_5_settingsBeingRead_fr(): Unit = beingRead()

    private fun beingRead() = FakeTcpRadio().use { radio ->
        radio.holdReplies(7_000)
        launch("onboarding" to false, "tcp" to radio.address, "screen" to "radio")
        waitReal("the link") { model.isConnected }
        // Nothing answered yet (then the values still zeros, RadioSettings.read false): twice.
        repeat(2) {
            pause(1_500)
            assertFalse("read before the radio answered", model.radioSettings?.read == true)
            see(R.string.reading_radio_settings)
            assertFalse(has(R.string.radio_settings_none))
            assertFalse(has(R.string.lora_now))
        }
        // Answered: its values (the LoRa section's "Now", at the top).
        waitReal("its settings read", 30_000) { model.radioSettings?.read == true }
        waitReal("its values shown") { has(R.string.lora_now) }
        assertFalse(has(R.string.reading_radio_settings))
    }

    /** [ms] of real time, the app running (a link with threads of its own). */
    private fun pause(ms: Long) {
        val end = System.nanoTime() + ms * 1_000_000
        while (System.nanoTime() < end) {
            shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(50))
            Thread.sleep(20)
        }
        compose.waitForIdle()
    }

    /** RL-6: a setting the radio cannot take: said so, the old value stays. */
    @Test
    fun RL_6_refusedSettingKeepsTheOldValue() = refused()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_6_refusedSettingKeepsTheOldValue_fr() = refused()

    private fun refused() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings != null }
        val before = model.radioSettings!!
        // A custom frequency far outside any LoRa band.
        compose.onAllNodesWithText(str(R.string.preset)).onLast().assertIsDisplayed()
        tapPresetChoice()
        if (compose.onAllNodes(hasText(str(R.string.preset_custom)) and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty()) {
            tapInDialog(R.string.preset_custom)
        }
        // The frequency field shows the radio's, in MHz.
        val freq = "%.3f".format(before.freqKhz.toDouble() / 1000)
        compose.onNode(hasSetTextAction() and hasText(freq)).performTextReplacement("5000")
        // Its click action, not a touch: under Robolectric a touch elsewhere drops what the field
        // holds (the phone keeps it, checked on the emulator). The row is one button with its
        // words (B-and-24).
        compose.onNode(hasClickAction() and hasText(str(R.string.apply))).performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        text(R.string.lora_confirm_title).assertIsDisplayed()
        text("%.3f MHz".format(5000.0), substring = true).assertIsDisplayed()
        tapInDialog(R.string.apply)
        text(R.string.not_changed).assertIsDisplayed()
        text(R.string.err_invalid_setting).assertIsDisplayed()
        tapInDialog(R.string.ok)
        advance(3)
        assertEquals(before.freqKhz, model.radioSettings!!.freqKhz)
        text("SF${before.sf} · CR 4/${before.cr}", substring = true).assertIsDisplayed()
    }

    /** The preset row's current choice, which opens the list of presets. */
    private fun tapPresetChoice() {
        val r = model.radioSettings!!
        val labels = listOf(R.string.preset_custom, R.string.preset_eu_narrow, R.string.preset_us_canada).map { str(it) }
        val shown = labels.firstOrNull { has(it) } ?: error("no preset shown for ${r.freqKhz} kHz")
        tap(shown)
    }

    /** RL-6 (D-4): a setting the radio refuses on its own, answering an error (as a board
     * without GPS does for a GPS interval): said so, and the value shown stays the radio's. */
    @Test
    fun RL_6_radioRefusesASetting(): Unit = refusedByTheRadio()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_6_radioRefusesASetting_fr(): Unit = refusedByTheRadio()

    private fun refusedByTheRadio() = FakeTcpRadio().use { radio ->
        radio.refuses = { FakeTcpRadio.gpsInterval(it) }
        launch("onboarding" to false, "tcp" to radio.address, "screen" to "radio")
        waitReal("the radio's settings (${model.radioSettings?.gps})", 30_000) { model.radioSettings?.gps != null }
        see(R.string.radio_gps)
        // Its GPS on: taken.
        if (model.radioSettings?.gps != true) {
            toggle(R.string.radio_gps)
            waitReal("its GPS on") { model.radioSettings?.gps == true }
        }
        assertEquals(null, model.radioError)
        val before = model.radioSettings!!.gpsIntervalS
        fun label(s: UInt?) = when {
            s == null -> str(R.string.not_set)
            s >= 3_600u -> str(R.string.hours_n, (s / 3_600u).toInt())
            else -> str(R.string.minutes_n, (s / 60u).toInt())
        }
        val asked = if (before == 300u) 900u else 300u
        // A fix every 5 (or 15) minutes: refused by the radio.
        tap(label(before))
        tapInDialog(label(asked))
        waitReal("the radio's refusal") { has(R.string.radio_refused) }
        text(R.string.not_changed).assertIsDisplayed()
        tapInDialog(R.string.ok)
        // The radio's value, still.
        assertEquals(before, model.radioSettings?.gpsIntervalS)
        see(label(before))
        assertFalse(has(label(asked)))
    }

    /** RL-7: the radio's battery in its settings; Show radio details puts the radio card on
     * Messages. */
    @Test
    fun RL_7_batteryAndRadioCard() = batteryAndCard()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_7_batteryAndRadioCard_fr() = batteryAndCard()

    private fun batteryAndCard() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's battery") { model.radioSettings?.batteryMv != null }
        val r = model.radioSettings!!
        text(R.string.battery).assertIsDisplayed()
        text("${r.batteryPercent} %", substring = true).assertIsDisplayed()
        tapDesc(R.string.back)

        // Off: the radio in one line. On: its card, with the airtime left.
        tab(R.string.tab_messages)
        assertFalse(has(R.string.available))
        tab(R.string.tab_settings)
        toggle(R.string.radio_details)
        assertTrue(model.radioDetails)
        tab(R.string.tab_messages)
        text(R.string.available).assertIsDisplayed()
        text("SF${r.sf}", substring = true).assertIsDisplayed()
    }
}
