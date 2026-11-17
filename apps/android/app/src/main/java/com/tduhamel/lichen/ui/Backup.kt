package com.tduhamel.lichen.ui

import android.text.format.Formatter
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.FileOpen
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ModalBottomSheetProperties
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.SecureFlagPolicy
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import kotlinx.coroutines.launch

/**
 * The encrypted backup (ADR-0037): a row that asks where to save the file (the Storage Access
 * Framework's "create document"), makes the backup there, and says what it holds.
 */
@Composable
fun BackupExportRow(model: AppModel, title: String, subtitle: String) {
    val t = lichen
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var working by remember { mutableStateOf(false) }
    var outcome by remember { mutableStateOf<AppModel.BackupOutcome?>(null) }
    val create = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        working = true
        scope.launch {
            outcome = model.exportBackup(uri)
            working = false
        }
    }
    Row(
        Modifier.fillMaxWidth().clickable(enabled = !working) { create.launch(model.backupFileName()) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) { CardRow(title, subtitle) }
        if (working) {
            CircularProgressIndicator(Modifier.padding(end = 16.dp).size(20.dp), color = t.tx2, strokeWidth = 2.dp)
        } else {
            Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
        }
    }
    outcome?.let { o ->
        val text = when (o) {
            is AppModel.BackupOutcome.Made -> stringResource(
                R.string.backup_made,
                o.summary.messages.toInt(),
                o.summary.files.toInt(),
                Formatter.formatShortFileSize(context, o.summary.bytes.toLong()),
            )
            is AppModel.BackupOutcome.Failed -> o.reason
        }
        AlertDialog(
            onDismissRequest = { outcome = null },
            title = {
                Text(stringResource(if (o is AppModel.BackupOutcome.Made) R.string.backup_saved else R.string.backup_failed))
            },
            text = { Text(text) },
            confirmButton = { TextButton(onClick = { outcome = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** "Restore a backup" (ADR-0037): the file, then its 24 words; the backup's identity and data
 * replace this phone's. A secure window: the words are as secret as the ones shown. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RestoreBackupSheet(model: AppModel, dismiss: () -> Unit, restored: () -> Unit = {}) {
    val t = lichen
    val scope = rememberCoroutineScope()
    var file by remember { mutableStateOf<android.net.Uri?>(null) }
    var text by remember { mutableStateOf("") }
    var working by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val words = text.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) file = uri
    }
    ModalBottomSheet(
        onDismissRequest = { if (!working) dismiss() },
        sheetState = rememberModalBottomSheetState(true),
        containerColor = t.bg,
        properties = ModalBottomSheetProperties(securePolicy = SecureFlagPolicy.SecureOn),
    ) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(stringResource(R.string.restore_backup), fontSize = 22.sp, color = t.tx)
            Text(stringResource(R.string.restore_backup_intro), fontSize = 14.sp, color = t.tx2)
            CardList {
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = !working) { pick.launch(arrayOf("*/*")) },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        CardRow(
                            file?.lastPathSegment?.substringAfterLast('/') ?: stringResource(R.string.choose_backup_file),
                            if (file == null) stringResource(R.string.choose_backup_file_note) else null,
                        )
                    }
                    Icon(
                        // The open-file icon is decorative (the row says it); the tick says "chosen".
                        if (file == null) Icons.Outlined.FileOpen else Icons.Outlined.CheckCircle,
                        if (file == null) null else stringResource(R.string.file_chosen),
                        tint = if (file == null) t.tx2 else t.ok, modifier = Modifier.padding(end = 16.dp),
                    )
                }
            }
            Text(stringResource(R.string.backup_words), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            OutlinedTextField(
                text, { text = it },
                Modifier.fillMaxWidth().heightIn(min = 140.dp),
                enabled = !working,
                textStyle = androidx.compose.ui.text.TextStyle(fontSize = 16.sp, fontFamily = FontFamily.Monospace, color = t.tx),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                shape = RoundedCornerShape(12.dp),
                colors = fieldColors(),
            )
            Text(
                stringResource(R.string.words_count, words.size), style = figure(13.sp).copy(fontWeight = FontWeight.Normal),
                color = if (words.size == 24) t.ok else t.tx2,
            )
            error?.let { Banner(BannerKind.SOS, it) }
            PrimaryButton(
                stringResource(if (working) R.string.restoring else R.string.restore),
                enabled = file != null && words.size == 24 && !working,
            ) {
                val uri = file ?: return@PrimaryButton
                working = true
                error = null
                scope.launch {
                    error = model.restoreBackup(uri, words)
                    working = false
                    if (error == null) {
                        dismiss()
                        restored()
                    }
                }
            }
        }
    }
}
