package com.tduhamel.lichen.ui

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.text.format.Formatter
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.FileProvider
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.decodeImage
import com.tduhamel.lichen.model.hex
import java.io.File

/** What the viewer shows: the best version on this phone. */
private class Shown(val bitmap: Bitmap, val caption: String, val file: File?)

/** A photo full screen: the best version on this phone (the original over the bridge, else the
 * full thumbnail, else the LoRa preview), zoomable, with what it is, how to get a better one,
 * and Share. `m` is an image, or an announcement without a LoRa preview. */
@Composable
fun PhotoViewer(model: AppModel, m: StoredMessage, close: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val chat = ChatId.of(m.chat)
    val announce = when (val msg = m.message) {
        is Message.Image -> model.announcement(msg.mediaId, chat)
        is Message.Announce -> m
        else -> null
    }
    val a = announce?.message as? Message.Announce
    val original = a?.let { model.downloads[it.hash.hex()] }
    val shown = remember(m.id, m.message, original) { best(context, m, a, original) }
    var error by remember { mutableStateOf<String?>(null) }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var drag by remember { mutableFloatStateOf(0f) }
    Dialog(close, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            shown?.let { s ->
                Image(
                    s.bitmap.asImageBitmap(),
                    stringResource(R.string.photo),
                    Modifier
                        .fillMaxSize()
                        .pointerInput(Unit) {
                            detectTapGestures(onDoubleTap = {
                                scale = if (scale > 1f) 1f else 2.5f
                                offset = Offset.Zero
                            })
                        }
                        .pointerInput(Unit) {
                            detectTransformGestures { _, pan, zoom, _ ->
                                scale = (scale * zoom).coerceIn(1f, 6f)
                                offset = if (scale > 1f) offset + pan else Offset.Zero
                            }
                        }
                        // Not zoomed, a drag down closes it.
                        .then(
                            if (scale <= 1f) {
                                Modifier.pointerInput(Unit) {
                                    detectVerticalDragGestures(
                                        onDragEnd = { if (drag > 300f) close() else drag = 0f },
                                        onDragCancel = { drag = 0f },
                                    ) { _, dy -> drag = (drag + dy).coerceAtLeast(0f) }
                                }
                            } else {
                                Modifier
                            },
                        )
                        .graphicsLayer {
                            scaleX = scale
                            scaleY = scale
                            translationX = offset.x
                            translationY = offset.y + drag
                        },
                    contentScale = ContentScale.Fit,
                )
            }
            Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val round = IconButtonDefaults.iconButtonColors(containerColor = Color.Black.copy(alpha = 0.5f), contentColor = Color.White)
                    IconButton(onClick = close, colors = round) { Icon(Icons.Filled.Close, stringResource(R.string.close)) }
                    Spacer(Modifier.weight(1f))
                    if (shown != null) {
                        IconButton(onClick = { share(context, shown) }, colors = round) {
                            Icon(Icons.Outlined.Share, stringResource(R.string.share))
                        }
                    }
                }
                Spacer(Modifier.weight(1f))
                Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    shown?.let { Text(it.caption, fontSize = 13.sp, color = Color.White.copy(alpha = 0.85f), textAlign = TextAlign.Center) }
                    when {
                        a != null && !announce.outgoing && original == null -> {
                            if (model.fetching[a.hash.hex()] != null) {
                                CircularProgressIndicator(Modifier.size(20.dp), color = Color.White, strokeWidth = 2.dp)
                            } else if (model.fetchRoute(a.providers).isNotEmpty()) {
                                SecondaryButton(stringResource(R.string.download_full), background = t.irohBg, foreground = t.irohFg) {
                                    model.fetch(announce)
                                }
                            }
                        }
                        a == null && m.message is Message.Image && !(m.message as Message.Image).full &&
                            !m.outgoing && (m.message as Message.Image).fullLen.toInt() > 0 -> {
                            if (m.id in model.requestedFull) {
                                Text(stringResource(R.string.full_requested), fontSize = 13.sp, color = Color.White.copy(alpha = 0.85f))
                            } else {
                                SecondaryButton(
                                    stringResource(R.string.load_full_image),
                                    background = t.loraBg, foreground = t.loraFg, enabled = model.isConnected,
                                ) { error = model.requestFull(m) }
                            }
                        }
                    }
                    error?.let { Text(it, fontSize = 13.sp, color = t.sos) }
                }
            }
        }
    }
}

private fun best(context: Context, m: StoredMessage, a: Message.Announce?, original: File?): Shown? {
    if (a != null && original != null) {
        runCatching { decodeImage(original.readBytes()) }.getOrNull()?.let { bmp ->
            val via = if (a.providers.any { it.transport.toInt() == 1 }) "iroh" else "Tor"
            val size = Formatter.formatShortFileSize(context, a.size.toLong())
            return Shown(bmp, context.getString(R.string.photo_original, size, via), original)
        }
    }
    val img = m.message as? Message.Image ?: return null
    val bmp = decodeImage(img.data) ?: return null
    val size = Formatter.formatShortFileSize(context, img.data.size.toLong())
    return Shown(bmp, context.getString(if (img.full) R.string.photo_full_thumbnail else R.string.photo_preview, size), null)
}

/** Shares the original as it is, else the thumbnail as a JPEG (most apps do not open AVIF). */
private fun share(context: Context, s: Shown) {
    // A folder of its own per share: an earlier share keeps its own bytes (recheck R06).
    val file = if (s.file != null) {
        uniqueShareFile(context, s.file.name)?.also { s.file.copyTo(it) } ?: return
    } else {
        uniqueShareFile(context, "lichen-photo.jpg")?.also { f ->
            f.outputStream().use { s.bitmap.compress(Bitmap.CompressFormat.JPEG, 90, it) }
        } ?: return
    }
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND)
        .setType(if (s.file != null) "image/*" else "image/jpeg")
        .putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, null))
}
