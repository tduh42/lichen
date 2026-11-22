package com.tduhamel.lichen.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.model.ChatId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Area 7 of docs/testing.md: groups. */
@RunWith(AndroidJUnit4::class)
class GroupTest : UiTest() {
    private val demoGroup = "Rando Vercors"

    private fun count(t: String, substring: Boolean = false) =
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).fetchSemanticsNodes().size

    private fun messages(chat: ChatId) = model.chats[chat].orEmpty()

    private fun texts(chat: ChatId) = messages(chat).mapNotNull { (it.message as? Message.Text)?.text }

    private fun write(t: String) {
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(t)
        tapDesc(R.string.send)
    }

    /** The demo's group, once Camille made it and we are in. */
    private fun demoGroup(): ChatId {
        advanceUntil("the demo group", 200) { model.groups.any { it.name == demoGroup } }
        return ChatId.group(model.groups.first { it.name == demoGroup }.gid)
    }

    /** GR-1: a group of our own: in Messages, with us alone in it. */
    @Test
    fun GR_1_createAGroup() = create()

    @Test
    @Config(qualifiers = "+fr-night")
    fun GR_1_createAGroup_frDark() = create()

    private fun create() {
        launch("demo" to true)
        contact("Camille")
        // New message › New group: its own screen (F-4).
        tapDesc(R.string.new_message)
        tap(R.string.new_group)
        compose.onNode(hasSetTextAction() and hasText(str(R.string.group_name))).performTextInput("  Ridge walk  ")
        tap(R.string.create_group)
        // Its chat opens; the name trimmed.
        val g = model.groups.single { it.name == "Ridge walk" }
        assertEquals(1, g.members.size)
        text(R.string.message_placeholder).assertIsDisplayed()
        text("Ridge walk").assertIsDisplayed()
        // Alone in it.
        tapDesc(R.string.group_details)
        see(R.string.you)
        assertTrue(model.groups.single { it.name == "Ridge walk" }.members.size == 1)
        tapDesc(R.string.back)
        tapDesc(R.string.back)
        text("Ridge walk").assertIsDisplayed()
    }

    /** GR-2: our group's invitation QR; Léa, next to us, scans it and joins: she is a member. */
    @Test
    fun GR_2_inviteByQrSomeoneJoins() = invite()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_2_inviteByQrSomeoneJoins_fr() = invite()

    private fun invite() {
        launch("demo" to true, "screen" to "invite", "inviteDemo" to true)
        val name = str(R.string.sunday_hike)
        advanceUntil("our group", 30) { model.groups.any { it.name == name } }
        advanceUntil("the invitation's code", 30) { hasDesc(str(R.string.invitation_code_for, name)) }
        advanceUntil("Léa joined", 180) { model.groups.first { it.name == name }.members.any { it.name == "Léa" } }
        see("Léa")
        assertEquals(2, model.groups.first { it.name == name }.members.size)
    }

    /** GR-3: Camille's invitation scanned: what it is; joined, the group opens with its members. */
    @Test
    fun GR_3_joinByScanningAnInvitation() = join()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_3_joinByScanningAnInvitation_fr() = join()

    private fun join() {
        launch("demo" to true, "screen" to "join")
        advanceUntil("the join panel", 120) { has(R.string.join) }
        see(str(R.string.join_q, "Refuge tonight"))
        see(str(R.string.invitation_from, "Camille"))
        tap(R.string.join)
        see(str(R.string.asking_admin, "Camille"))
        advanceUntil("in Refuge tonight", 300) { model.groups.any { it.name == "Refuge tonight" } }
        val g = model.groups.first { it.name == "Refuge tonight" }
        assertTrue(g.members.any { it.name == "Camille" })
        see(str(R.string.joined_group, "Refuge tonight"))
        tap(R.string.open_group)
        text(R.string.message_placeholder).assertIsDisplayed()
        text("Refuge tonight").assertIsDisplayed()
    }

    /** GR-4: Camille's group as an invitation in Messages: joined, or declined and gone. */
    @Test
    fun GR_4_invitationAccepted() = invitation(accept = true)

    @Test
    @Config(qualifiers = "+fr")
    fun GR_4_invitationDeclined_fr() = invitation(accept = false)

    private fun invitation(accept: Boolean) {
        launch("demo" to true, "groupInvitationDemo" to true)
        advanceUntil("Camille's invitation", 200) { model.invitations.isNotEmpty() }
        assertTrue(model.groups.none { it.name == demoGroup })
        see(str(R.string.invites_you_to, "Camille", demoGroup))
        if (accept) {
            tap(R.string.join)
            advanceUntil("in the group", 60) { model.groups.any { it.name == demoGroup } }
            assertTrue(model.invitations.isEmpty())
            see(demoGroup)
        } else {
            tap(R.string.decline)
            assertTrue(model.invitations.isEmpty())
            advance(60)
            assertTrue(model.groups.none { it.name == demoGroup })
            assertFalse(has(str(R.string.invites_you_to, "Camille", demoGroup)))
        }
    }

    /** GR-5: posts in the demo group: each with its author, in order; ours goes out. */
    @Test
    fun GR_5_postAndReceiveInAGroup() = posts()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_5_postAndReceiveInAGroup_fr() = posts()

    private fun posts() {
        launch("demo" to true, "open" to demoGroup)
        val chat = demoGroup()
        advanceUntil("Julien's post", 300) { texts(chat).any { it.startsWith("Leaving the col now") } }
        advanceUntil("in the group's chat", 30) { has(R.string.message_placeholder) }
        see("Julien")
        see("Leaving the col now", substring = true)
        write("Coming up behind you")
        advanceUntil("our post sent", 60) {
            messages(chat).any { it.outgoing && (it.message as? Message.Text)?.text == "Coming up behind you" && it.status !is MessageStatus.Waiting }
        }
        assertEquals(1, count("Coming up behind you"))
        // In the order they came.
        val ids = messages(chat).map { it.id }
        assertEquals(ids.sorted(), ids)
    }

    /** GR-6: Julien's post the radio missed shows a gap, then comes back marked late. */
    @Test
    fun GR_6_lostPostComesBackLate() = late()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_6_lostPostComesBackLate_fr() = late()

    private fun late() {
        launch("demo" to true, "open" to demoGroup)
        val chat = demoGroup()
        advanceUntil("Julien's post back, late", 600, { texts(chat) }) {
            messages(chat).any { it.late && (it.message as? Message.Text)?.text?.startsWith("Mist on the ridge") == true }
        }
        advanceUntil("in the group's chat", 30) { has(R.string.message_placeholder) }
        see("Mist on the ridge", substring = true)
        val m = messages(chat).first { (it.message as? Message.Text)?.text?.startsWith("Mist on the ridge") == true }
        assertTrue(m.late)
        assertTrue(has(str(R.string.arrived_late, "", "").substringAfter("· ").substringBefore(",").trim(), substring = true))
        assertEquals(1, count("Mist on the ridge", substring = true))
    }

    /** GR-7: a group voice note (6 s at most, 5 fragments): its cost before sending; sent;
     * played. */
    @Test
    fun GR_7_groupVoiceNote() = voice()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_7_groupVoiceNote_fr() = voice()

    private fun voice() {
        launch("demo" to true, "open" to demoGroup, "screen" to "voiceReview")
        val chat = demoGroup()
        advanceUntil("the review bar", 30) { hasDesc(str(R.string.discard_recording)) }
        // Its cost, before sending: flooded to the group.
        assertTrue(has(str(R.string.group_voice_cost, "", "").substringBefore(":"), substring = true))
        tapDesc(R.string.send)
        advanceUntil("the voice note sent", 120, { messages(chat).filter { it.outgoing }.map { it.status } }) {
            messages(chat).any { it.outgoing && it.message is Message.Voice && it.status !is MessageStatus.Waiting }
        }
        val v = messages(chat).first { it.outgoing && it.message is Message.Voice }
        assertTrue("5 fragments at most", v.fragments.toInt() in 1..5)
        assertFalse(hasDesc(str(R.string.discard_recording)))
        // Played (the JVM has no sound: playing must not crash, B-and-15).
        tapDesc(R.string.play_voice)
        text(R.string.message_placeholder).assertIsDisplayed()
    }

    /** GR-8: our group renamed, its members listed; the demo group left: nothing more from it. */
    @Test
    fun GR_8_renameMembersLeave() = renameAndLeave()

    @Test
    @Config(qualifiers = "+fr")
    fun GR_8_renameMembersLeave_fr() = renameAndLeave()

    private fun renameAndLeave() {
        launch("demo" to true, "open" to demoGroup)
        val chat = demoGroup()
        advanceUntil("in the group's chat", 30) { has(R.string.message_placeholder) }
        tapDesc(R.string.group_details)
        // Its members.
        see("Camille")
        see("Julien")
        see(R.string.you)
        // Leave: confirmed, gone; Julien's later posts do not come.
        tap(R.string.leave_group)
        text(R.string.leave_group_q).assertIsDisplayed()
        tapInDialog(R.string.leave_group)
        advanceUntil("out of the group", 30) { model.groups.none { it.name == demoGroup } }
        val before = texts(chat).size
        advance(300)
        assertEquals(before, texts(chat).size)

        // Our own group, renamed: the new name everywhere.
        model.createGroup("Ridge walk", emptyList())
        val g = model.groups.first { it.name == "Ridge walk" }
        model.openChat = ChatId.group(g.gid)
        compose.waitForIdle()
        tapDesc(R.string.group_details)
        tap(R.string.group_name_label)
        compose.onAllNodes(hasSetTextAction()).onLast().performTextReplacement("Ridge walk, Sunday")
        tapInDialog(R.string.rename)
        assertTrue(model.groups.any { it.name == "Ridge walk, Sunday" })
        see("Ridge walk, Sunday")
    }
}
