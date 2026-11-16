package com.tduhamel.lichen.ui

import android.text.format.DateUtils
import android.text.format.Formatter
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.decodeImage
import com.tduhamel.lichen.model.hex

/** What a media message is, for Chat info › Media, files and voice (ADR-0034). */
private enum class MediaKind { PHOTOS, FILES, VOICE }

private fun kindOf(m: StoredMessage): MediaKind? = when (val msg = m.message) {
    is Message.Image -> MediaKind.PHOTOS
    is Message.File -> MediaKind.FILES
    is Message.Voice -> MediaKind.VOICE
    is Message.Announce -> when (msg.kind.toInt()) {
        1 -> MediaKind.PHOTOS
        2 -> MediaKind.VOICE
        else -> MediaKind.FILES
    }
    else -> null
}

/** Chat info › Media, files and voice (ADR-0034): what a conversation holds, the newest first.
 * Photos in a grid (the best version on this phone), files and voice messages in lists. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatMediaScreen(model: AppModel, chat: ChatId, back: () -> Unit) {
    val t = lichen
    var kind by remember { mutableStateOf(MediaKind.PHOTOS) }
    val items = remember(chat) { model.chatMedia(chat) }
    var viewing by remember { mutableStateOf<StoredMessage?>(null) }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.media_files_voice), fontSize = 20.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
            val choices = listOf(MediaKind.PHOTOS to R.string.photos, MediaKind.FILES to R.string.files, MediaKind.VOICE to R.string.voice)
            choices.forEachIndexed { i, (k, label) ->
                SegmentedButton(
                    selected = kind == k,
                    onClick = { kind = k },
                    shape = SegmentedButtonDefaults.itemShape(i, choices.size),
                ) { Text(stringResource(label)) }
            }
        }
        val shown = items.filter { kindOf(it) == kind }
        if (shown.isEmpty()) {
            Text(
                stringResource(
                    when (kind) {
                        MediaKind.PHOTOS -> R.string.no_photos_here
                        MediaKind.FILES -> R.string.no_files_here
                        MediaKind.VOICE -> R.string.no_voice_here
                    },
                ),
                color = t.tx2, modifier = Modifier.padding(24.dp).align(Alignment.CenterHorizontally),
            )
        } else if (kind == MediaKind.PHOTOS) {
            LazyVerticalGrid(
                GridCells.Adaptive(104.dp),
                Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
                contentPadding = PaddingValues(4.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                items(shown, key = { it.id }) { m -> Thumbnail(model, m) { viewing = m } }
            }
        } else {
            LazyColumn(
                Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
                contentPadding = PaddingValues(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(shown, key = { it.id }) { m ->
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(
                            DateUtils.formatDateTime(LocalContext.current, m.atMs, DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_TIME),
                            fontSize = 12.sp, color = t.tx2,
                        )
                        MediaContent(model, m)
                    }
                }
            }
        }
    }
    viewing?.let { PhotoViewer(model, it) { viewing = null } }
}

/** A photo's square thumbnail: a download over the bridge, else the LoRa image. */
@Composable
private fun Thumbnail(model: AppModel, m: StoredMessage, open: () -> Unit) {
    val t = lichen
    val msg = m.message
    val announce = when (msg) {
        is Message.Image -> model.announcement(msg.mediaId, ChatId.of(m.chat))?.message as? Message.Announce
        is Message.Announce -> msg
        else -> null
    }
    val fetched = announce?.let { model.downloads[it.hash.hex()] }
    val bitmap = rememberInBackground(m.id, fetched) {
        (fetched?.let { f -> runCatching { decodeImage(f.readBytes(), 480) }.getOrNull() } ?: (msg as? Message.Image)?.let { decodeImage(it.data, 240) })
            ?.asImageBitmap()
    }
    Box(
        Modifier.aspectRatio(1f).clip(RoundedCornerShape(4.dp)).background(t.s2).clickable(onClickLabel = stringResource(R.string.open_photo), onClick = open),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(bitmap, stringResource(R.string.photo), Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        } else {
            Icon(Icons.Outlined.Image, stringResource(R.string.photo), tint = t.tx2)
        }
    }
}

/** Settings › Storage (ADR-0034): what each conversation takes on this phone, and media older
 * than a month, three months or a year removed as "delete for me" removes them. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StorageScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    var version by remember { mutableStateOf(0) }
    val usage = remember(version) { model.storageUsage().sortedByDescending { it.second } }
    var clearing by remember { mutableStateOf<Int?>(null) }
    var cleared by remember { mutableStateOf<Int?>(null) }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.storage), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                SectionLabel(
                    stringResource(R.string.storage_in_all, Formatter.formatShortFileSize(context, usage.sumOf { it.second })),
                    Modifier.padding(top = 10.dp),
                )
            }
            item {
                CardList {
                    if (usage.isEmpty()) CardRow(stringResource(R.string.no_conversations_yet))
                    usage.forEachIndexed { i, (u, bytes) ->
                        if (i > 0) CardDivider()
                        val chat = ChatId.of(u.chat)
                        CardRow(
                            model.title(chat),
                            pluralStringResource(R.plurals.messages_media, u.messages.toInt(), u.messages.toInt(), u.media.toInt()),
                        ) { Text(Formatter.formatShortFileSize(context, bytes), fontSize = 15.sp, color = t.tx2) }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.clear_media), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    listOf(30, 90, 365).forEachIndexed { i, days ->
                        if (i > 0) CardDivider()
                        Box(Modifier.clickable { clearing = days }) {
                            CardRow(olderThan(days)) {}
                        }
                    }
                }
            }
            item {
                Text(stringResource(R.string.clear_media_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
        }
    }
    clearing?.let { days ->
        AlertDialog(
            onDismissRequest = { clearing = null },
            title = { Text(olderThan(days)) },
            text = { Text(stringResource(R.string.cannot_be_undone)) },
            confirmButton = {
                TextButton(onClick = {
                    clearing = null
                    cleared = model.clearMedia(days)
                    version++
                }) { Text(stringResource(R.string.remove), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { clearing = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
    cleared?.let { n ->
        AlertDialog(
            onDismissRequest = { cleared = null },
            text = { Text(pluralStringResource(R.plurals.media_removed, n, n)) },
            confirmButton = { TextButton(onClick = { cleared = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun olderThan(days: Int): String = stringResource(
    when (days) {
        30 -> R.string.media_older_30
        90 -> R.string.media_older_90
        else -> R.string.media_older_365
    },
)
