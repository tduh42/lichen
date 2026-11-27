package com.tduhamel.lichen.ui

import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.hasContentDescription
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
import com.tduhamel.lichen.core.RadioReading
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.FailReason
import com.tduhamel.lichen.core.LinkKind
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.RadioSetting
import com.tduhamel.lichen.core.meshcoreContactCode
import com.tduhamel.lichen.core.readMeshcoreContactCode
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.RadioFigure
import com.tduhamel.lichen.model.figureText
import com.tduhamel.lichen.model.figures
import com.tduhamel.lichen.model.shareOf
import java.text.NumberFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

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
        // The radio read again first (RL-12), then the usual line.
        advanceUntil("the radio read again", 30, { model.radioReading }) { model.radioReading == null }
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

    /** RL-12: the radio line while a connection reads (§1.1): with 200 more contacts on the
     * demo radio, "Reading the radio · contacts … of 206" with its bar, said as one line; the
     * Messages list says it is reading rather than "No conversations yet"; then the usual
     * connected line, no bar. */
    @Test
    fun RL_12_radioLineWhileReading() = readingLine()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_12_radioLineWhileReading_fr() = readingLine()

    private fun readingLine() {
        launch("demo" to true, "manyContacts" to 200)
        advanceUntil("the contacts counted", 10, { model.radioReading }) {
            val r = model.radioReading
            r is RadioReading.Contacts && r.done > 0u && r.done < r.total
        }
        assertEquals(206u, (model.radioReading as RadioReading.Contacts).total)
        // The count goes on as the screen settles: the line as it reads, whatever count it is at.
        lineSaying(R.string.reading_radio_contacts, 206).assertIsDisplayed()
        compose.onNodeWithTag(READING_BAR, useUnmergedTree = true).assertIsDisplayed()
        // Not "no conversations" while the radio's lists come in.
        assertFalse(has(R.string.no_conversations))
        if (model.conversations.isEmpty()) text(R.string.reading_radio).assertIsDisplayed()
        // Over: the usual line.
        advanceUntil("the reading over", 30, { model.radioReading }) { model.radioReading == null }
        text(str(R.string.radio_connected, model.nodeName)).assertIsDisplayed()
        assertTrue(compose.onAllNodesWithTag(READING_BAR, useUnmergedTree = true).fetchSemanticsNodes().isEmpty())
        assertTrue(model.contacts.size >= 200)
    }

    /** RL-12: switched off, then on again: the messages that waited on the radio are fetched,
     * "Fetching waiting messages · n" on the radio line meanwhile, and they arrive. */
    @Test
    fun RL_12_fetchingWaitingMessages() = fetchingWaiting()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_12_fetchingWaitingMessages_fr() = fetchingWaiting()

    /** The radio line said as [id] with any count in place of its first number (and [args]
     * after it): "Reading the radio, contacts 34 of 206". */
    private fun lineSaying(@androidx.annotation.StringRes id: Int, vararg args: Any): androidx.compose.ui.test.SemanticsNodeInteraction {
        val said = Regex(Regex.escape(said(str(id, 999_999, *args))).replace("999999", "\\E\\d+\\Q"))
        return compose.onNode(
            androidx.compose.ui.test.SemanticsMatcher("the radio line: $said") { n ->
                n.config.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.ContentDescription)?.any { said.matches(it) } == true
            },
            useUnmergedTree = true,
        )
    }

    private fun fetchingWaiting() {
        launch("demo" to true, "offline" to 40, "online" to 150)
        advanceUntil("the radio off", 60) { !model.isConnected }
        // Back on: the reading again, from the radio's settings to what waited on it.
        advanceUntil("back on", 130) { model.isConnected }
        val unread = model.conversations.sumOf { it.unread.toInt() }
        advanceUntil("the waiting messages fetched", 20, { model.radioReading }) { model.radioReading is RadioReading.Messages }
        lineSaying(R.string.fetching_waiting).assertIsDisplayed()
        advanceUntil("the reading over", 30, { model.radioReading }) { model.radioReading == null }
        text(str(R.string.radio_connected, model.nodeName)).assertIsDisplayed()
        // They arrived.
        assertTrue(model.conversations.sumOf { it.unread.toInt() } > unread)
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
        text("SF${r.sf} · CR\u00A04/${r.cr}", substring = true).assertIsDisplayed()
        see("${r.txDbm} dBm")
        // One dB less, applied by the radio and read back.
        tap("−")
        advanceUntil("the new power read back", 10) { model.radioSettings?.txDbm == (r.txDbm - 1).toByte() }
        see("${r.txDbm - 1} dBm")
        assertEquals(null, model.radioError)
    }

    /** RL-5: the radio on top of its settings: its name, its link and for how long, its
     * firmware and the hour's airtime left. */
    @Test
    fun RL_5_radioHeader() = header()

    @Test
    @Config(qualifiers = "+fr-night")
    fun RL_5_radioHeader_frDark() = header()

    private fun header() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings?.read == true }
        val r = model.radioSettings!!
        see(r.name)
        see(str(R.string.demo_network) + " · ", substring = true)
        // The firmware when the radio gives it (the demo's does not).
        if (r.firmware.isNotEmpty()) see(r.firmware) else assertFalse(has(R.string.firmware_stat))
        see(R.string.airtime_left_stat)
        val left = (model.budget.limitMs - minOf(model.budget.usedMs, model.budget.limitMs)) / 1000u
        see("$left s")
    }

    /** RL-5: Search radio settings: "power" ("puissance" in French) finds Transmit power, its
     * section under it; chosen, every section comes back and the list stands on its row, under
     * the search, which stays on top. */
    @Test
    fun RL_5_searchRadioSettings() = searchSettings("power")

    @Test
    @Config(qualifiers = "+fr")
    fun RL_5_searchRadioSettings_fr() = searchSettings("puissance")

    private fun searchSettings(word: String) {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings?.read == true }
        // Far down first.
        scrollTo(hasText(str(R.string.restart_radio)))
        val field = compose.onNode(hasSetTextAction() and hasContentDescription(str(R.string.search_radio_settings)))
        field.performTextInput(word)
        compose.waitForIdle()
        // Transmit power alone (its section, named the same, not repeated); the other sections gone.
        assertEquals(1, compose.onAllNodesWithText(str(R.string.transmit_power), useUnmergedTree = true).fetchSemanticsNodes().size)
        assertFalse(has(R.string.restart_radio))
        assertFalse(has(R.string.lora_now))
        tap(R.string.transmit_power)
        advance(1)
        // Every section back, the list on Transmit power: its row just under the search.
        assertTrue(has(R.string.lora_note) || has(R.string.restart_radio) || has(R.string.telemetry_base))
        val power = compose.onAllNodesWithText(str(R.string.transmit_power), useUnmergedTree = true).onLast()
        power.assertIsDisplayed()
        val top = power.fetchSemanticsNode().boundsInRoot.top
        val under = field.fetchSemanticsNode().boundsInRoot.bottom
        val dp = context.resources.displayMetrics.density
        assertTrue("Transmit power ${(top - under) / dp} dp under the search", top >= under && top - under < 120 * dp)
        see("${model.radioSettings!!.txDbm} dBm")
        // Found by a section's name too: Bluetooth's three rows.
        field.performTextInput("Bluetooth")
        compose.waitForIdle()
        for (row in listOf(R.string.ble_pin, R.string.save_pin, R.string.restart_radio)) assertTrue(str(row), has(row))
        // Nothing found, said so.
        field.performTextReplacement("zzz")
        compose.waitForIdle()
        see(R.string.no_setting_matches)
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

    /** RL-5 (B-user-2): a radio whose GPS is on but has no fix (indoors), its position put in
     * its adverts: none yet, and why; this phone's position offered, and taken. */
    @Test
    fun RL_5_gpsWithoutFixTakesThePhonesPosition() = gpsWithoutFix()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_5_gpsWithoutFixTakesThePhonesPosition_fr() = gpsWithoutFix()

    private fun gpsWithoutFix() {
        launch("demo" to true, "radioNoFix" to true, "screen" to "radio")
        // Its GPS, read after its other settings.
        advanceUntil("the radio's GPS") { model.radioSettings?.gps != null }
        assertEquals(true, model.radioSettings?.gps)
        toggle(R.string.position_in_adverts)
        tapInDialog(R.string.show_it)
        advanceUntil("in its adverts", 30) { model.radioSettings?.positionInAdverts == true }
        see(R.string.none_yet)
        // This phone's position offered (the screen asks for it), and why.
        advanceUntil("this phone's position offered", 30, { shown() }) { has(R.string.use_phone_position) }
        see(R.string.advert_gps_no_fix)
        val fix = model.myFix!!
        tap(R.string.use_phone_position)
        // SET_ADVERT_LATLON: the radio advertises it, in figures.
        advanceUntil("the phone's position on the radio", 30, { model.radioSettings }) { model.radioSettings?.latE6 != 0 }
        val r = model.radioSettings!!
        assertEquals(Math.round(fix.lat * 1e6).toInt(), r.latE6)
        assertEquals(Math.round(fix.lon * 1e6).toInt(), r.lonE6)
        assertFalse(has(R.string.none_yet))
        assertFalse(has(R.string.advert_gps_no_fix))
        see(String.format(java.util.Locale.ROOT, "%.5f, %.5f", r.latE6 / 1e6, r.lonE6 / 1e6))
        // A radio with its GPS on and a position keeps it: the phone's is no longer offered
        // (the GPS's next fix replaces it in the firmware).
        assertFalse(has(R.string.use_phone_position))
    }

    /** RL-10 (B-user-4, D-14): Settings › Send an advert: nearby or the whole mesh, each with
     * what it reaches, and what it carries (the radio's name; its position once advertised);
     * what the demo radio received; sent at what time, how far, from the radio's answer; with
     * the radio off, not sent. */
    @Test
    fun RL_10_sendAnAdvert() = advert()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_10_sendAnAdvert_fr() = advert()

    private fun sentAt(flood: Boolean): String {
        val sent = model.advert as AppModel.AdvertState.Sent
        assertEquals(flood, sent.flood)
        return str(if (flood) R.string.advert_sent_mesh else R.string.advert_sent_nearby, timeOf(context, sent.atMs))
    }

    private fun advert() {
        launch("demo" to true)
        advanceUntil("the radio's settings") { model.radioSettings?.read == true }
        val demo = model.demoNetwork!!
        val name = model.radioSettings!!.name
        tab(R.string.tab_settings)
        see(R.string.send_advert_note)
        // The choice: how far each goes, what it carries; cancelled, nothing goes.
        tap(R.string.send_advert)
        for (s in listOf(R.string.advert_nearby, R.string.advert_nearby_note, R.string.advert_mesh, R.string.advert_mesh_note)) see(s)
        see(str(R.string.advert_carries, name))
        tapInDialog(R.string.cancel)
        assertEquals(null, demo.lastAdvert())
        assertEquals(null, model.advert)
        // Nearby: the radio gets a zero-hop advert; "Sending…" until it answers (its answer held
        // back a moment here), then the time.
        demo.holdReplies(100_000uL)
        tap(R.string.send_advert)
        tapInDialog(R.string.advert_nearby)
        see(R.string.advert_sending)
        demo.holdReplies(0uL)
        advanceUntil("the radio's answer", 10) { model.advert is AppModel.AdvertState.Sent }
        assertEquals(false, demo.lastAdvert())
        see(sentAt(false))
        // The whole mesh: flooded.
        tap(R.string.send_advert)
        tapInDialog(R.string.advert_mesh)
        advanceUntil("the radio's answer", 10) { (model.advert as? AppModel.AdvertState.Sent)?.flood == true }
        assertEquals(true, demo.lastAdvert())
        see(sentAt(true))
        // Position in adverts on, and a position: the advert says it carries it.
        val r = model.radioSettings!!
        model.radioSet(RadioSetting.PREFERENCES, "position", { it.copy(positionInAdverts = true) }) {
            model.core.setRadioPreferences(r.manualAdd, r.telemetry, true, r.multiAcks)
        }
        model.radioSet(RadioSetting.ADVERT_POSITION, "advert_position", { it.copy(latE6 = 45_123_450, lonE6 = 5_123_450) }) {
            model.core.setAdvertPosition(45_123_450, 5_123_450)
        }
        advanceUntil("the position in adverts", 10) { model.radioSettings?.let { it.positionInAdverts && it.latE6 != 0 } == true }
        tap(R.string.send_advert)
        see(str(R.string.advert_carries_position, name))
        tapInDialog(R.string.cancel)
        // The radio off: not sent, said.
        model.setDemoRadio(false)
        advance(1)
        tap(R.string.send_advert)
        tapInDialog(R.string.advert_mesh)
        advanceUntil("not sent", 5) { model.advert is AppModel.AdvertState.NotSent }
        see(R.string.advert_not_sent)
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

    /** RL-5 (F-1): the radio's MeshCore public key, in groups of 8, copied and shared; its
     * MeshCore code, a QR a stock MeshCore app scans to add this radio. */
    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun RL_5_meshcorePublicKeyAndCode() = meshcoreKey()

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "+fr")
    fun RL_5_meshcorePublicKeyAndCode_fr() = meshcoreKey()

    private fun meshcoreKey() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings and key") { model.radioSettings?.read == true && model.nodeKey != null }
        val key = model.nodeKey!!
        val hex = key.joinToString("") { "%02x".format(it) }
        // The key, the radio's own, in groups of 8.
        see(R.string.meshcore_public_key)
        see(hex.chunked(8).joinToString(" "))
        // Its code: the QR reads back as the stock app's contact code for this radio.
        see(R.string.meshcore_code_note)
        val code = readQr(str(R.string.meshcore_code))
        assertEquals(meshcoreContactCode(model.radioSettings!!.name, key, NodeKind.COMPANION), code)
        val read = readMeshcoreContactCode(code)
        assertTrue(read.key.contentEquals(key))
        assertEquals(NodeKind.COMPANION, read.kind)
        // Copy: the 64 hex digits on the clipboard, said so.
        tapDesc(R.string.copy)
        assertEquals(hex, clipboardText())
        see(R.string.copied)
        // Share: the same, to another app.
        val app = shadowOf(context as android.app.Application)
        while (app.nextStartedActivity != null) Unit
        tapDesc(R.string.share)
        val chooser = app.nextStartedActivity
        assertEquals(android.content.Intent.ACTION_CHOOSER, chooser?.action)
        val sent = androidx.core.content.IntentCompat.getParcelableExtra(chooser!!, android.content.Intent.EXTRA_INTENT, android.content.Intent::class.java)
        assertEquals(hex, sent?.getStringExtra(android.content.Intent.EXTRA_TEXT))
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
        text("%.3f\u00A0MHz".format(5000.0), substring = true).assertIsDisplayed()
        tapInDialog(R.string.apply)
        text(R.string.not_changed).assertIsDisplayed()
        text(R.string.err_invalid_setting).assertIsDisplayed()
        tapInDialog(R.string.ok)
        advance(3)
        assertEquals(before.freqKhz, model.radioSettings!!.freqKhz)
        text("SF${before.sf} · CR\u00A04/${before.cr}", substring = true).assertIsDisplayed()
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
        // Said on its row (B-user-5), not in a dialog.
        waitReal("the radio's refusal") { has(R.string.radio_refused) }
        assertFalse(has(R.string.not_changed))
        // The radio's value, still.
        assertEquals(before, model.radioSettings?.gpsIntervalS)
        see(label(before))
        assertFalse(has(label(asked)))
    }

    /** RL-11 (B-user-5): each setting says what happened, on its row. A telemetry change: a
     * spinner while the radio has not answered, then "Saved on the radio", gone after 2 s. With
     * `radioIgnores preferences`, the position switch and a telemetry choice: the radio kept its
     * old value, which the controls show again. With `radioRefuses power`: refused. */
    @Test
    fun RL_11_eachSettingSaysWhatHappened() = outcomes()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_11_eachSettingSaysWhatHappened_fr() = outcomes()

    /** The telemetry row's current choice (the first of the three, "who may ask for the basics"),
     * opened, and [to] chosen. */
    private fun telemetryBase(to: Int) {
        scrollTo(hasText(str(R.string.telemetry_base)))
        compose.onAllNodesWithText(str(R.string.nobody)).onFirst().performClick()
        compose.waitForIdle()
        tapInDialog(to)
    }

    private fun outcomes() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's settings") { model.radioSettings?.read == true }
        assertEquals(0, model.radioSettings!!.telemetry.base.toInt())
        // The radio's answer held back a moment: waiting, the value asked for and the spinner.
        val demo = model.demoNetwork!!
        demo.holdReplies(100_000uL)
        telemetryBase(R.string.everyone)
        advance(1)
        assertTrue(hasDesc(str(R.string.radio_waiting)))
        assertEquals(2, model.radioAsked?.telemetry?.base?.toInt())
        demo.holdReplies(0uL)
        advanceUntil("saved", 10) { has(R.string.radio_saved) }
        assertEquals(2, model.radioSettings!!.telemetry.base.toInt())
        assertEquals(null, model.radioAsked)
        // Gone after its 2 s (a delay of the screen's own: Compose's clock).
        compose.mainClock.advanceTimeBy(2_500)
        compose.waitForIdle()
        assertFalse(has(R.string.radio_saved))
    }

    /** RL-11 (B-user-6): on a radio that says nothing of its own, as on a quiet mesh, a setting
     * goes out at once and is said saved in seconds. It used to wait in the core's queue until
     * something else woke the app: the row's spinner stayed for minutes. */
    @Test
    fun RL_11_aSettingGoesOutOnAQuietRadio() = quietRadio()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_11_aSettingGoesOutOnAQuietRadio_fr() = quietRadio()

    private fun quietRadio(): Unit = FakeTcpRadio(quiet = true).use {
        launch("onboarding" to false, "tcp" to it.address, "screen" to "radio")
        waitReal("the radio's settings", 30_000) { model.radioSettings?.read == true }
        // The connection's reads over, nothing more comes from the radio.
        val quiet = System.nanoTime() + 2_000_000_000L
        waitReal("the connection's reads over") { System.nanoTime() > quiet }
        telemetryBase(R.string.everyone)
        waitReal("saved", 3_000) { has(R.string.radio_saved) }
        assertEquals(2, model.radioSettings!!.telemetry.base.toInt())
        assertEquals(null, model.radioAsked)
    }

    @Test
    fun RL_11_aSettingTheRadioDoesNotKeep() = notKept()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_11_aSettingTheRadioDoesNotKeep_fr() = notKept()

    private fun notKept() {
        launch("demo" to true, "screen" to "radio", "radioIgnores" to "preferences", "radioRefuses" to "power")
        advanceUntil("the radio's settings") { model.radioSettings?.read == true }
        assertFalse(model.radioSettings!!.positionInAdverts)
        // The position in adverts: asked for, answered, not kept.
        toggle(R.string.position_in_adverts)
        tapInDialog(R.string.show_it)
        advanceUntil("not kept", 10) { has(R.string.radio_not_kept) }
        assertFalse(model.radioSettings!!.positionInAdverts)
        assertEquals(null, model.radioAsked)
        see(R.string.position_in_adverts_off)
        // A telemetry choice too; the message stays until the next change.
        telemetryBase(R.string.everyone)
        advanceUntil("not kept, twice", 10) {
            compose.onAllNodesWithText(str(R.string.radio_not_kept)).fetchSemanticsNodes().size == 2
        }
        advance(3)
        assertTrue(has(R.string.radio_not_kept))
        assertEquals(0, model.radioSettings!!.telemetry.base.toInt())
        // The power: refused, on its row.
        if (!has(str(R.string.transmit_power))) scrollTo(hasText(str(R.string.transmit_power)))
        compose.onAllNodesWithText("−").onFirst().performClick()
        compose.waitForIdle()
        advanceUntil("refused", 10) { has(R.string.radio_refused) }
        assertFalse(has(R.string.not_changed))
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
        // The ring on top, said with its voltage.
        val volts = "%.2f".format(r.batteryMv!!.toDouble() / 1000)
        desc(str(R.string.battery_ring, r.batteryPercent!!.toInt(), volts)).assertIsDisplayed()
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

    /** RL-7: the radio's own figures (§16.4) in Radio settings: its noise floor in the header,
     * and "Since it started" at the bottom with every figure the demo radio reports, each row one
     * element to TalkBack ("Packets received, 1,204"), and the note under the card. Read at
     * connection, before the radio heard a packet, its last packet is unknown (its RSSI and SNR
     * both read 0): no row for it, never "0 dB"; a minute later, read again, it has one. */
    @Test
    fun RL_7_radioFigures() = radioFigures()

    @Test
    @Config(qualifiers = "+fr")
    fun RL_7_radioFigures_fr() = radioFigures()

    @Test
    @Config(qualifiers = "+night")
    fun RL_7_radioFigures_dark() = radioFigures()

    private fun radioFigures() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's figures") { model.radioSettings?.run { read && noiseFloorDbm != null && packetsReceived != null } == true }
        val r = model.radioSettings!!
        // The demo radio's noise floor, a third figure in the header.
        assertEquals(-118, r.noiseFloorDbm!!.toInt())
        text("−118 dBm").assertIsDisplayed()
        see(R.string.noise_floor_stat)
        // The card, last: the figures it reported, in their order, each said as one.
        scrollTo(hasText(str(R.string.figures_note)))
        see(R.string.since_started)
        text(R.string.figures_note).assertIsDisplayed()
        val first = model.radioSettings!!
        // Read at connection: no packet heard yet, so every figure but the last packet's.
        assertEquals(0u, first.packetsReceived)
        assertEquals(null to null, first.lastRssiDbm to first.lastSnrDb)
        assertEquals(RadioFigure.entries - RadioFigure.LAST_PACKET, first.figures())
        assertFalse(has(R.string.fig_last_packet))
        val zeroDb = Regex("""(^|[^\d.,])0 dB""")
        compose.onAllNodes(
            SemanticsMatcher("says 0 dB") { n ->
                (n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
                    n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()).any { zeroDb.containsMatchIn(it) }
            },
            useUnmergedTree = true,
        ).assertCountEquals(0)
        // A minute later, on the demo's clock and the screen's, read again: the radio has heard
        // the mesh, and its last packet has a row, RSSI and SNR.
        advance(61)
        compose.mainClock.advanceTimeBy(60_000)
        // Its packets come last of the three answers: all of them read again.
        advanceUntil("the last packet's figures", 10) {
            model.radioSettings?.run { lastRssiDbm != null && lastSnrDb != null && (packetsReceived ?: 0u) > 0u } == true
        }
        val now = model.radioSettings!!
        val figures = now.figures()
        assertEquals(RadioFigure.entries.toList(), figures)
        for (f in figures) {
            val said = "${str(FIGURE_TITLES.getValue(f))}, ${now.figureText(context.resources, f)}"
            scrollTo(hasContentDescription(said))
            desc(said).assertIsDisplayed()
        }
        // In this language's figures: "1,204" ("1 204" in French); airtime and its share of the time on.
        val received = NumberFormat.getIntegerInstance().format(now.packetsReceived!!.toLong())
        assertTrue(hasDesc("${str(R.string.fig_received)}, $received"))
        val share = shareOf(now.txAirtimeS!!.toLong(), now.uptimeS!!.toLong())!!
        assertTrue(now.figureText(context.resources, RadioFigure.AIRTIME_SENT).endsWith(str(R.string.fig_airtime_share, "", share)))
    }

    /** RL-7: the figures are read again every minute while Radio settings is open, and no more
     * once it closes (nothing else asks for them). */
    @Test
    fun RL_7_figuresReadAgainEveryMinute() {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's figures") { model.radioSettings?.run { read && uptimeS != null && packetsReceived != null } == true }
        advance(2)
        val first = model.radioSettings!!.uptimeS!!
        // A minute later, on the demo's clock and the screen's: read again.
        advance(61)
        compose.mainClock.advanceTimeBy(60_000)
        advanceUntil("the figures read again", 10) { (model.radioSettings?.uptimeS ?: 0u) >= first + 60u }
        val uptime = model.radioSettings!!.figureText(context.resources, RadioFigure.UPTIME)
        scrollTo(hasContentDescription("${str(R.string.fig_uptime)}, $uptime"))
        desc("${str(R.string.fig_uptime)}, $uptime").assertIsDisplayed()
        // Closed: not again.
        tapDesc(R.string.back)
        advance(2)
        val second = model.radioSettings!!.uptimeS
        advance(61)
        compose.mainClock.advanceTimeBy(60_000)
        advance(5)
        assertEquals(second, model.radioSettings!!.uptimeS)
    }

    /** RL-7: a radio that reports none of these figures shows neither the header's noise floor
     * nor the card (never a 0 standing for a figure it did not give). */
    @Test
    fun RL_7_noFiguresNoCard() {
        launch("demo" to true, "screen" to "radio")
        // Every answer in (the figures come in three), none still on its way.
        advanceUntil("the radio's figures") { model.radioSettings?.run { read && noiseFloorDbm != null && uptimeS != null && packetsReceived != null } == true }
        advance(2)
        // The radio as one that reports none of them.
        fun noFigures() {
            model.radioSettings = model.radioSettings!!.copy(
                noiseFloorDbm = null, lastRssiDbm = null, lastSnrDb = null, txAirtimeS = null, rxAirtimeS = null,
                uptimeS = null, packetsReceived = null, packetsSent = null, receiveErrors = null,
            )
            compose.waitForIdle()
            assertTrue(model.radioSettings!!.figures().isEmpty())
        }
        // At the bottom: the card goes.
        scrollTo(hasText(str(R.string.meshcore_code_note)))
        see(R.string.since_started)
        noFigures()
        assertFalse(has(R.string.since_started))
        assertFalse(has(R.string.figures_note))
        // On top: the header's noise floor goes.
        scrollTo(hasText(str(R.string.airtime_left_stat)))
        noFigures()
        assertFalse(has(R.string.noise_floor_stat))
        assertFalse(has(R.string.since_started))
        // Searched, nothing of them either.
        val field = compose.onNode(hasSetTextAction() and hasContentDescription(str(R.string.search_radio_settings)))
        field.performTextInput(str(R.string.noise_floor).take(5))
        compose.waitForIdle()
        see(R.string.no_setting_matches)
    }

    /** RL-7: Search radio settings finds the figures: "noise" ("bruit") lands on the header's
     * noise floor, "errors" ("erreurs") on the card's row. */
    @Test
    fun RL_7_searchFigures() = searchFigures("noise", "errors")

    @Test
    @Config(qualifiers = "+fr")
    fun RL_7_searchFigures_fr() = searchFigures("bruit", "erreurs")

    private fun searchFigures(noise: String, errors: String) {
        launch("demo" to true, "screen" to "radio")
        advanceUntil("the radio's figures") { model.radioSettings?.run { read && noiseFloorDbm != null && receiveErrors != null } == true }
        // Far down first.
        scrollTo(hasText(str(R.string.restart_radio)))
        assertFalse(has("−118 dBm"))
        val field = compose.onNode(hasSetTextAction() and hasContentDescription(str(R.string.search_radio_settings)))
        field.performTextInput(noise)
        compose.waitForIdle()
        // The noise floor alone, in This radio.
        see(R.string.noise_floor)
        see(R.string.this_radio)
        assertFalse(has(R.string.restart_radio))
        tap(R.string.noise_floor)
        advance(1)
        text("−118 dBm").assertIsDisplayed()
        // The errors' row, at the bottom.
        field.performTextInput(errors)
        compose.waitForIdle()
        see(R.string.fig_errors)
        tap(R.string.fig_errors)
        advance(1)
        val now = model.radioSettings!!
        desc("${str(R.string.fig_errors)}, ${now.figureText(context.resources, RadioFigure.ERRORS)}").assertIsDisplayed()
    }

    private companion object {
        val FIGURE_TITLES = mapOf(
            RadioFigure.UPTIME to R.string.fig_uptime,
            RadioFigure.RECEIVED to R.string.fig_received,
            RadioFigure.SENT to R.string.fig_sent,
            RadioFigure.ERRORS to R.string.fig_errors,
            RadioFigure.AIRTIME_SENT to R.string.fig_airtime_sent,
            RadioFigure.AIRTIME_RECEIVED to R.string.fig_airtime_received,
            RadioFigure.LAST_PACKET to R.string.fig_last_packet,
        )
    }
}
