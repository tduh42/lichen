package com.tduhamel.lichen.ui

import android.content.Intent
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.google.zxing.BinaryBitmap
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.channelQr
import com.tduhamel.lichen.model.hex
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.io.File

/** Area 5 of docs/testing.md: MeshCore chats and channels. */
@RunWith(AndroidJUnit4::class)
class MeshCoreTest : UiTest() {
    private val marcHello = "Hi! Marc from the refuge. Are you on lichen? I only have the MeshCore app."
    private val marcAnswer = "It works! See you tonight, I'll keep the soup warm."

    private fun count(t: String, substring: Boolean = false) =
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).fetchSemanticsNodes().size

    private fun messages(chat: ChatId) = model.chats[chat].orEmpty()

    private fun texts(chat: ChatId) = messages(chat).mapNotNull { (it.message as? Message.Text)?.text }

    private fun write(t: String) {
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(t)
        tapDesc(R.string.send)
    }

    private fun public(): ChatId {
        advanceUntil("the Public channel") { model.channels.any { it.name == "Public" } }
        return ChatId.channel(model.channels.first { it.name == "Public" }.id)
    }

    /** MC-1: Marc, with the stock MeshCore app, writes; our answer reaches his radio; he
     * answers once. His chat is marked MeshCore, not lichen-encrypted. */
    @Test
    fun MC_1_plainChatWithMarc() = plainChat()

    @Test
    @Config(qualifiers = "+fr-night")
    fun MC_1_plainChatWithMarc_frDark() = plainChat()

    private fun plainChat() {
        val answer = "Yes! Two of us, around seven."
        launch("demo" to true, "open" to "Marc")
        val chat = ChatId.mesh(contact("Marc").key)
        advanceUntil("Marc's hello", 120) { texts(chat).contains(marcHello) }
        see(marcHello)
        assertTrue(has(str(R.string.not_lichen_encrypted), substring = true) || has(str(R.string.meshcore), substring = true))
        // Our answer once the hub is quiet (Camille's photo goes through it in the first minutes).
        advanceUntil("the hub quiet", 400) { model.now() >= 330_000uL }
        write(answer)
        advanceUntil("our answer acknowledged by his radio", 120, { messages(chat).filter { it.outgoing }.map { it.status } }) {
            messages(chat).any { it.outgoing && (it.message as? Message.Text)?.text == answer && it.status == MessageStatus.Delivered }
        }
        advanceUntil("Marc's answer", 120, { texts(chat) }) { texts(chat).contains(marcAnswer) }
        advance(60)
        see(marcAnswer)
        assertEquals(1, count(marcAnswer))
        assertEquals(1, texts(chat).count { it == answer })
        // In Messages: his row, marked MeshCore.
        tapDesc(R.string.back)
        text("Marc").assertIsDisplayed()
        text(R.string.meshcore).assertIsDisplayed()
    }

    /** MC-2: the Public channel: posts with their authors; ours goes out, once. */
    @Test
    fun MC_2_publicChannelReadAndPost() = publicChannel()

    @Test
    @Config(qualifiers = "+fr")
    fun MC_2_publicChannelReadAndPost_fr() = publicChannel()

    private fun publicChannel() {
        launch("demo" to true, "open" to "Public")
        val chat = public()
        advanceUntil("Lucie's post", 60) { texts(chat).any { it.startsWith("Fog on the Pas de la Ville") } }
        see("Lucie")
        see("Fog on the Pas de la Ville", substring = true)
        write("Heading up the ridge at noon")
        advanceUntil("our post on the air", 60, { messages(chat).filter { it.outgoing }.map { it.status } }) {
            messages(chat).any { it.outgoing && it.status == MessageStatus.Flooded }
        }
        assertEquals(1, count("Heading up the ridge at noon"))
        see(str(R.string.state_sent), substring = true)
    }

    /** MC-3: the channel's QR code, after the warning, is the code the stock app reads; a
     * channel's code scanned joins it. */
    @Test
    fun MC_3_channelQrShowAndJoin() = channelQr()

    @Test
    @Config(qualifiers = "+fr")
    fun MC_3_channelQrShowAndJoin_fr() = channelQr()

    private fun channelQr() {
        launch("demo" to true, "open" to "Public")
        val chat = public()
        tapDesc(R.string.channel_details)
        tap(R.string.share_by_qr)
        text(R.string.whoever_scans_joins).assertIsDisplayed()
        assertFalse(hasDesc(str(R.string.qr_of, "Public")))
        tap(R.string.show_code)
        val image = desc(str(R.string.qr_of, "Public")).captureToImage().asAndroidBitmap()
        val code = model.channelQr(chat.hex.unhexed())
        assertEquals(code, decodeQr(image))
        assertTrue(code!!.startsWith("meshcore://channel/add?name="))

        // Another channel's code, scanned: joined, and its chat opens.
        repeat(3) { runCatching { tapDesc(R.string.back) } }
        tab(R.string.tab_messages)
        tapDesc(R.string.new_message)
        tap(R.string.join_a_channel)
        scanForTests = "meshcore://channel/add?name=Refuge%20crew&secret=" + "5a".repeat(16)
        tap(R.string.scan_channel_button)
        text("Refuge crew").assertIsDisplayed()
        text(R.string.channel_will_follow).assertIsDisplayed()
        tap(R.string.join_the_channel)
        assertTrue(model.channels.any { it.name == "Refuge crew" })
        text(R.string.message_placeholder).assertIsDisplayed()
        text("Refuge crew").assertIsDisplayed()
    }

    /** MC-4: the MeshCore app's history comes in once, older than what lichen got; a second
     * import adds nothing. */
    @Test
    fun MC_4_importTheMeshCoreHistory() = importHistory()

    @Test
    @Config(qualifiers = "+fr")
    fun MC_4_importTheMeshCoreHistory_fr() = importHistory()

    private fun importHistory() {
        launch("demo" to true)
        val marc = contact("Marc")
        val chat = ChatId.mesh(marc.key)
        advanceUntil("Marc's hello", 120) { model.last(chat) != null }
        val export = exportDb(model.nodeKey!!, marc.key, System.currentTimeMillis() / 1000 - 7 * 86_400)
        tab(R.string.tab_settings)
        tap(R.string.import_meshcore)
        answerNextActivity(Intent().setData(Uri.fromFile(export)))
        val direct = plural(R.plurals.import_direct, 2, 2)
        val channel = plural(R.plurals.import_channel, 1, 1)
        text(R.string.import_title).assertIsDisplayed()
        text(str(R.string.import_done, direct, channel)).assertIsDisplayed()
        tapInDialog(R.string.ok)
        // Again: nothing more, said so.
        tap(R.string.import_meshcore)
        answerNextActivity(Intent().setData(Uri.fromFile(export)))
        val none = str(R.string.import_done, plural(R.plurals.import_direct, 0, 0), plural(R.plurals.import_channel, 0, 0))
        text(none + " " + plural(R.plurals.import_already_here, 3, 3)).assertIsDisplayed()
        tapInDialog(R.string.ok)
        // Marc's chat: the two older messages first, his hello still the last.
        tab(R.string.tab_messages)
        tap("Marc")
        val t = texts(chat)
        assertEquals(listOf("Are you coming up this weekend?", "Saturday, with a friend."), t.take(2))
        assertTrue(t.drop(2).first().startsWith("Hi! Marc from the refuge."))
        assertEquals(1, count("Are you coming up this weekend?"))
    }

    /** The MeshCore app's export ("export app database", its schema 29): Marc both ways and a
     * post on Public, as the stock app keeps them (as QA-core's MC-4 builds it). */
    private fun exportDb(me: ByteArray, marc: ByteArray, atS: Long): File {
        val f = File(context.cacheDir, "meshcore-test-export.db").apply { delete() }
        SQLiteDatabase.openOrCreateDatabase(f, null).use { db ->
            db.execSQL("""CREATE TABLE "contact_messages" ("id" INTEGER PRIMARY KEY, "status" TEXT NOT NULL, "to" TEXT NOT NULL, "from" TEXT NOT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL)""")
            db.execSQL("""CREATE TABLE "channel_messages" ("id" INTEGER PRIMARY KEY, "channel_secret" BLOB NULL, "from" TEXT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL)""")
            db.execSQL("""CREATE TABLE "contacts" ("id" INTEGER PRIMARY KEY, "public_key" BLOB NOT NULL, "adv_name" TEXT NOT NULL, "custom_name" TEXT NULL)""")
            val insert = """INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp) VALUES (?, ?, ?, 0, ?, ?, ?)"""
            db.execSQL(insert, arrayOf<Any>("received", me.hex(), marc.hex(), atS, "Are you coming up this weekend?", atS * 1000))
            db.execSQL(insert, arrayOf<Any>("delivered", marc.hex(), me.hex(), atS + 60, "Saturday, with a friend.", (atS + 60) * 1000))
            val publicSecret = byteArrayOf(0x8B.toByte(), 0x33, 0x87.toByte(), 0xE9.toByte(), 0xC5.toByte(), 0xCD.toByte(), 0xEA.toByte(), 0x6A, 0xC9.toByte(), 0xE5.toByte(), 0xED.toByte(), 0xBA.toByte(), 0xA1.toByte(), 0x15, 0xCD.toByte(), 0x72)
            db.execSQL(
                """INSERT INTO channel_messages (channel_secret, "from", txt_type, sender_timestamp, text, timestamp) VALUES (?, NULL, 0, ?, 'Lucie: Snow above 1800 m', ?)""",
                arrayOf<Any>(publicSecret, atS + 120, (atS + 120) * 1000),
            )
            db.execSQL("INSERT INTO contacts (public_key, adv_name, custom_name) VALUES (?, 'Marc', NULL)", arrayOf<Any>(marc))
        }
        return f
    }

    private fun String.unhexed(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    private fun decodeQr(image: android.graphics.Bitmap): String {
        val px = IntArray(image.width * image.height)
        image.getPixels(px, 0, image.width, 0, 0, image.width, image.height)
        return QRCodeReader().decode(BinaryBitmap(HybridBinarizer(RGBLuminanceSource(image.width, image.height, px)))).text
    }
}
