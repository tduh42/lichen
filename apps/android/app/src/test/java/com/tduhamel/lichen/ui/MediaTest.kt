package com.tduhamel.lichen.ui

import android.graphics.Bitmap
import android.provider.MediaStore
import com.tduhamel.lichen.model.hex
import org.robolectric.Robolectric
import java.io.File
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.core.prepareImage
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.samplePhoto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Area 6 of docs/testing.md: media. The Internet bridge stays off (iroh: MD-7 is the FFI's
 * test over the network); the tests run offline. */
@RunWith(AndroidJUnit4::class)
class MediaTest : UiTest() {
    private fun messages(chat: ChatId) = model.chats[chat].orEmpty()

    private fun ours(chat: ChatId) = messages(chat).filter { it.outgoing }

    private fun write(t: String) {
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(t)
        tapDesc(R.string.send)
    }

    /** The app with the demo and these extras, iroh off (before any sheet asks for it); with
     * `open`, Camille's lichen chat once her key is known (her MeshCore chat before it). */
    private fun start(vararg extras: Pair<String, Any>): ChatId {
        launch("demo" to true, *extras)
        model.chooseIroh(false)
        advanceUntil("Camille's key", 120) { model.contacts.any { it.name == "Camille" && it.identity != null } }
        val chat = ChatId.contact(contact("Camille").key)
        // Her lichen chat: the composer's Attach (her MeshCore chat has none), or the review bar.
        if (extras.any { it.first == "open" }) {
            advanceUntil("her lichen chat", 30) { hasDesc(str(R.string.attach)) || hasDesc(str(R.string.discard_recording)) }
        }
        return chat
    }

    /** The photo sheet: the picture is prepared on a thread of its own (real time). */
    private fun photoSheet() = waitReal("the photo sheet", 30_000) { has(R.string.send_a_photo) }

    private fun quietHub() = advanceUntil("the hub quiet", 400) { model.now() >= 330_000uL }

    /** MD-1: a photo to Camille: its cost first, the preview goes over LoRa and arrives. */
    @Test
    fun MD_1_sendAPhoto() = sendPhoto()

    @Test
    @Config(qualifiers = "+fr")
    fun MD_1_sendAPhoto_fr() = sendPhoto()

    private fun sendPhoto() {
        val chat = start("open" to "Camille", "screen" to "media")
        photoSheet()
        // What it is, in the app's language (B-and-17).
        val photo = samplePhoto()
        val image = prepareImage(photo.rgba, photo.width.toUInt(), photo.height.toUInt())
        see(str(R.string.photo_sizes, image.width.toInt(), image.height.toInt(), image.preview.size, image.full.size))
        see(R.string.lora_preview)
        assertTrue("its airtime, before sending", has("≈ ", substring = true))
        quietHub()
        tap(R.string.send_preview)
        advanceUntil("the preview delivered", 300, { ours(chat).map { it.status } }) {
            ours(chat).any { it.message is Message.Image && it.status == MessageStatus.Delivered }
        }
        assertEquals(1, ours(chat).count { it.message is Message.Image })
    }

    /** MD-1: a photo that does not fit in the hour's airtime is refused, and said so. */
    @Test
    fun MD_1_photoRefusedOverTheHoursAirtime() {
        val chat = start("open" to "Camille", "screen" to "media")
        photoSheet()
        // Previews until the hour's airtime is spent.
        val photo = samplePhoto()
        val image = prepareImage(photo.rgba, photo.width.toUInt(), photo.height.toUInt())
        var refused: String? = null
        for (i in 0 until 500) {
            refused = model.sendImage(image, chat) ?: continue
            break
        }
        assertNotNull("refused at last", refused)
        assertTrue(refused!!, refused.startsWith(str(R.string.err_over_budget, 1).substringBefore(":")))
        val queued = ours(chat).size
        // The sheet says so, and keeps the photo.
        tap(R.string.send_preview)
        see(refused.substringBefore(":"), substring = true)
        text(R.string.send_a_photo).assertIsDisplayed()
        assertEquals(queued, ours(chat).size)
    }

    /** MD-2: Camille asks for the full photo: the card on it; Not now, she asks again later;
     * Send, the full photo goes and she thanks us. */
    @Test
    fun MD_2_fullPhotoRequested() {
        val chat = start("open" to "Camille", "screen" to "media")
        photoSheet()
        quietHub()
        tap(R.string.send_preview)
        advanceUntil("her request", 300) { ours(chat).any { it.message is Message.Image && model.fullRequest(it) != null } }
        val photo = ours(chat).first { it.message is Message.Image }
        val asks = str(R.string.full_photo_request, "Camille", 0).substringBefore("0").substringAfter("Camille").trim()
        see(asks, substring = true)
        tap(R.string.not_now)
        assertEquals(null, model.fullRequest(photo))
        // Asked again, three minutes after her request.
        advanceUntil("her second request", 300) { model.fullRequest(photo) != null }
        see(asks, substring = true)
        tap(R.string.send)
        advanceUntil("her thanks", 600) {
            messages(chat).any { !it.outgoing && (it.message as? Message.Text)?.text == "Got the full picture, thanks!" }
        }
        assertTrue(ours(chat).any { (it.message as? Message.Image)?.full == true })
    }

    /** MD-3: Camille's photo: its card; opened in the viewer, closed. (Share shows once the
     * AVIF is decoded, which the JVM cannot: left to the emulator.) */
    @Test
    fun MD_3_receiveAPhotoAndOpenIt() = receivePhoto()

    @Test
    @Config(qualifiers = "+fr-night")
    fun MD_3_receiveAPhotoAndOpenIt_frDark() = receivePhoto()

    private fun receivePhoto() {
        val chat = start("open" to "Camille")
        advanceUntil("her photo", 400) { messages(chat).any { !it.outgoing && it.message is Message.Image } }
        see("The view from the refuge tonight", substring = true)
        // Its card ("Photo", the thumbnail or its place), opened by a tap.
        compose.onAllNodesWithText(str(R.string.photo), useUnmergedTree = true).onFirst().assertIsDisplayed()
        val opens = SemanticsMatcher("opens the photo") { it.config.getOrElseNullable(SemanticsActions.OnClick) { null }?.label == str(R.string.open_photo) }
        compose.onAllNodes(opens).onFirst().performClick()
        compose.waitForIdle()
        desc(R.string.close).assertIsDisplayed()
        tapDesc(R.string.close)
        assertFalse(hasDesc(str(R.string.close)))
        text(R.string.message_placeholder).assertIsDisplayed()
    }

    /** MD-3: Camille's photo, its original fetched, saved to the phone's gallery from the viewer:
     * in Pictures/lichen, as it came, and said so (D-5). (The JVM decodes the JPEG original,
     * not the AVIF thumbnails.) */
    @Test
    fun MD_3_saveToTheGallery() = save()

    @Test
    @Config(qualifiers = "+fr")
    fun MD_3_saveToTheGallery_fr() = save()

    private fun save() {
        FakeGallery.items.clear()
        Robolectric.setupContentProvider(FakeGallery::class.java, MediaStore.AUTHORITY)
        val chat = start("open" to "Camille")
        advanceUntil("her photo and its announcement", 400) {
            messages(chat).any { m -> !m.outgoing && (m.message as? Message.Image)?.let { model.announcement(it.mediaId, chat) } != null }
        }
        val image = messages(chat).first { !it.outgoing && it.message is Message.Image }.message as Message.Image
        val announce = model.announcement(image.mediaId, chat)!!.message as Message.Announce
        // Its original, as if fetched over the bridge.
        val original = File(context.cacheDir, "original.jpg")
        original.outputStream().use { out ->
            Bitmap.createBitmap(64, 48, Bitmap.Config.ARGB_8888).apply { eraseColor(0xFF3366AA.toInt()) }
                .compress(Bitmap.CompressFormat.JPEG, 90, out)
        }
        model.downloads[announce.hash.hex()] = original
        val opens = SemanticsMatcher("opens the photo") { it.config.getOrElseNullable(SemanticsActions.OnClick) { null }?.label == str(R.string.open_photo) }
        compose.onAllNodes(opens).onFirst().performClick()
        waitReal("the original shown") { hasDesc(str(R.string.share)) }
        assertTrue("a Save button", hasDesc(str(R.string.save)))
        tapDesc(R.string.save)
        waitReal("saved") { has(R.string.photo_saved) }
        val saved = FakeGallery.items.single()
        assertEquals("Pictures/lichen", saved.values.getAsString(MediaStore.Images.Media.RELATIVE_PATH).trimEnd('/'))
        assertEquals("image/jpeg", saved.values.getAsString(MediaStore.Images.Media.MIME_TYPE))
        assertEquals(0, saved.values.getAsInteger(MediaStore.Images.Media.IS_PENDING))
        assertTrue(saved.values.getAsString(MediaStore.Images.Media.DISPLAY_NAME).endsWith(".jpg"))
        assertTrue("as it came", original.readBytes().contentEquals(saved.file.readBytes()))
    }

    /** MD-4: a recording under review: its cost, sent, then played. */
    @Test
    fun MD_4_voiceReviewSendPlay() = voice()

    @Test
    @Config(qualifiers = "+fr")
    fun MD_4_voiceReviewSendPlay_fr() = voice()

    private fun voice() {
        val chat = start("open" to "Camille", "screen" to "voiceReview")
        advanceUntil("the review bar", 30) { hasDesc(str(R.string.discard_recording)) }
        // Its quality against its airtime, before sending.
        assertTrue("its cost", has(str(R.string.voice_quality_cost, "", "").substringBefore(":"), substring = true))
        tapDesc(R.string.send)
        advanceUntil("the voice note delivered", 300, { ours(chat).map { it.status } }) {
            ours(chat).any { it.message is Message.Voice && it.status == MessageStatus.Delivered }
        }
        // Played (the JVM has no sound: playing must not crash, B-and-15).
        tapDesc(R.string.play_voice)
        text(R.string.message_placeholder).assertIsDisplayed()
    }

    /** MD-5: a small note goes over LoRa; a large document cannot, and says so. */
    @Test
    fun MD_5_smallFileSent() = smallFile()

    @Test
    @Config(qualifiers = "+fr")
    fun MD_5_smallFileSent_fr() = smallFile()

    private fun smallFile() {
        val chat = start("open" to "Camille", "screen" to "file")
        advanceUntil("the file sheet", 30) { has(R.string.send_a_file) }
        see("Refuge notes.txt")
        see(R.string.over_lora)
        quietHub()
        tap(R.string.send_over_lora)
        advanceUntil("the note delivered", 400, { ours(chat).map { it.status } }) {
            ours(chat).any { it.message is Message.File && it.status == MessageStatus.Delivered }
        }
        see("Refuge notes.txt")
    }

    @Test
    fun MD_5_bigFileRefusedOverLora() = bigFile()

    @Test
    @Config(qualifiers = "+fr")
    fun MD_5_bigFileRefusedOverLora_fr() = bigFile()

    private fun bigFile() {
        val chat = start("open" to "Camille", "screen" to "fileBig")
        advanceUntil("the file sheet", 30) { has(R.string.send_a_file) }
        see("Topo Vercors 2026.pdf")
        see(R.string.file_no_lora_title)
        see(str(R.string.too_large_for_lora, "").substringBefore("("), substring = true)
        compose.onAllNodesWithText(str(R.string.send_announcement)).onFirst().assertIsNotEnabled()
        assertTrue(ours(chat).isEmpty())
    }

    /** MD-6: a sticker from the picker, sent and shown; a GIPHY link goes as a GIF. */
    @Test
    fun MD_6_stickerAndGif() {
        val chat = start("open" to "Camille")
        advanceUntil("her chat", 30) { has(R.string.message_placeholder) }
        tapDesc(R.string.attach)
        tapInDialog(R.string.sticker)
        text(R.string.stickers).assertIsDisplayed()
        compose.onAllNodes(hasContentDescription(str(R.string.sticker_named, ""), substring = true)).onFirst().performClick()
        compose.waitForIdle()
        advanceUntil("the sticker sent", 120) { ours(chat).any { it.message is Message.Sticker } }
        write("https://giphy.com/gifs/sunset-sunrise-denali-l4KidtNiqsus526M8")
        advanceUntil("the GIF sent", 120) { ours(chat).any { it.message is Message.Gif } }
    }
}
