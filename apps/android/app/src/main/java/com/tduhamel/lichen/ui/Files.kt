package com.tduhamel.lichen.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.text.format.Formatter
import android.webkit.MimeTypeMap
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Audiotrack
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.PictureAsPdf
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.fitFileName
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.fragmentsText
import java.io.File
import java.util.UUID

/** The most a file can be: what the Internet bridge carries (§14.1, 64 MiB). */
const val FILE_MAX = 64 shl 20

/** [readPicked]'s answer for a file over [FILE_MAX]. */
val TOO_LARGE = FileDraft("", ByteArray(0))

/**
 * A picked document, read only when it is at most [FILE_MAX] (recheck R04): the size the
 * provider reports is checked first, and reading stops one byte past the limit whatever it said.
 * Null when it cannot be read or is empty, [TOO_LARGE] when too large. Call off the main thread.
 */
fun readPicked(context: Context, uri: android.net.Uri): FileDraft? = runCatching {
    val cr = context.contentResolver
    var name: String? = null
    var size: Long? = null
    cr.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME, android.provider.OpenableColumns.SIZE), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
            name = c.getString(0)
            if (!c.isNull(1)) size = c.getLong(1)
        }
    }
    if ((size ?: 0) > FILE_MAX) return TOO_LARGE
    val data = cr.openInputStream(uri)?.use { input ->
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (out.size() <= FILE_MAX) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
        }
        if (out.size() > FILE_MAX) return TOO_LARGE
        out.toByteArray()
    } ?: return null
    if (data.isEmpty()) null else FileDraft(name ?: uri.lastPathSegment ?: "file", data)
}.getOrNull()

/** A file picked in the composer. */
class FileDraft(val name: String, val data: ByteArray)

/** An icon for a file, by its extension. */
fun fileIcon(name: String): ImageVector = when (name.substringAfterLast('.', "").lowercase()) {
    "pdf" -> Icons.Outlined.PictureAsPdf
    "txt", "md", "csv", "json" -> Icons.Outlined.Description
    "gpx", "kml", "kmz", "geojson" -> Icons.Outlined.Map
    "jpg", "jpeg", "png", "heic", "avif", "gif" -> Icons.Outlined.Image
    "zip", "gz", "7z", "tar" -> Icons.Outlined.FolderZip
    "mp3", "m4a", "wav", "ogg" -> Icons.Outlined.Audiotrack
    else -> Icons.AutoMirrored.Outlined.InsertDriveFile
}

/**
 * A new file for `name` in a folder of its own under the shared cache folder (the
 * FileProvider's). A received name is only a label, and a copy another app was given access to is
 * never overwritten by another file (recheck R01, R06). [clearOldShares] removes them later.
 */
fun uniqueShareFile(context: Context, name: String): File? {
    val dir = File(File(context.cacheDir, "shared"), UUID.randomUUID().toString())
    if (!dir.mkdirs()) return null
    val out = File(dir, fitFileName(name, 120u).ifEmpty { "file" })
    return out.takeIf { it.canonicalFile.parentFile == dir.canonicalFile }
}

/** Copies made to open or share files more than a day ago: the apps given them are done. */
fun clearOldShares(context: Context) {
    val cutoff = System.currentTimeMillis() - 24 * 3_600_000L
    File(context.cacheDir, "shared").listFiles()?.filter { it.lastModified() < cutoff }?.forEach { it.deleteRecursively() }
}

/** A copy of a file under its own name, in a folder of its own (the FileProvider's). */
private fun sharedCopy(context: Context, name: String, data: ByteArray? = null, file: File? = null): File? {
    val out = uniqueShareFile(context, name) ?: return null
    return runCatching {
        when {
            data != null -> out.writeBytes(data)
            file != null -> file.copyTo(out)
            else -> return null
        }
        out
    }.getOrNull()
}

/**
 * Opens a file with the app for its type (by its extension); with none installed, offers to
 * share it instead. lichen never runs a file itself (ADR-0022).
 */
fun openFile(context: Context, name: String, data: ByteArray? = null, file: File? = null) {
    val copy = sharedCopy(context, name, data, file) ?: return
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", copy)
    val type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
        ?: "application/octet-stream"
    val view = Intent(Intent.ACTION_VIEW).setDataAndType(uri, type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    try {
        context.startActivity(view)
    } catch (_: ActivityNotFoundException) {
        val send = Intent(Intent.ACTION_SEND).setType(type).putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        context.startActivity(Intent.createChooser(send, context.getString(R.string.no_app_to_open)))
    }
}

/** Icon, name and size of a file, with a transport badge. */
@Composable
fun FileRow(name: String, size: Long, badge: @Composable () -> Unit = {}) {
    val t = lichen
    val context = LocalContext.current
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.size(40.dp).background(t.s2, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
            Icon(fileIcon(name), null, tint = t.tx, modifier = Modifier.size(22.dp))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(name, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.tx, maxLines = 2)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(Formatter.formatShortFileSize(context, size), fontSize = 12.sp, color = t.tx2)
                badge()
            }
        }
    }
}

/** A small file that came (or went) over LoRa: tap to open it. */
@Composable
fun FileCard(f: Message.File) {
    val t = lichen
    val context = LocalContext.current
    Box(
        Modifier
            .width(250.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(t.s1)
            .border(1.dp, t.line, RoundedCornerShape(18.dp))
            .clickable(onClickLabel = stringResource(R.string.open)) { openFile(context, f.name, data = f.data) }
            .padding(12.dp),
    ) {
        FileRow(f.name, f.data.size.toLong()) { TransportBadge(Transport.LORA, "LoRa") }
    }
}

private enum class FileRoute { LORA, IROH, TOR }

/**
 * How to send a picked file (ADR-0022): over LoRa when small (up to 8 KB on the air once
 * compressed, ADR-0023; contacts only, its airtime shown), else over iroh or Tor; why not, when
 * neither can.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendFileSheet(model: AppModel, chat: ChatId, draft: FileDraft, done: (String?) -> Unit) {
    val t = lichen
    // iroh runs while the sheet is open (SR-29).
    DisposableEffect(Unit) {
        model.needBridge()
        onDispose { model.releaseBridge() }
    }
    val context = LocalContext.current
    val res = LocalResources.current
    var mode by remember { mutableStateOf<FileRoute?>(null) }
    // Why the last try failed: the sheet and its file stay (recheck R05).
    var failure by remember { mutableStateOf<String?>(null) }
    val size = Formatter.formatShortFileSize(context, draft.data.size.toLong())
    // The file as it would go over LoRa, compressed when smaller; null when it does not fit.
    val lora = remember(draft) { model.loraFile(chat, draft.name, draft.data) }
    val loraOK = lora != null
    val irohOK = model.bridgeOnline
    val torOK = model.torStatus == AppModel.TorStatus.READY
    // The route picked, as long as it is still ready (recheck R05); else the best one ready.
    val chosen = when {
        mode == FileRoute.LORA && loraOK -> FileRoute.LORA
        mode == FileRoute.IROH && irohOK -> FileRoute.IROH
        mode == FileRoute.TOR && torOK -> FileRoute.TOR
        loraOK -> FileRoute.LORA
        irohOK -> FileRoute.IROH
        torOK -> FileRoute.TOR
        else -> null
    }
    val loraDetail = when {
        chat.group -> stringResource(R.string.contacts_only_never_flooded)
        lora == null -> stringResource(R.string.too_large_for_lora, size)
        lora.compressed -> stringResource(
            R.string.lora_file_compressed,
            Formatter.formatShortFileSize(context, lora.wireBytes.toLong()),
            fragmentsText(res, lora.fragments.toInt()),
            "%.0f".format(lora.airtimeMs.toDouble() / 1000),
            "%.0f".format(lora.airtimeMs.toDouble() / 3600),
        )
        else -> stringResource(
            R.string.lora_file_detail,
            fragmentsText(res, lora.fragments.toInt()),
            "%.0f".format(lora.airtimeMs.toDouble() / 1000),
            "%.0f".format(lora.airtimeMs.toDouble() / 3600),
        )
    }
    val irohDetail = when {
        irohOK -> stringResource(R.string.iroh_file_detail)
        !model.irohEnabled -> stringResource(R.string.iroh_off_in_settings)
        else -> stringResource(R.string.needs_internet)
    }
    val torDetail = when (model.torStatus) {
        AppModel.TorStatus.READY -> stringResource(R.string.tor_file_detail)
        AppModel.TorStatus.CONNECTING, AppModel.TorStatus.PUBLISHING -> stringResource(R.string.connecting_tor)
        AppModel.TorStatus.FAILED -> stringResource(R.string.tor_failed)
        AppModel.TorStatus.OFF -> stringResource(R.string.turn_tor_on)
    }
    ModalBottomSheet(onDismissRequest = { done(null) }, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.send_a_file), fontSize = 22.sp, color = t.tx)
            FileRow(draft.name, draft.data.size.toLong())
            SectionLabel(stringResource(R.string.how_to_send, model.title(chat)))
            val loraBadge = lora?.let { stringResource(R.string.approx_seconds, "%.0f".format(maxOf(1.0, it.airtimeMs.toDouble() / 1000))) } ?: "LoRa"
            RouteOption(stringResource(R.string.over_lora), loraDetail, loraBadge, Transport.LORA, chosen == FileRoute.LORA, loraOK) {
                mode = FileRoute.LORA
            }
            RouteOption(stringResource(R.string.via_iroh), irohDetail, stringResource(R.string.one_packet), Transport.IROH, chosen == FileRoute.IROH, irohOK) {
                mode = FileRoute.IROH
            }
            RouteOption(stringResource(R.string.via_tor), torDetail, stringResource(R.string.one_packet), Transport.TOR, chosen == FileRoute.TOR, torOK) {
                mode = FileRoute.TOR
            }
            if (chosen == null) Banner(BannerKind.WARN, stringResource(R.string.file_no_lora_title), stringResource(R.string.file_no_lora_note))
            Text(stringResource(R.string.airtime_left_files, (model.airtimeLeftMs / 1000u).toLong()), fontSize = 13.sp, color = t.tx2)
            failure?.let { Banner(BannerKind.SOS, it) }
            PrimaryButton(
                stringResource(if (chosen == FileRoute.LORA) R.string.send_over_lora else R.string.send_announcement),
                Modifier.padding(top = 8.dp),
                enabled = chosen != null,
            ) {
                val e = when (chosen) {
                    FileRoute.LORA -> model.sendFile(draft.data, draft.name, chat)
                    FileRoute.IROH, FileRoute.TOR ->
                        model.sendOverBridge(draft.data, draft.name, chat, anonymous = chosen == FileRoute.TOR, kind = 3u)
                    null -> return@PrimaryButton
                }
                if (e != null) failure = e else done(null)
            }
        }
    }
}

@Composable
private fun RouteOption(
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
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(
            Modifier.size(22.dp).border(2.dp, if (selected) t.tx else t.line, androidx.compose.foundation.shape.CircleShape),
            contentAlignment = Alignment.Center,
        ) { if (selected) Box(Modifier.size(10.dp).background(t.tx, androidx.compose.foundation.shape.CircleShape)) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = if (enabled) t.tx else t.tx2)
            Text(detail, fontSize = 13.sp, color = t.tx2)
        }
        TransportBadge(kind, badge)
    }
}
