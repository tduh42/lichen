package com.tduhamel.lichen.ui

import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SegmentedButton
import com.tduhamel.lichen.model.VoicePlayer
import com.tduhamel.lichen.core.PreparedVoice
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.runtime.mutableIntStateOf
import android.text.format.Formatter
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.outlined.HourglassEmpty
import androidx.compose.material.icons.outlined.Verified
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.Spacer
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.PreparedImage
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.core.decodeVoice
import com.tduhamel.lichen.core.imageDatagramLen
import com.tduhamel.lichen.core.voiceDatagramLenFor
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.clock
import com.tduhamel.lichen.model.decodeImage
import com.tduhamel.lichen.model.fragmentsText
import com.tduhamel.lichen.model.hex
import java.io.File

/** A message's media, or false when it is shown as text. */
@Composable
fun MediaContent(model: AppModel, m: StoredMessage): Boolean {
    when (val msg = m.message) {
        is Message.Voice -> VoiceNote(model, m, msg)
        is Message.Image -> ImageCard(model, m, msg)
        is Message.File -> FileCard(msg)
        is Message.Sticker -> StickerView(msg.emoji)
        is Message.Gif -> GifCard(model, m, msg)
        // Shown inside its LoRa preview's card when that preview is here.
        is Message.Announce -> if (model.preview(msg.mediaId, ChatId.of(m.chat)) == null) {
            Column(
                Modifier
                    .width(250.dp)
                    .clip(RoundedCornerShape(18.dp))
                    .background(lichen.s1)
                    .border(1.dp, lichen.line, RoundedCornerShape(18.dp))
                    .padding(12.dp),
            ) { BridgeSection(model, m, standalone = true) }
        }
        else -> return false
    }
    return true
}

/** Play button (LoRa orange), waveform, length, codec (mockup `chat`). */
@Composable
private fun VoiceNote(model: AppModel, m: StoredMessage, v: Message.Voice) {
    val t = lichen
    // The core says whether this build plays the mode.
    val playable = remember(v.mode) { decodeVoice(v.mode, 0u, ByteArray(0)) != null }
    val playing = model.player.playing == m.id
    val fg = if (m.outgoing) t.actFg else t.tx
    Row(
        Modifier
            .background(if (m.outgoing) t.actBg else t.s2, RoundedCornerShape(18.dp))
            .padding(start = 4.dp, end = 14.dp, top = 2.dp, bottom = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        IconButton(
            onClick = { model.player.toggle(m) },
            enabled = playable,
            colors = IconButtonDefaults.iconButtonColors(containerColor = t.lora, contentColor = Color(0xFF141413)),
            modifier = Modifier.size(40.dp),
        ) {
            Icon(
                if (playing) Icons.Filled.Stop else Icons.Filled.PlayArrow,
                stringResource(if (playing) R.string.stop else R.string.play_voice),
            )
        }
        // The waveform fills as it plays; a tap or a drag on it goes there.
        val seed = v.data.copyOf(minOf(32, v.data.size))
        var width by remember { mutableIntStateOf(1) }
        val seekTo = { x: Float ->
            val f = x / width.coerceAtLeast(1)
            if (playing) model.player.seek(f) else model.player.play(m, f)
        }
        Box(
            Modifier
                .onSizeChanged { width = it.width }
                .then(
                    if (playable) {
                        Modifier
                            .pointerInput(m.id) { detectTapGestures { seekTo(it.x) } }
                            .pointerInput(m.id) { detectHorizontalDragGestures { change, _ -> seekTo(change.position.x) } }
                    } else {
                        Modifier
                    },
                ),
        ) {
            Waveform(seed, if (m.outgoing) fg.copy(alpha = 0.45f) else t.tx2.copy(alpha = 0.55f))
            val progress = if (playing) model.player.progress else 0f
            Box(Modifier.drawWithContent { clipRect(right = size.width * progress) { this@drawWithContent.drawContent() } }) {
                Waveform(seed, fg)
            }
        }
        Column {
            val total = v.durationMs.toDouble() / 1000
            Text(
                if (playing) "${clock(model.player.position)} / ${clock(total)}" else clock(total),
                fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = fg,
            )
            Text(codecName(v.mode.toInt()), fontSize = 11.sp, color = fg.copy(alpha = 0.7f))
        }
    }
}

/** "Codec2 1600" for a VOICE mode (§13.1). */
fun codecName(mode: Int): String = "Codec2 " + when (mode) {
    0 -> "700C"
    1 -> "1200"
    2 -> "1600"
    3 -> "2400"
    else -> "?"
}

/** Bars drawn from the Codec2 bits: stable and decorative. */
@Composable
fun Waveform(seed: ByteArray, color: Color, bars: Int = 22) {
    Row(Modifier.height(24.dp).clearAndSetSemantics {}, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        repeat(bars) { i ->
            val b = if (seed.isEmpty()) 128 else seed[i % seed.size].toInt() and 0xFF
            Box(Modifier.width(3.dp).height((5 + (b * 7 + i * 13) % 18).dp).background(color, CircleShape))
        }
    }
}

/** An image over LoRa: the preview or the full thumbnail, and how to get the full one. */
@Composable
private fun ImageCard(model: AppModel, m: StoredMessage, img: Message.Image) {
    val t = lichen
    val context = LocalContext.current
    val res = LocalResources.current
    val chat = ChatId.of(m.chat)
    val announce = model.announcement(img.mediaId, chat)
    val fetched = (announce?.message as? Message.Announce)?.let { model.downloads[it.hash.hex()] }
    val bitmap = remember(m.id, img.data.size, fetched) {
        (fetched?.let { decodeFile(it) } ?: decodeImage(img.data))?.asImageBitmap()
    }
    var error by remember { mutableStateOf<String?>(null) }
    var viewing by remember { mutableStateOf(false) }
    if (viewing) PhotoViewer(model, m) { viewing = false }
    val ratio = img.width.toFloat().coerceAtLeast(1f) / img.height.toFloat().coerceAtLeast(1f)
    Column(
        Modifier
            .width(250.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(t.s1)
            .border(1.dp, t.line, RoundedCornerShape(18.dp)),
    ) {
        // Tapped: full screen, zoomable.
        Box(
            Modifier.fillMaxWidth().aspectRatio(ratio).background(t.s2).clickable(onClickLabel = stringResource(R.string.open_photo)) { viewing = true },
            contentAlignment = Alignment.BottomStart,
        ) {
            if (bitmap != null) {
                Image(
                    bitmap, stringResource(R.string.photo),
                    Modifier.fillMaxWidth().aspectRatio(ratio).then(if (!img.full && fetched == null) Modifier.blur(1.5.dp) else Modifier),
                    contentScale = ContentScale.Crop,
                    filterQuality = FilterQuality.Medium,
                )
            }
            if (!img.full && fetched == null) {
                Text(
                    stringResource(R.string.lora_preview_bytes, img.data.size), fontSize = 11.sp, color = t.tx,
                    modifier = Modifier.padding(8.dp).background(t.bg.copy(alpha = 0.85f), RoundedCornerShape(6.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
                )
            }
        }
        Column(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row {
                Text(stringResource(R.string.photo), fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.tx, modifier = Modifier.weight(1f))
                val bytes = if (img.full) img.data.size.toLong() else img.fullLen.toLong()
                Text(Formatter.formatShortFileSize(context, bytes), fontSize = 13.sp, color = t.tx2)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                TransportBadge(Transport.LORA, stringResource(if (img.full) R.string.lora_full_thumbnail else R.string.lora_preview_badge))
                if (!img.full && !m.outgoing) Text(stringResource(R.string.full_on_request), fontSize = 11.sp, color = t.tx2)
            }
            when {
                announce != null -> BridgeSection(model, announce, standalone = false)
                !m.outgoing && !img.full && img.fullLen.toInt() > 0 -> if (m.id in model.requestedFull) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Icon(Icons.Outlined.HourglassEmpty, null, tint = t.tx2, modifier = Modifier.size(14.dp))
                        Text(stringResource(R.string.full_requested), fontSize = 13.sp, color = t.tx2)
                    }
                } else {
                    val e = model.estimate(chat, 21u + img.fullLen.toUInt())
                    val label = e?.let {
                        stringResource(R.string.full_image_cost, fragmentsText(res, it.fragments.toInt()), "%.0f".format(it.airtimeMs.toDouble() / 1000))
                    } ?: stringResource(R.string.load_full_image)
                    SecondaryButton(label, background = t.loraBg, foreground = t.loraFg, enabled = model.isConnected) {
                        error = model.requestFull(m)
                    }
                }
            }
            error?.let { Text(it, fontSize = 12.sp, color = t.sosFg) }
        }
    }
}

private fun decodeFile(f: File): android.graphics.Bitmap? = runCatching { decodeImage(f.readBytes()) }.getOrNull()

/** Content announced over LoRa and fetched over the Internet bridge (§14, mockup `chat`). */
@Composable
private fun BridgeSection(model: AppModel, announce: StoredMessage, standalone: Boolean) {
    val t = lichen
    val context = LocalContext.current
    val a = announce.message as? Message.Announce ?: return
    val h = a.hash.hex()
    val file = model.downloads[h]
    // A file (ADR-0022), not a photo: its icon and name, opened once here.
    val isFile = a.kind.toInt() == 3
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (isFile) {
            Box(Modifier.clickable(enabled = file != null) { file?.let { openFile(context, a.name, file = it) } }) {
                FileRow(a.name, a.size.toLong())
            }
        } else if (standalone && file != null) {
            val bitmap = remember(file) { decodeFile(file)?.asImageBitmap() }
            var viewing by remember { mutableStateOf(false) }
            if (viewing) PhotoViewer(model, announce) { viewing = false }
            if (bitmap != null) {
                Image(
                    bitmap, a.name,
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { viewing = true },
                    contentScale = ContentScale.FillWidth,
                )
            }
        }
        if (!isFile) {
            Row {
                Text(a.name, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.tx, maxLines = 1, modifier = Modifier.weight(1f))
                Text(Formatter.formatShortFileSize(context, a.size.toLong()), fontSize = 13.sp, color = t.tx2)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (p in a.providers) {
                when (p.transport.toInt()) {
                    1 -> TransportBadge(Transport.IROH, "iroh")
                    2 -> TransportBadge(Transport.TOR, "Tor")
                }
            }
            if (file != null) {
                Icon(Icons.Outlined.Verified, null, tint = t.ok, modifier = Modifier.size(14.dp))
                Text(stringResource(R.string.hash_verified), fontSize = 11.sp, color = t.ok)
                if (isFile) {
                    Spacer(Modifier.weight(1f))
                    TextButton(onClick = { openFile(context, a.name, file = file) }) {
                        Text(stringResource(R.string.open), color = t.tx, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }
        model.fetchErrors[h]?.let { Text(it, fontSize = 12.sp, color = t.sosFg) }
        if (file == null && !announce.outgoing) {
            val via = model.fetching[h]
            if (via != null) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CircularProgressIndicator(Modifier.size(16.dp), color = t.tx2, strokeWidth = 2.dp)
                    Text(
                        stringResource(if (via == 2) R.string.downloading_tor else R.string.downloading_iroh),
                        fontSize = 13.sp, color = t.tx2, modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = { model.cancelFetch(announce) }) {
                        Text(stringResource(R.string.cancel), color = t.tx, fontWeight = FontWeight.SemiBold)
                    }
                }
            } else {
                val route = model.fetchRoute(a.providers)
                if (route.isEmpty()) {
                    // Offered, but nothing here reaches the sender.
                    Text(stringResource(if (isFile) R.string.file_unavailable else R.string.original_unavailable), fontSize = 13.sp, color = t.tx2)
                } else {
                    val torOnly = route.first().transport.toInt() == 2
                    SecondaryButton(
                        stringResource(
                            when {
                                model.fetchErrors[h] != null -> R.string.try_again
                                isFile -> R.string.download
                                else -> R.string.download_full
                            },
                        ),
                        background = if (torOnly) t.torBg else t.irohBg,
                        foreground = if (torOnly) t.torFg else t.irohFg,
                    ) { model.fetch(announce) }
                }
            }
        }
    }
}

/** While recording: cancel, time, cost so far, send. */
@Composable
fun RecordingBar(model: AppModel, chat: ChatId, onReview: (ShortArray) -> Unit, onError: (String) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    val seconds = model.recorder.elapsed
    // The chosen mode's bytes for what is recorded so far.
    val e = model.estimate(chat, voiceDatagramLenFor(model.voiceMode.toUByte(), (seconds * 1000).toUInt()))
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        IconButton(
            onClick = { model.recorder.stop(keep = false) },
            colors = IconButtonDefaults.iconButtonColors(containerColor = t.s2, contentColor = t.tx),
            modifier = Modifier.size(48.dp),
        ) { Icon(Icons.Filled.Close, stringResource(R.string.cancel)) }
        Box(Modifier.size(10.dp).background(t.sos, CircleShape))
        Column(Modifier.weight(1f)) {
            Text("${clock(seconds)} / ${clock(15.0)}", style = figure(15.sp), color = t.tx)
            e?.let {
                Text(
                    stringResource(R.string.cost_so_far, fragmentsText(res, it.fragments.toInt()), "%.0f".format(it.airtimeMs.toDouble() / 1000)),
                    fontSize = 11.sp, color = t.tx2,
                )
            }
        }
        IconButton(
            onClick = {
                // Stop and listen before sending.
                model.recorder.stop(keep = true)?.let(onReview)
            },
            colors = IconButtonDefaults.iconButtonColors(containerColor = t.s2, contentColor = t.tx),
            modifier = Modifier.size(48.dp),
        ) { Icon(Icons.Filled.Stop, stringResource(R.string.stop_and_listen)) }
        IconButton(
            onClick = {
                // To the conversation the recording was made in.
                val to = model.recorder.chat ?: return@IconButton
                val pcm = model.recorder.stop(keep = true) ?: return@IconButton
                model.sendVoice(com.tduhamel.lichen.core.prepareVoice(pcm.toList(), model.voiceMode.toUByte()), to)?.let(onError)
            },
            colors = IconButtonDefaults.iconButtonColors(containerColor = t.actBg, contentColor = t.actFg),
            modifier = Modifier.size(48.dp),
        ) { Icon(Icons.Filled.ArrowUpward, stringResource(R.string.send)) }
    }
}

/** A recording stopped and not sent yet. */
class VoiceReview(val pcm: ShortArray, val mode: Int, val voice: PreparedVoice)

/** A stopped recording: play what will be sent (encoded, then decoded: Codec2's real sound),
 * choose its quality against its airtime, drop it or send it. */
@Composable
fun ReviewBar(model: AppModel, chat: ChatId, r: VoiceReview, change: (VoiceReview?) -> Unit, onError: (String) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    val reviewing = model.player.playing == VoicePlayer.REVIEW
    val total = r.voice.durationMs.toDouble() / 1000
    val cost = model.estimate(chat, voiceDatagramLenFor(r.mode.toUByte(), r.voice.durationMs))
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            IconButton(
                onClick = {
                    model.player.stop()
                    change(null)
                },
                colors = IconButtonDefaults.iconButtonColors(containerColor = t.s2, contentColor = t.sos),
                modifier = Modifier.size(48.dp),
            ) { Icon(Icons.Outlined.Delete, stringResource(R.string.discard_recording)) }
            IconButton(
                onClick = {
                    if (reviewing) {
                        model.player.stop()
                    } else {
                        decodeVoice(r.voice.mode, r.voice.frames, r.voice.data)?.let {
                            model.player.play(it.toShortArray(), VoicePlayer.REVIEW)
                        }
                    }
                },
                colors = IconButtonDefaults.iconButtonColors(containerColor = t.lora, contentColor = Color(0xFF141413)),
                modifier = Modifier.size(48.dp),
            ) {
                Icon(
                    if (reviewing) Icons.Filled.Stop else Icons.Filled.PlayArrow,
                    stringResource(if (reviewing) R.string.stop else R.string.listen_before_sending),
                )
            }
            Text(
                if (reviewing) "${clock(model.player.position)} / ${clock(total)}" else clock(total),
                style = figure(15.sp), color = t.tx, modifier = Modifier.weight(1f),
            )
            IconButton(
                onClick = {
                    model.player.stop()
                    val e = model.sendVoice(r.voice, chat)
                    if (e != null) onError(e) else change(null)
                },
                colors = IconButtonDefaults.iconButtonColors(containerColor = t.actBg, contentColor = t.actFg),
                modifier = Modifier.size(48.dp),
            ) { Icon(Icons.Filled.ArrowUpward, stringResource(R.string.send)) }
        }
        // Voice quality against time on air, for this message.
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            val options = listOf(0 to R.string.voice_compact, 2 to R.string.voice_standard, 3 to R.string.voice_clear)
            options.forEachIndexed { i, (mode, label) ->
                SegmentedButton(
                    selected = r.mode == mode,
                    onClick = {
                        model.player.stop()
                        change(VoiceReview(r.pcm, mode, com.tduhamel.lichen.core.prepareVoice(r.pcm.toList(), mode.toUByte())))
                    },
                    shape = SegmentedButtonDefaults.itemShape(i, options.size),
                    colors = segmentedColors(),
                ) { Text(stringResource(label)) }
            }
        }
        cost?.let {
            Text(
                stringResource(R.string.voice_quality_cost, fragmentsText(res, it.fragments.toInt()), "%.0f".format(it.airtimeMs.toDouble() / 1000)),
                fontSize = 11.sp, color = t.tx2,
            )
        }
    }
}

private enum class PhotoMode { LORA, IROH, TOR }

/** How to send a photo, and what each way costs (mockup `media`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendPhotoSheet(model: AppModel, chat: ChatId, image: PreparedImage, original: ByteArray, done: (String?) -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val res = LocalResources.current
    var mode by remember { mutableStateOf<PhotoMode?>(null) }
    // Why the last try failed: the sheet and its photo stay (recheck R05).
    var failure by remember { mutableStateOf<String?>(null) }
    val irohOK = model.bridgeOnline
    val torOK = model.torStatus == AppModel.TorStatus.READY
    // LoRa previews go to contacts only; groups get the announcement (§14.1).
    val chosen = mode ?: when {
        !chat.group -> PhotoMode.LORA
        irohOK || !torOK -> PhotoMode.IROH
        else -> PhotoMode.TOR
    }
    val sendable = when (chosen) {
        PhotoMode.LORA -> !chat.group
        PhotoMode.IROH -> irohOK
        PhotoMode.TOR -> torOK
    }
    val size = Formatter.formatShortFileSize(context, original.size.toLong())
    val estimate = model.estimate(chat, imageDatagramLen(image))
    val cost = estimate?.let { stringResource(R.string.approx_seconds, "%.0f".format(maxOf(1.0, it.airtimeMs.toDouble() / 1000))) } ?: "LoRa"
    val preview = remember(image) { decodeImage(image.full)?.asImageBitmap() }
    val irohDetail = when {
        irohOK -> stringResource(R.string.iroh_detail, size)
        !model.irohEnabled -> stringResource(R.string.iroh_off_in_settings)
        else -> stringResource(R.string.needs_internet)
    }
    val torDetail = when (model.torStatus) {
        AppModel.TorStatus.READY -> stringResource(R.string.tor_detail, size)
        AppModel.TorStatus.CONNECTING, AppModel.TorStatus.PUBLISHING -> stringResource(R.string.connecting_tor)
        AppModel.TorStatus.FAILED -> stringResource(R.string.tor_failed)
        AppModel.TorStatus.OFF -> stringResource(R.string.turn_tor_on)
    }
    ModalBottomSheet(onDismissRequest = { done(null) }, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.send_a_photo), fontSize = 22.sp, color = t.tx)
            Box(Modifier.fillMaxWidth().height(230.dp).clip(CardShape).background(t.s2), contentAlignment = Alignment.BottomStart) {
                if (preview != null) Image(preview, null, Modifier.fillMaxWidth().height(230.dp), contentScale = ContentScale.Crop)
                Text(
                    "${image.width}×${image.height} AVIF · preview ${image.preview.size} B · full ${image.full.size} B",
                    fontSize = 12.sp, color = t.tx,
                    modifier = Modifier.padding(12.dp).background(t.bg, RoundedCornerShape(6.dp)).padding(horizontal = 8.dp, vertical = 4.dp),
                )
            }
            SectionLabel(stringResource(R.string.how_to_send, model.title(chat)))
            val previewFragments = estimate?.let { fragmentsText(res, it.fragments.toInt()) } ?: fragmentsText(res, 3)
            PhotoOption(
                stringResource(R.string.lora_preview),
                if (chat.group) stringResource(R.string.contacts_only_never_flooded) else stringResource(R.string.lora_preview_detail, previewFragments),
                cost, Transport.LORA, chosen == PhotoMode.LORA, !chat.group,
            ) { mode = PhotoMode.LORA }
            PhotoOption(stringResource(R.string.full_via_iroh), irohDetail, stringResource(R.string.one_packet), Transport.IROH, chosen == PhotoMode.IROH, irohOK) {
                mode = PhotoMode.IROH
            }
            PhotoOption(stringResource(R.string.full_via_tor), torDetail, stringResource(R.string.one_packet), Transport.TOR, chosen == PhotoMode.TOR, torOK) {
                mode = PhotoMode.TOR
            }
            Text(stringResource(R.string.airtime_left_images, (model.airtimeLeftMs / 1000u).toLong()), fontSize = 13.sp, color = t.tx2)
            failure?.let { Banner(BannerKind.SOS, it) }
            PrimaryButton(
                stringResource(if (chosen == PhotoMode.LORA) R.string.send_preview else R.string.send_announcement),
                Modifier.padding(top = 8.dp),
                enabled = sendable,
            ) {
                val e = if (chosen == PhotoMode.LORA) {
                    model.sendImage(image, chat)
                } else {
                    model.sendOverBridge(original, com.tduhamel.lichen.model.photoName(original), chat, anonymous = chosen == PhotoMode.TOR)
                }
                if (e != null) failure = e else done(null)
            }
        }
    }
}

@Composable
private fun PhotoOption(
    title: String,
    detail: String,
    badge: String,
    kind: Transport,
    selected: Boolean,
    enabled: Boolean,
    select: () -> Unit,
) {
    val t = lichen
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) t.s2 else t.s1)
            .clickable(enabled = enabled, onClick = select)
            .padding(horizontal = 14.dp, vertical = 12.dp)
            .then(if (enabled) Modifier else Modifier.background(Color.Transparent)),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(
            Modifier.size(22.dp).border(2.dp, if (selected) t.tx else t.line, CircleShape),
            contentAlignment = Alignment.Center,
        ) { if (selected) Box(Modifier.size(10.dp).background(t.tx, CircleShape)) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = if (enabled) t.tx else t.tx2)
            Text(detail, fontSize = 13.sp, color = t.tx2)
        }
        TransportBadge(kind, badge)
    }
}
