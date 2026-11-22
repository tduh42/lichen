package com.tduhamel.lichen.ui

import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.hasScrollToIndexAction
import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.google.zxing.BinaryBitmap
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.LichenCore
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.IdentityRead
import com.tduhamel.lichen.model.IdentityStore
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.Base64

/** Area 1 of docs/testing.md: onboarding and identity. */
@RunWith(AndroidJUnit4::class)
class OnboardingTest : UiTest() {
    /** ON-1: first launch to the end: an identity made, its phrase shown, Messages empty with
     * the radio prompt. */
    @Test
    fun ON_1_onboardingToTheEnd() = onboarding()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_1_onboardingToTheEnd_fr() = onboarding()

    @Test
    @Config(qualifiers = "+fr-night")
    fun ON_1_onboardingToTheEnd_frDark() = onboarding()

    private fun onboarding() {
        launch("onboarding" to true)
        text(R.string.tagline).assertIsDisplayed()
        text(R.string.connect_my_radio).assertIsDisplayed()
        text(R.string.restore_backup).assertIsDisplayed()
        tap(R.string.later)

        // The identity, made at the first start and saved.
        text(R.string.your_identity).assertIsDisplayed()
        assertTrue(IdentityStore(context).read() is IdentityRead.Found)
        val fingerprint = model.fingerprint
        assertEquals(6, fingerprint.size)
        for (g in fingerprint) text(g).assertIsDisplayed()
        text(R.string.not_backed_up).assertIsDisplayed()

        // The 24 words, then written down: the warning goes.
        tap(R.string.recovery_phrase)
        val words = model.recoveryPhrase()
        assertEquals(24, words.size)
        for (w in words.toSet()) assertTrue("word $w shown", has(w))
        tap(R.string.wrote_them_down)
        // Three of them checked (D-7).
        answer(words, asked().map { it - 1 })
        tap(R.string.check)
        assertFalse(has(R.string.not_backed_up))
        assertTrue(model.phraseBackedUp)

        // A first contact: no radio yet, so no code; later.
        tap(R.string.continue_)
        text(R.string.add_contact).assertIsDisplayed()
        text(R.string.connect_radio_first).assertIsDisplayed()
        tap(R.string.later)

        // Messages, empty, with the radio prompt: no radio yet, not one that dropped (D-1).
        assertTrue(model.onboarded)
        text(R.string.no_conversations).assertIsDisplayed()
        // (In French the title is the radio line's own words, so it shows twice.)
        compose.onAllNodesWithText(str(R.string.no_radio_yet)).onFirst().assertIsDisplayed()
        text(R.string.connect_radio_to_use).assertIsDisplayed()
        assertFalse(has(R.string.radio_disconnected))
        text(R.string.connect).assertIsDisplayed()
        assertEquals(fingerprint, model.fingerprint)
    }

    /** ON-2: after a restart, the same identity, and no welcome screens. */
    @Test
    fun ON_2_restartKeepsTheIdentity() {
        launch("onboarding" to true)
        tap(R.string.later)
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("Morgan")
        tap(R.string.continue_)
        tap(R.string.later)
        val fingerprint = model.fingerprint
        val phrase = model.recoveryPhrase()

        restart()
        assertFalse(has(R.string.connect_my_radio))
        text(R.string.no_conversations).assertIsDisplayed()
        assertEquals(fingerprint, model.fingerprint)
        assertEquals(phrase, model.recoveryPhrase())

        // Settings › Your identity: the same fingerprint, the same name.
        tab(R.string.tab_settings)
        tap(R.string.your_identity)
        for (g in fingerprint) text(g).assertIsDisplayed()
        text("Morgan").assertIsDisplayed()
    }

    /** ON-3: the phrase restores the identity it belongs to; wrong words are refused and change
     * nothing. */
    @Test
    fun ON_3_recoveryPhraseRestoresItsIdentity() = recoveryPhrase()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_3_recoveryPhraseRestoresItsIdentity_fr() = recoveryPhrase()

    private fun recoveryPhrase() {
        launch("onboarding" to false, "screen" to "identity")
        val mine = model.fingerprint
        tap(R.string.restore_identity)
        // The sheet's field (the name field is behind it).
        val field = compose.onAllNodes(hasSetTextAction()).onLast()

        // 23 words: not yet.
        field.performTextInput(List(23) { "abandon" }.joinToString(" "))
        text(R.string.words_count, 23).assertIsDisplayed()
        compose.onAllNodesWithText(str(R.string.restore)).onFirst().assertIsNotEnabled()

        // 24 words that are no phrase: refused, said so, nothing changed.
        field.performTextReplacement(List(24) { "abandon" }.joinToString(" "))
        compose.onAllNodesWithText(str(R.string.restore)).onFirst().assertIsEnabled()
        tap(R.string.restore)
        text(R.string.err_not_a_phrase).assertIsDisplayed()
        assertEquals(mine, model.fingerprint)

        // Another identity's 24 words: that identity, here.
        val other = LichenCore("test", ByteArray(32) { 3 }, ByteArray(32) { 42 }, null)
        val theirs = other.fingerprint()
        field.performTextReplacement(other.recoveryPhrase().joinToString(" "))
        tap(R.string.restore)
        other.close()
        assertEquals(theirs, model.fingerprint)
        assertNotEquals(mine, theirs)
        for (g in theirs) text(g).assertIsDisplayed()
        assertFalse(has(R.string.not_backed_up))
    }

    /** The positions the phrase check asks for ("Word 5"), from the screen. */
    private fun asked(): List<Int> {
        val (before, after) = str(R.string.word_n, 999).split("999")
        val label = Regex(Regex.escape(before) + "(\\d+)" + Regex.escape(after))
        return shown().mapNotNull { label.matchEntire(it)?.groupValues?.get(1)?.toInt() }.distinct()
    }

    /** The check's three fields (the sheet's; the name field is behind it). */
    private fun answer(words: List<String>, asked: List<Int>, wrongAt: Int? = null) {
        val fields = compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size
        asked.forEachIndexed { k, position ->
            val word = if (k == wrongAt) words[position] + "x" else words[position].uppercase()
            compose.onAllNodes(hasSetTextAction())[fields - 3 + k].performTextReplacement(" $word ")
        }
    }

    /** ON-3 (D-7): "I wrote them down" asks for three of the 24 words, by position: a wrong one
     * says so and the phrase can be seen again; only three right words back the identity up. */
    @Test
    fun ON_3_writtenDownPhraseIsChecked() = phraseCheck()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_3_writtenDownPhraseIsChecked_fr() = phraseCheck()

    private fun phraseCheck() {
        launch("onboarding" to false, "screen" to "identity")
        val words = model.recoveryPhrase()
        tap(R.string.recovery_phrase)
        tap(R.string.wrote_them_down)
        text(R.string.check_your_phrase).assertIsDisplayed()
        val first = asked()
        assertEquals(first.toString(), 3, first.size)
        assertTrue(first.all { it in 1..24 })
        // One word wrong: said so, nothing backed up; the phrase again.
        answer(words, first.map { it - 1 }, wrongAt = 1)
        tap(R.string.check)
        see(R.string.phrase_word_wrong)
        assertFalse(model.phraseBackedUp)
        tap(R.string.show_phrase_again)
        for (w in words.toSet()) assertTrue("word $w shown", has(w))
        // Asked again: three right words (case and spaces aside), backed up.
        tap(R.string.wrote_them_down)
        val again = asked()
        assertEquals(3, again.size)
        answer(words, again.map { it - 1 })
        tap(R.string.check)
        assertTrue(model.phraseBackedUp)
        assertFalse(has(R.string.not_backed_up))
    }

    /** ON-3 (D-7): in the demo, its throwaway identity's words, checked, back up nothing of the
     * phone's own identity (as B-ios-2 on iOS). */
    @Test
    fun ON_3_theDemoDoesNotBackUpTheRealIdentity() {
        launch("demo" to true, "screen" to "identity")
        advanceUntil("the demo radio") { model.isConnected }
        val words = model.recoveryPhrase()
        tap(R.string.recovery_phrase)
        tap(R.string.wrote_them_down)
        answer(words, asked().map { it - 1 })
        tap(R.string.check)
        // The phone's own identity, after the demo: still not backed up.
        restart("onboarding" to false, "screen" to "identity")
        assertFalse(model.phraseBackedUp)
        see(R.string.not_backed_up)
    }

    /** ON-4: my code: a QR code that reads back as my card, which names me. */
    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun ON_4_myCardAndItsQrCode() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        model.changeName("Morgan")
        compose.waitForIdle()
        text(R.string.my_code).assertIsDisplayed()
        val card = model.myCard()!!
        val image = desc(R.string.your_contact_code).captureToImage().asAndroidBitmap()
        assertEquals(card, decodeQr(image))
        // Read by another phone: a contact named after me.
        val other = LichenCore("test", ByteArray(32) { 3 }, ByteArray(32) { 42 }, null)
        val me = other.addContactCard(card, true)
        other.close()
        assertEquals("Morgan", me.name)
        assertTrue(me.verified)
    }

    /** ON-5: each identity or database problem has its screen and its ways out. */
    @Test
    fun ON_5_identityProblemScreens() = problems()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_5_identityProblemScreens_fr() = problems()

    private fun problems() {
        data class Case(val extra: String, val title: Int, val body: Int, val words: Boolean, val backup: Boolean, val empty: Boolean, val new: Boolean)
        val cases = listOf(
            Case("unavailable", R.string.identity_problem_title, R.string.identity_problem_unavailable, true, true, false, false),
            Case("corrupt", R.string.identity_problem_title, R.string.identity_problem_corrupt, true, true, false, true),
            Case("notsaved", R.string.identity_problem_title, R.string.identity_problem_not_saved, true, true, false, false),
            Case("storage", R.string.storage_problem_title, R.string.storage_problem, false, false, false, false),
            Case("databasekey", R.string.database_key_unavailable_title, R.string.database_key_unavailable, false, false, false, false),
            Case("databasekeylost", R.string.database_key_lost_title, R.string.database_key_lost, false, true, true, false),
        )
        for ((i, c) in cases.withIndex()) {
            if (i == 0) launch("onboarding" to false, "identityProblem" to c.extra) else restart("onboarding" to false, "identityProblem" to c.extra)
            text(c.title).assertIsDisplayed()
            text(c.body).assertIsDisplayed()
            text(R.string.try_again).assertIsDisplayed()
            assertEquals(c.extra, c.words, has(R.string.restore_from_words))
            assertEquals(c.extra, c.backup, has(R.string.restore_backup))
            assertEquals(c.extra, c.empty, has(R.string.empty_history))
            assertEquals(c.extra, c.new, has(R.string.new_identity))
            // The way out: the stored identity reads again, and the app starts with it.
            tap(R.string.try_again)
            assertEquals(c.extra, null, model.identityProblem)
            text(R.string.no_conversations).assertIsDisplayed()
        }
    }

    /** ON-5: the identity unreadable for good: a new one, deliberately, after a warning. */
    @Test
    fun ON_5_corruptIdentityStartsANewOne() {
        launch("onboarding" to false)
        val old = model.fingerprint
        restart("onboarding" to false, "identityProblem" to "corrupt")
        tap(R.string.new_identity)
        text(R.string.new_identity_confirm).assertIsDisplayed()
        compose.onAllNodesWithText(str(R.string.new_identity)).onFirst().assertIsDisplayed()
        // The dialog's button (the screen's own is behind it).
        compose.onAllNodesWithText(str(R.string.new_identity))[1].performClick()
        compose.waitForIdle()
        assertEquals(null, model.identityProblem)
        assertNotEquals(old, model.fingerprint)
        text(R.string.no_conversations).assertIsDisplayed()
    }

    /** ON-6: Camille's safety number, compared and approved: her key is verified. */
    @Test
    fun ON_6_verifyAContact() = verify()

    @Test
    @Config(qualifiers = "+night")
    fun ON_6_verifyAContact_dark() = verify()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_6_verifyAContact_fr() {
        // "tu", never "vous" (B-and-13).
        assertEquals("Compare ces chiffres avec Camille", str(R.string.compare_numbers_with, "Camille"))
        verify()
    }

    private fun verify() {
        launch("demo" to true, "open" to "Camille", "screen" to "verify")
        advanceUntil("Camille's identity") { model.contacts.any { it.name == "Camille" && it.identity != null } }
        advanceUntil("the verification sheet") { has(R.string.numbers_match) }
        val camille = contact("Camille")
        assertFalse(camille.verified)
        text(R.string.safety_number).assertIsDisplayed()
        text(R.string.compare_numbers_with, "Camille").assertIsDisplayed()
        val numbers = model.safetyNumberWith(camille.identity!!)!!
        assertEquals(numbers, model.core.safetyNumber(camille.key))
        for (g in numbers) text(g).assertIsDisplayed()
        tap(R.string.numbers_match)
        assertTrue(contact("Camille").verified)
        desc(R.string.key_verified).assertIsDisplayed()
        text(R.string.session_verified).assertIsDisplayed()
    }

    /** ON-7: someone's card pasted: added, unverified until the numbers are compared; their chat
     * opens and takes a message. A text that is no code, or our own code, adds nobody. */
    @Test
    fun ON_7_addContactByCard() = addByCard()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_7_addContactByCard_fr() = addByCard()

    private fun addByCard() {
        launch("demo" to true, "screen" to "contact")
        advanceUntil("the radio's key") { model.nodeKey != null }
        val before = model.contacts.size
        tap(R.string.scan)
        val field = compose.onAllNodes(hasSetTextAction()).onLast()
        // No code, then our own: refused, nobody added.
        field.performTextInput("lichen:1:nothing")
        tap(R.string.add)
        text(R.string.err_bad_code).assertIsDisplayed()
        field.performTextReplacement(model.myCard()!!)
        tap(R.string.add)
        text(R.string.own_code).assertIsDisplayed()
        assertEquals(before, model.contacts.size)
        // An invitation to a group of ours: our own code too.
        model.createGroup("Hike", emptyList())
        val invite = model.createInvite(model.groups.first { it.name == "Hike" }.gid).first!!.text
        field.performTextReplacement(invite)
        tap(R.string.add)
        text(R.string.own_code).assertIsDisplayed()
        assertFalse(has(R.string.join))

        // Noé's card: an identity of their own, a radio ours does not know yet.
        val (card, identity, node) = noeCard()
        field.performTextReplacement(card)
        tap(R.string.add)
        text(R.string.compare_numbers_with, "Noé").assertIsDisplayed()
        val added = model.contacts.first { it.key.contentEquals(node) }
        assertArrayEquals(identity, added.identity)
        // Pasted, it proves nothing: verified only once the numbers are compared.
        assertFalse(added.verified)
        tap(R.string.numbers_match)
        assertTrue(model.contacts.first { it.key.contentEquals(node) }.verified)
        // Their chat, open, takes a message.
        text(R.string.message_placeholder).assertIsDisplayed()
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("Hello Noé")
        tapDesc(R.string.send)
        assertTrue(model.chats[ChatId.contact(node)].orEmpty().any { it.outgoing })
    }

    /** B-user-1: the phone that did not scan is asked to compare too. Camille's key, learned
     * from her first message and not verified: her chat says to compare; Compare shows the same
     * numbers the scanning phone shows, and once they match the banner goes for good. */
    @Test
    fun ON_6_otherPhoneIsAskedToCompare() = askedToCompare()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_6_otherPhoneIsAskedToCompare_fr() = askedToCompare()

    private fun askedToCompare() {
        launch("demo" to true)
        camilleWrote()
        assertFalse(contact("Camille").verified)
        tap("Camille")
        text(R.string.compare_safety_numbers_with, "Camille").assertIsDisplayed()
        tap(R.string.compare)
        text(R.string.compare_numbers_with, "Camille").assertIsDisplayed()
        tap(R.string.numbers_match)
        assertTrue(contact("Camille").verified)
        assertFalse(has(str(R.string.compare_safety_numbers_with, "Camille")))
    }

    /** D-10: at large text (font scale 1.5 and more), the compare banner is compact: its title
     * and Compare / Later, without the note; at the usual size, the note too. */
    @Test
    @Config(fontScale = 2.0f)
    fun ON_6_compactCompareBannerAtLargeText() = compareBanner(compact = true)

    @Test
    @Config(qualifiers = "+fr", fontScale = 1.5f)
    fun ON_6_compactCompareBannerAtLargeText_fr() = compareBanner(compact = true)

    @Test
    fun ON_6_compareBannerWithItsNote() = compareBanner(compact = false)

    private fun compareBanner(compact: Boolean) {
        launch("demo" to true)
        camilleWrote()
        tap("Camille")
        text(R.string.compare_safety_numbers_with, "Camille").assertIsDisplayed()
        text(R.string.compare).assertIsDisplayed()
        text(R.string.later).assertIsDisplayed()
        assertEquals("the note", !compact, has(R.string.compare_safety_numbers_note))
    }

    /** B-ios-29's rule, on both apps: while the keyboard is up, the compare banner is compact
     * (D-10's: its title and Compare / Later), in every language; down, its note comes back. */
    @Test
    fun ON_6_compactCompareBannerWithTheKeyboardUp() = bannerWithTheKeyboard()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_6_compactCompareBannerWithTheKeyboardUp_fr() = bannerWithTheKeyboard()

    private fun bannerWithTheKeyboard() {
        launch("demo" to true)
        camilleWrote()
        tap("Camille")
        text(R.string.compare_safety_numbers_note).assertIsDisplayed()
        keyboard(300)
        assertFalse("the note with the keyboard up", has(R.string.compare_safety_numbers_note))
        text(R.string.compare_safety_numbers_with, "Camille").assertIsDisplayed()
        text(R.string.compare).assertIsDisplayed()
        text(R.string.later).assertIsDisplayed()
        keyboard(0)
        text(R.string.compare_safety_numbers_note).assertIsDisplayed()
    }

    /** B-ios-29 on a smaller phone (640 dp tall), in French, at font scale 1.3, the keyboard up
     * (250 dp): the compare banner leaves the composer on screen while we reply, and the
     * messages room enough that a tapped quote shows its original. */
    @Test
    @Config(qualifiers = "+fr-h640dp", fontScale = 1.3f)
    fun ON_6_quoteOriginalWithTheKeyboardUp_fr() {
        launch("demo" to true)
        camilleWrote()
        tap("Camille")
        val chat = ChatId.contact(contact("Camille").key)
        val first = model.chats[chat].orEmpty().first { !it.outgoing }
        val hers = (first.message as com.tduhamel.lichen.core.Message.Text).text
        compose.onAllNodesWithText(hers).onLast().performTouchInput { longClick() }
        compose.waitForIdle()
        tapInDialog(R.string.reply)
        keyboard(250)
        val reply = "Great, see you there"
        // The composer stays on screen while we reply (before, the banner pushed it out).
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(reply)
        tapDesc(R.string.send)
        advanceUntil("our reply sent", 30) { model.chats[chat].orEmpty().any { it.outgoing && it.quote != null } }
        advance(2)
        // Her message out of sight, the quote in our reply; tapped, her message in sight.
        assertFalse("her message still in sight", hersInSight(hers))
        compose.onAllNodesWithText(hers.take(20), substring = true).onLast().performClick()
        compose.mainClock.advanceTimeBy(1_000)
        advance(1)
        assertTrue("her message not brought into sight: ${shown()}", hersInSight(hers))
    }

    /** Her message's own bubble, on the left (the quote in our reply, on the right, says the
     * same words), at least half in the list's view. */
    private fun hersInSight(hers: String): Boolean {
        val list = compose.onNode(hasScrollToIndexAction()).fetchSemanticsNode().boundsInRoot
        return compose.onAllNodesWithText(hers, useUnmergedTree = true).fetchSemanticsNodes().any {
            val b = it.boundsInRoot
            val shown = minOf(b.bottom, list.bottom) - maxOf(b.top, list.top)
            b.left < list.left + list.width * 0.15f && shown >= b.height / 2
        }
    }

    /** B-user-1: Later puts the banner away, after a restart too; doing neither, it comes back. */
    @Test
    fun ON_6_compareLaterAndAfterARestart() {
        launch("demo" to true)
        camilleWrote()
        tap("Camille")
        text(R.string.compare_safety_numbers_with, "Camille").assertIsDisplayed()
        tapDesc(R.string.back)
        // Neither compared nor put off: back after a restart.
        restart("demo" to true)
        camilleWrote()
        tap("Camille")
        text(R.string.compare_safety_numbers_with, "Camille").assertIsDisplayed()
        tap(R.string.later)
        assertFalse(has(str(R.string.compare_safety_numbers_with, "Camille")))
        tapDesc(R.string.back)
        // Put off: not after a restart either.
        restart("demo" to true)
        camilleWrote()
        tap("Camille")
        assertFalse(has(str(R.string.compare_safety_numbers_with, "Camille")))
        assertFalse(contact("Camille").verified)
    }

    /** B-user-1: after scanning someone's code in person and seeing the same numbers, our code
     * for them to scan, so that their phone verifies us in the same sitting; their chat then
     * asks nothing (verified). */
    @Test
    fun ON_7_scannedInPersonThenShowYourCode() = scannedThenShowYourCode()

    @Test
    @Config(qualifiers = "+fr")
    fun ON_7_scannedInPersonThenShowYourCode_fr() = scannedThenShowYourCode()

    private fun scannedThenShowYourCode() {
        launch("demo" to true)
        advanceUntil("the radio's key") { model.nodeKey != null }
        val (card, _, node) = noeCard()
        // As the camera reads it, in person (Add a contact opens on it).
        model.debugInvite = card
        compose.waitForIdle()
        text(R.string.added_verified, "Noé").assertIsDisplayed()
        text(R.string.compare_numbers_with, "Noé").assertIsDisplayed()
        tap(R.string.done)
        text(R.string.now_let_scan_your_code, "Noé").assertIsDisplayed()
        val image = desc(R.string.your_contact_code).captureToImage().asAndroidBitmap()
        assertEquals(model.myCard(), decodeQr(image))
        tap(R.string.done)
        text(R.string.message_placeholder).assertIsDisplayed()
        assertTrue(model.contacts.first { it.key.contentEquals(node) }.verified)
        assertFalse(has(str(R.string.compare_safety_numbers_with, "Noé")))
    }

    /** Camille's first message in: her key known (not verified). */
    private fun camilleWrote() {
        advanceUntil("Camille's first message", 60) { model.last(ChatId.contact(contact("Camille").key))?.outgoing == false }
    }

    /** Noé's card: an identity of their own, a radio ours does not know yet. */
    private fun noeCard(): Triple<String, ByteArray, ByteArray> {
        val noe = LichenCore("test", ByteArray(32) { 5 }, ByteArray(32) { 55 }, null)
        val identity = noe.key()!!
        noe.close()
        val node = ByteArray(32) { (it * 7 + 3).toByte() }
        val card = "lichen:1:" + Base64.getUrlEncoder().withoutPadding().encodeToString(identity + node + "Noé".toByteArray())
        return Triple(card, identity, node)
    }

    private fun decodeQr(image: Bitmap): String {
        val px = IntArray(image.width * image.height)
        image.getPixels(px, 0, image.width, 0, 0, image.width, image.height)
        val bits = BinaryBitmap(HybridBinarizer(RGBLuminanceSource(image.width, image.height, px)))
        return QRCodeReader().decode(bits).text
    }
}
