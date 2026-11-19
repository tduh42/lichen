package com.tduhamel.lichen.ui

import android.Manifest
import android.app.Application
import android.app.NotificationManager
import android.content.Intent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.timerLabel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Area 4 of docs/testing.md: direct lichen chats. */
@RunWith(AndroidJUnit4::class)
class DirectChatTest : UiTest() {
    private val first = "Hi! I'm at the refuge, the mobile network doesn't reach up here."

    private fun count(t: String, substring: Boolean = false) =
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).fetchSemanticsNodes().size

    /** Types in the chat's field and sends. */
    private fun write(t: String) {
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(t)
        tapDesc(R.string.send)
    }

    private fun longPress(t: String, substring: Boolean = false) {
        compose.onAllNodesWithText(t, substring = substring).onLast().performTouchInput { longClick() }
        compose.waitForIdle()
    }

    /** Where a label starts, in the menu on screen. */
    private fun left(t: String): Float =
        compose.onAllNodes(hasText(t) and hasAnyAncestor(isPopup()), useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot.left

    private fun camille(): ChatId = ChatId.contact(contact("Camille").key)

    private fun messages(chat: ChatId) = model.chats[chat].orEmpty()

    private fun ours(chat: ChatId, t: String) = messages(chat).filter { it.outgoing && (it.message as? Message.Text)?.text == t }

    /** Camille's chat open, her first message in. */
    private fun openCamille(vararg extras: Pair<String, Any>) {
        launch("demo" to true, "open" to "Camille", *extras)
        val chat = camille()
        advanceUntil("Camille's first message", 60) { messages(chat).any { !it.outgoing } }
    }

    /** DM-1: a text sent: sending, then delivered; shown once. */
    @Test
    fun DM_1_sendAText() = send()

    @Test
    @Config(qualifiers = "+fr-night")
    fun DM_1_sendAText_frDark() = send()

    private fun send() {
        // `say` answers Camille's first message (about 38 s in).
        launch("demo" to true, "open" to "Camille", "say" to "On my way, see you at six")
        val chat = camille()
        advanceUntil("our text in the chat", 60) { ours(chat, "On my way, see you at six").isNotEmpty() }
        assertEquals(1, count("On my way, see you at six"))
        advanceUntil("delivered", 60) { ours(chat, "On my way, see you at six").single().status == MessageStatus.Delivered }
        see(str(R.string.state_delivered), substring = true)
        assertEquals(1, count("On my way, see you at six"))
        assertEquals(1, ours(chat, "On my way, see you at six").size)
    }

    /** DM-2: Camille's text: her row, an unread badge on it and on the tab, a notification. */
    @Test
    fun DM_2_receiveAText() = receive()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_2_receiveAText_fr() = receive()

    private fun receive() {
        shadowOf(context as Application).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        launch("demo" to true)
        val chat = camille()
        advanceUntil("Camille's first message", 60) { model.last(chat)?.outgoing == false }
        text(first).assertIsDisplayed()
        // On her row and on the Messages tab.
        assertEquals(2, compose.onAllNodesWithContentDescription(plural(R.plurals.unread, 1, 1)).fetchSemanticsNodes().size)
        val notes = shadowOf(context.getSystemService(NotificationManager::class.java)).allNotifications
        assertTrue("a notification from Camille", notes.any { n ->
            n.extras.getCharSequence("android.title")?.toString() == "Camille" &&
                n.extras.getCharSequence("android.text")?.toString()?.contains("I'm at the refuge") == true
        })
    }

    /** DM-3: a long text, in many fragments, delivered whole. */
    @Test
    fun DM_3_longTextArrivesWhole() {
        openCamille()
        val chat = camille()
        // About 700 bytes: several fragments, well within the hour's airtime.
        val long = (1..10).joinToString(" ") { "Line $it of the route: ridge, then the refuge, water at the spring." }
        write(long)
        advanceUntil("the long text delivered", 300, { ours(chat, long).singleOrNull()?.status }) {
            ours(chat, long).singleOrNull()?.status == MessageStatus.Delivered
        }
        val m = ours(chat, long).single()
        assertTrue("fragmented", m.fragments.toInt() > 1)
        assertEquals(long, (m.message as Message.Text).text)
        assertEquals(1, count("Line 1 of the route", substring = true))
    }

    /** DM-4: a reply quotes what it answers; tapping the quote shows it. */
    @Test
    fun DM_4_reply() = reply()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_4_reply_fr() = reply()

    private fun reply() {
        openCamille()
        val chat = camille()
        longPress(first)
        // The menu's labels in one column (each item with its icon).
        assertEquals(left(str(R.string.message_info)), left(str(R.string.reply)), 1f)
        assertEquals(left(str(R.string.message_info)), left(str(R.string.copy)), 1f)
        tapInDialog(R.string.reply)
        text(R.string.replying_to, "Camille").assertIsDisplayed()
        write("Great, see you there")
        advanceUntil("our reply delivered", 60) { ours(chat, "Great, see you there").singleOrNull()?.status == MessageStatus.Delivered }
        val r = ours(chat, "Great, see you there").single()
        assertNotNull(r.quote)
        assertEquals(messages(chat).first { !it.outgoing }.id, r.quote!!.messageId)
        // The quote: her message's start, under our reply; tapped, her message.
        assertEquals(2, count("I'm at the refuge", substring = true))
        compose.onAllNodesWithText("I'm at the refuge", substring = true).onLast().performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText(first).onFirst().assertIsDisplayed()
    }

    /** DM-5: a reaction added, changed, removed. */
    @Test
    fun DM_5_reactChangeRemove() {
        openCamille()
        val chat = camille()
        fun theirs() = messages(chat).first { !it.outgoing }
        longPress(first)
        tapInDialog("👍")
        advanceUntil("our 👍", 30) { theirs().reactions.any { it.mine && it.emoji == "👍" } }
        assertTrue(has("👍", substring = true))
        // Changed to ❤️ from the long-press menu.
        longPress(first)
        tapInDialog("❤️")
        advanceUntil("❤️ instead", 30) { theirs().reactions.any { it.mine && it.emoji == "❤️" } }
        assertFalse(theirs().reactions.any { it.mine && it.emoji == "👍" })
        // The same again removes it.
        longPress(first)
        tapInDialog("❤️")
        advanceUntil("no reaction of ours", 30) { theirs().reactions.none { it.mine } }
    }

    /** DM-6: our text edited: the new text, marked edited. */
    @Test
    fun DM_6_editASentText() = edit()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_6_editASentText_fr() = edit()

    private fun edit() {
        openCamille()
        val chat = camille()
        write("Arriving at 6")
        advanceUntil("delivered", 60) { ours(chat, "Arriving at 6").singleOrNull()?.status == MessageStatus.Delivered }
        longPress("Arriving at 6")
        tapInDialog(R.string.edit)
        text(R.string.editing_your_message).assertIsDisplayed()
        compose.onAllNodes(hasSetTextAction()).onLast().performTextReplacement("Arriving at 7")
        tapDesc(R.string.send)
        advanceUntil("the edit applied", 60) { messages(chat).any { it.outgoing && it.edited && (it.message as? Message.Text)?.text == "Arriving at 7" } }
        assertEquals(1, count("Arriving at 7"))
        assertEquals(0, count("Arriving at 6"))
        see(str(R.string.edited), substring = true)
    }

    /** DM-7: deleted for me: gone here; for everyone: gone, and said so. */
    @Test
    fun DM_7_deleteForMeAndForEveryone() = delete()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_7_deleteForMeAndForEveryone_fr() = delete()

    private fun delete() {
        openCamille()
        val chat = camille()
        write("First draft")
        write("Second draft")
        advanceUntil("both delivered", 90) {
            ours(chat, "First draft").singleOrNull()?.status == MessageStatus.Delivered &&
                ours(chat, "Second draft").singleOrNull()?.status == MessageStatus.Delivered
        }
        longPress("First draft")
        tapInDialog(R.string.delete_for_me)
        text(R.string.delete_for_me_q).assertIsDisplayed()
        tapInDialog(R.string.delete_for_me)
        assertEquals(0, count("First draft"))
        assertTrue(ours(chat, "First draft").isEmpty())

        longPress("Second draft")
        tapInDialog(R.string.delete_for_everyone)
        text(R.string.delete_for_everyone_q).assertIsDisplayed()
        tapInDialog(R.string.delete_for_everyone)
        advanceUntil("deleted for everyone", 60) { ours(chat, "Second draft").isEmpty() }
        assertEquals(0, count("Second draft"))
    }

    /** DM-8: the timer set on the chat; messages vanish after it. */
    @Test
    fun DM_8_disappearingTimerIsSet() {
        openCamille("timer" to 300)
        assertEquals(300u, model.timers[camille()])
        desc(str(R.string.disappearing_on, timerLabel(context.resources, 300u))).assertIsDisplayed()
    }

    @Test
    fun DM_8_messagesVanishAfterTheTimer() {
        openCamille("timer" to 300)
        val chat = camille()
        write("Gone in five minutes")
        advanceUntil("delivered", 60) { ours(chat, "Gone in five minutes").singleOrNull()?.status == MessageStatus.Delivered }
        advanceUntil("vanished", 400) { ours(chat, "Gone in five minutes").isEmpty() }
        assertEquals(0, count("Gone in five minutes"))
    }

    /** DM-9: older pages load in order, without duplicates. */
    @Test
    fun DM_9_pagingOlderMessages() {
        openCamille("fill" to 250, "earlier" to true)
        val chat = camille()
        advanceUntil("the history filled", 30) { model.conversations.any { ChatId.of(it.chat) == chat } && messages(chat).size >= 100 }
        // Earlier pages, until all are here.
        repeat(5) {
            if (model.olderAvailable[chat] == true) {
                model.loadOlder(chat)
                compose.waitForIdle()
            }
        }
        val ids = messages(chat).map { it.id }
        assertEquals("no duplicates", ids.size, ids.toSet().size)
        assertEquals("in order", ids.sorted(), ids)
        assertTrue("the 250 older ones", messages(chat).size >= 250)
    }

    /** DM-10: a search across chats; a result opens its chat on it. */
    @Test
    fun DM_10_searchMessages() = search()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_10_searchMessages_fr() = search()

    private fun search() {
        launch("demo" to true)
        advanceUntil("Camille's and Lucie's", 60) { model.last(camille())?.outgoing == false }
        tapDesc(R.string.search_messages)
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("refuge")
        advanceUntil("results", 5) { has("mobile network", substring = true) }
        compose.onAllNodesWithText("mobile network", substring = true).onFirst().performClick()
        compose.waitForIdle()
        text(first).assertIsDisplayed()
        text(R.string.message_placeholder).assertIsDisplayed()
    }

    /** DM-11: Info on a delivered message: when, the route, fragments and airtime. */
    @Test
    fun DM_11_messageInfo() = info()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_11_messageInfo_fr() = info()

    private fun info() {
        openCamille()
        val chat = camille()
        write("How is the snow?")
        advanceUntil("delivered", 180, { ours(chat, "How is the snow?").singleOrNull()?.status }) {
            ours(chat, "How is the snow?").singleOrNull()?.status == MessageStatus.Delivered
        }
        longPress("How is the snow?")
        tapInDialog(R.string.message_info)
        val m = ours(chat, "How is the snow?").single()
        see(R.string.airtime)
        see(plural(R.plurals.fragments, m.fragments.toInt(), m.fragments.toInt()), substring = true)
    }

    /** DM-12: unread counts on the row and the tab; opening the chat clears them. */
    @Test
    fun DM_12_unreadAndMarkRead() = unread()

    @Test
    @Config(qualifiers = "+fr")
    fun DM_12_unreadAndMarkRead_fr() = unread()

    private fun unread() {
        launch("demo" to true)
        val chat = camille()
        advanceUntil("Camille's first message", 60) { model.last(chat)?.outgoing == false }
        assertEquals(1, model.unread(chat))
        // A badge on her row (and on Public's: Lucie's post); the tab counts them all.
        val one = plural(R.plurals.unread, 1, 1)
        val badges = compose.onAllNodesWithContentDescription(one).fetchSemanticsNodes().size
        assertTrue(badges >= 1)
        val total = model.totalUnread
        tap("Camille")
        tapDesc(R.string.back)
        // Read: her badge gone, one less on the tab.
        assertEquals(0, model.unread(chat))
        assertEquals(badges - 1, compose.onAllNodesWithContentDescription(one).fetchSemanticsNodes().size)
        assertEquals(total - 1, model.totalUnread)
    }

    /** DM-13: a message forwarded to Julien arrives in his chat. */
    @Test
    fun DM_13_forward() {
        openCamille()
        // Julien as a lichen contact: his identity known (the group's keys, about 100 s in).
        advanceUntil("Julien's identity", 240) { model.contacts.any { it.name == "Julien" && it.identity != null } }
        // Once the hub is quiet (Camille's photo goes through it in the first five minutes).
        advanceUntil("the hub quiet", 400) { model.now() >= 330_000uL }
        val julien = ChatId.contact(contact("Julien").key)
        longPress(first)
        tapInDialog(R.string.forward)
        tap("Julien")
        advanceUntil("forwarded to Julien", 300, { model.last(julien)?.let { "${it.status} ${(it.message as? Message.Text)?.text}" } }) {
            model.last(julien)?.let { it.outgoing && (it.message as? Message.Text)?.text == first && it.status == MessageStatus.Delivered } == true
        }
    }

    /** DM-13 (D-2): a blocked contact is not offered as a forward's target. */
    @Test
    fun DM_13_forwardLeavesBlockedContactsOut() {
        openCamille()
        val julien = contact("Julien")
        val marc = contact("Marc")
        model.block(julien.key)
        longPress(first)
        tapInDialog(R.string.forward)
        val sheet = hasScrollAction() and hasAnyDescendant(hasText(marc.name))
        assertTrue(compose.onAllNodes(hasText("Marc") and hasAnyAncestor(sheet), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
        assertTrue(compose.onAllNodes(hasText("Julien") and hasAnyAncestor(sheet), useUnmergedTree = true).fetchSemanticsNodes().isEmpty())
    }

    /** DM-14: a long press on a link shows the message's menu (it opens nothing); a place
     * shows on lichen's map. */
    @Test
    fun DM_14_linksAndPlaces() {
        openCamille()
        val chat = camille()
        val t = "Topo: https://example.org/topo and the hut at 45.12345, 5.67891"
        write(t)
        advanceUntil("our text", 10) { ours(chat, t).isNotEmpty() }
        // A long press on the text, a link under the finger: the menu, no browser.
        val app = shadowOf(context as Application)
        while (app.nextStartedActivity != null) Unit
        // On the address, on the first line.
        compose.onAllNodesWithText(t, useUnmergedTree = true).onLast().performTouchInput { longClick(Offset(width * 0.45f, height * 0.25f)) }
        compose.waitForIdle()
        assertEquals(null, app.nextStartedActivity)
        tapInDialog(R.string.show_on_map)
        assertEquals(45.12345, model.mapPin?.lat ?: 0.0, 1e-6)
        assertEquals(5.67891, model.mapPin?.lon ?: 0.0, 1e-6)
        // The Map tab, on the place.
        text(R.string.tab_map).assertIsDisplayed()
    }
}
