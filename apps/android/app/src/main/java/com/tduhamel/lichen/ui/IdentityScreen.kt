package com.tduhamel.lichen.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.WindowManager
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.Spacer
import androidx.compose.material3.TextButton
import androidx.compose.material3.AlertDialog
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ModalBottomSheetProperties
import androidx.compose.ui.window.SecureFlagPolicy
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel

/** "Your identity" (mockup `identite`, ADR-0013): name, fingerprint, the 24-word backup, restore. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IdentityScreen(model: AppModel, back: () -> Unit, next: (() -> Unit)? = null) {
    val t = lichen
    var showingPhrase by remember { mutableStateOf(false) }
    var restoring by remember { mutableStateOf(false) }
    var restoringBackup by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.your_identity), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Avatar(model.myName.ifEmpty { model.nodeName }, model.core.key() ?: ByteArray(0), 76.dp)
            OutlinedTextField(
                model.myName, model::changeName,
                Modifier.widthIn(max = 280.dp),
                singleLine = true,
                placeholder = { Text(stringResource(R.string.your_name), Modifier.fillMaxWidth(), textAlign = TextAlign.Center) },
                textStyle = androidx.compose.ui.text.TextStyle(fontSize = 18.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center, color = t.tx),
                shape = RoundedCornerShape(12.dp),
                colors = fieldColors(),
            )
            Column(
                Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(Icons.Outlined.Key, null, tint = t.ok, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.key_fingerprint), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                }
                Groups(model.fingerprint, Modifier.fillMaxWidth())
                Text(stringResource(R.string.key_fingerprint_note), fontSize = 13.sp, color = t.tx2)
            }
            if (!model.phraseBackedUp) {
                Banner(BannerKind.WARN, stringResource(R.string.not_backed_up), stringResource(R.string.not_backed_up_note))
            }
            CardList {
                ChevronRow(stringResource(R.string.recovery_phrase), stringResource(R.string.recovery_phrase_note)) { showingPhrase = true }
                CardDivider()
                ChevronRow(stringResource(R.string.restore_identity), stringResource(R.string.restore_identity_note)) { restoring = true }
                CardDivider()
                ChevronRow(stringResource(R.string.restore_backup), stringResource(R.string.restore_backup_note)) { restoringBackup = true }
                if (model.canBackUp) {
                    CardDivider()
                    BackupExportRow(model, stringResource(R.string.encrypted_backup), stringResource(R.string.encrypted_backup_note))
                }
            }
            if (next != null) PrimaryButton(stringResource(R.string.continue_), Modifier.padding(top = 8.dp), onClick = next)
        }
    }
    if (showingPhrase) RecoveryPhraseSheet(model) { showingPhrase = false }
    if (restoring) RestoreSheet(model) { restoring = false }
    if (restoringBackup) RestoreBackupSheet(model, { restoringBackup = false }) { next?.invoke() }
}

@Composable
private fun ChevronRow(title: String, subtitle: String, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) { CardRow(title, subtitle) }
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = lichen.tx2, modifier = Modifier.padding(end = 12.dp))
    }
}

/** Groups of digits (fingerprint, safety number), three per line, big and tabular; fewer per
 * line when the text is large (they wrap instead of overflowing). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Groups(groups: List<String>, modifier: Modifier = Modifier) {
    FlowRow(
        modifier.widthIn(max = 320.dp),
        horizontalArrangement = Arrangement.spacedBy(24.dp, Alignment.CenterHorizontally),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        maxItemsInEachRow = 3,
    ) {
        groups.forEach { Text(it, style = figure(22.sp), color = lichen.tx) }
    }
}

@Composable
fun fieldColors() = lichen.let { t ->
    OutlinedTextFieldDefaults.colors(
        focusedContainerColor = t.s1, unfocusedContainerColor = t.s1,
        focusedBorderColor = t.tx2, unfocusedBorderColor = t.line,
        focusedTextColor = t.tx, unfocusedTextColor = t.tx, cursorColor = t.tx,
        focusedPlaceholderColor = t.tx2, unfocusedPlaceholderColor = t.tx2,
    )
}

private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}

/** The 24 words, to write on paper; screenshots and the recent-apps preview are blocked. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RecoveryPhraseSheet(model: AppModel, dismiss: () -> Unit) {
    val t = lichen
    val words = remember { model.recoveryPhrase() }
    val window = LocalContext.current.activity()?.window
    DisposableEffect(Unit) {
        window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
    // The sheet is a window of its own: secure it too (screenshots, recent apps).
    ModalBottomSheet(
        onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg,
        properties = ModalBottomSheetProperties(securePolicy = SecureFlagPolicy.SecureOn),
    ) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(stringResource(R.string.recovery_phrase), fontSize = 22.sp, color = t.tx)
            Text(stringResource(R.string.recovery_phrase_intro), fontSize = 15.sp, color = t.tx2)
            words.chunked(2).forEachIndexed { row, pair ->
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    pair.forEachIndexed { col, w ->
                        Row(
                            Modifier.weight(1f).background(t.s1, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("${row * 2 + col + 1}", fontSize = 13.sp, color = t.tx2, textAlign = TextAlign.End, modifier = Modifier.width(24.dp))
                            Text(w, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, fontFamily = FontFamily.Monospace, color = t.tx, modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
            PrimaryButton(stringResource(R.string.wrote_them_down), Modifier.padding(top = 8.dp)) {
                model.markBackedUp()
                dismiss()
            }
        }
    }
}

/** Restore an identity from its 24 words. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun RestoreSheet(model: AppModel, dismiss: () -> Unit) {
    val t = lichen
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val words = text.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }
    // The words typed are as secret as the words shown (audit M13): a secure window.
    ModalBottomSheet(
        onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg,
        properties = ModalBottomSheetProperties(securePolicy = SecureFlagPolicy.SecureOn),
    ) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(stringResource(R.string.restore_identity), fontSize = 22.sp, color = t.tx)
            Text(stringResource(R.string.restore_intro), fontSize = 14.sp, color = t.tx2)
            OutlinedTextField(
                text, { text = it },
                Modifier.fillMaxWidth().heightIn(min = 160.dp),
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
            PrimaryButton(stringResource(R.string.restore), enabled = words.size == 24) {
                error = model.restore(words)
                if (error == null) dismiss()
            }
        }
    }
}

/**
 * Our identity cannot be read or saved (the Keystore is unavailable, or holds something else):
 * nothing is replaced on its own. Retry, restore from the 24 words, or, when what is stored is
 * unreadable for good, start a new identity deliberately.
 */
@Composable
fun IdentityProblemScreen(model: AppModel, problem: AppModel.IdentityProblem) {
    val t = lichen
    var restoring by remember { mutableStateOf(false) }
    var restoringBackup by remember { mutableStateOf(false) }
    var confirmNew by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    Column(
        Modifier.fillMaxSize().background(t.bg).windowInsetsPadding(WindowInsets.systemBars).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Spacer(Modifier.weight(1f))
        Icon(Icons.Outlined.Key, null, tint = t.sos, modifier = Modifier.size(40.dp))
        Text(
            stringResource(if (problem == AppModel.IdentityProblem.STORAGE) R.string.storage_problem_title else R.string.identity_problem_title),
            fontSize = 24.sp, fontWeight = FontWeight.Bold, color = t.tx,
        )
        Text(
            stringResource(
                when (problem) {
                    AppModel.IdentityProblem.UNAVAILABLE -> R.string.identity_problem_unavailable
                    AppModel.IdentityProblem.CORRUPT -> R.string.identity_problem_corrupt
                    AppModel.IdentityProblem.NOT_SAVED -> R.string.identity_problem_not_saved
                    AppModel.IdentityProblem.STORAGE -> R.string.storage_problem
                },
            ),
            fontSize = 15.sp, color = t.tx2,
        )
        error?.let { Banner(BannerKind.SOS, it) }
        Spacer(Modifier.weight(1f))
        PrimaryButton(stringResource(R.string.try_again), Modifier.fillMaxWidth()) { model.retryIdentity() }
        if (problem != AppModel.IdentityProblem.STORAGE) {
            SecondaryButton(stringResource(R.string.restore_from_words)) { restoring = true }
            SecondaryButton(stringResource(R.string.restore_backup)) { restoringBackup = true }
        }
        if (problem == AppModel.IdentityProblem.CORRUPT) {
            TextButton({ confirmNew = true }, Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.new_identity), color = t.sos, fontSize = 15.sp)
            }
        }
    }
    if (restoring) RestoreSheet(model) { restoring = false }
    if (restoringBackup) RestoreBackupSheet(model, { restoringBackup = false })
    if (confirmNew) {
        AlertDialog(
            onDismissRequest = { confirmNew = false },
            title = { Text(stringResource(R.string.new_identity_confirm)) },
            text = { Text(stringResource(R.string.new_identity_warning)) },
            confirmButton = {
                TextButton({
                    confirmNew = false
                    error = model.startNewIdentity()
                }) { Text(stringResource(R.string.new_identity), color = t.sos) }
            },
            dismissButton = { TextButton({ confirmNew = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}
