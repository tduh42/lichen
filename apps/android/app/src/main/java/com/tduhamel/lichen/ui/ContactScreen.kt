package com.tduhamel.lichen.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hex

/** "Add a contact" (mockup `contact`, §4.7): show my code, scan theirs, compare the safety number. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactScreen(model: AppModel, back: () -> Unit, next: (() -> Unit)? = null) {
    val t = lichen
    var scanning by remember { mutableStateOf(false) }
    var added by remember { mutableStateOf<ContactInfo?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.add_contact), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            val c = added
            if (c != null) {
                VerifyPanel(model, c.key, null) {
                    // Their conversation opens, ready for the first message (the list shows
                    // contacts once there is a message).
                    model.openChat = ChatId.contact(c.key)
                    (next ?: back)()
                }
            } else {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    listOf(false to R.string.my_code, true to R.string.scan).forEachIndexed { i, (scan, label) ->
                        SegmentedButton(
                            selected = scanning == scan,
                            onClick = { scanning = scan },
                            shape = SegmentedButtonDefaults.itemShape(i, 2),
                            colors = segmentedColors(),
                        ) { Text(stringResource(label)) }
                    }
                }
                if (scanning) {
                    ScanPanel { text ->
                        val (contact, e) = model.addCard(text)
                        added = contact
                        error = e
                    }
                } else {
                    MyCode(model)
                }
                error?.let { Banner(BannerKind.SOS, it) }
                // In the first-launch flow: go on without a contact.
                if (next != null) SecondaryButton(stringResource(R.string.later), onClick = next)
            }
        }
    }
}

@Composable
fun segmentedColors() = lichen.let { t ->
    SegmentedButtonDefaults.colors(
        activeContainerColor = t.s2, activeContentColor = t.tx,
        inactiveContainerColor = t.s1, inactiveContentColor = t.tx2,
        activeBorderColor = t.line, inactiveBorderColor = t.line,
    )
}

@Composable
private fun MyCode(model: AppModel) {
    val t = lichen
    val card = model.myCard()
    if (card == null) {
        Banner(BannerKind.WARN, stringResource(R.string.connect_radio_first), stringResource(R.string.code_carries_address))
        return
    }
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        QrImage(card, stringResource(R.string.your_contact_code), Modifier.widthIn(max = 260.dp).fillMaxWidth())
        Text(stringResource(R.string.have_contact_scan), fontSize = 14.sp, color = t.tx2, textAlign = TextAlign.Center)
    }
}

/** Scans a contact code with the camera, or takes it pasted. */
@Composable
private fun ScanPanel(found: (String) -> Unit) {
    val t = lichen
    var pasted by remember { mutableStateOf("") }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        QrScanner(
            found,
            Modifier
                .fillMaxWidth()
                .height(300.dp)
                .background(t.s2, CardShape)
                .border(1.dp, t.line, CardShape),
        )
        Text(stringResource(R.string.point_camera), fontSize = 14.sp, color = t.tx2)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                pasted, { pasted = it },
                Modifier.weight(1f),
                singleLine = true,
                placeholder = { Text(stringResource(R.string.paste_code), fontFamily = FontFamily.Monospace, fontSize = 14.sp) },
                textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 14.sp, color = t.tx),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                shape = RoundedCornerShape(12.dp),
                colors = fieldColors(),
            )
            TextButton(onClick = { found(pasted) }, enabled = pasted.trim().startsWith("lichen:")) {
                Text(stringResource(R.string.add), fontWeight = FontWeight.SemiBold, color = t.tx)
            }
        }
    }
}

/**
 * The safety number with a contact (§4.7): the same 30 digits on both phones. Approving names
 * the identity whose number was shown, so a key that changes meanwhile is never approved unseen
 * (review PR-07).
 */
@Composable
fun VerifyPanel(model: AppModel, node: ByteArray, pending: ByteArray?, done: () -> Unit) {
    val t = lichen
    val name = model.contact(node)?.name ?: model.name(node)
    // The identity whose number is on screen, fixed when the panel opens.
    var shown by remember { mutableStateOf<ByteArray?>(null) }
    var isNew by remember { mutableStateOf(false) }
    var stale by remember { mutableStateOf(false) }
    fun refresh(again: Boolean) {
        val newer = model.conflicts[node.hex()] ?: if (again) null else pending
        isNew = newer != null
        shown = newer ?: model.contact(node)?.identity
        stale = again
    }
    // Fixed once shown; until then, the identity as soon as it is known (nothing compared yet).
    val known = model.contact(node)?.identity
    LaunchedEffect(known) { if (shown == null) refresh(false) }
    val numbers = shown?.let { model.safetyNumberWith(it) }
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        if (stale) {
            Banner(BannerKind.WARN, stringResource(R.string.key_changed_again), stringResource(R.string.compare_new_numbers))
        } else if (model.contact(node)?.verified == true && !isNew) {
            Banner(BannerKind.OK, stringResource(R.string.added_verified, name), stringResource(R.string.scanned_in_person))
        }
        Column(
            Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.compare_numbers_with, name), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            if (numbers != null) {
                Groups(numbers, Modifier.fillMaxWidth())
            } else {
                Text(stringResource(R.string.key_not_known), fontSize = 14.sp, color = t.tx2)
            }
            Text(stringResource(R.string.same_on_both_phones), fontSize = 13.sp, color = t.tx2)
        }
        val id = shown
        // Already verified (a scanned code): nothing to approve; the numbers are a check.
        val verified = model.contact(node)?.verified == true && !isNew && !stale
        if (numbers != null && id != null && verified) {
            Text(stringResource(R.string.verified_by_scan), fontSize = 13.sp, color = t.tx2)
            PrimaryButton(stringResource(R.string.done)) { done() }
            TextButton(onClick = {
                model.setVerified(node, id, false)
                done()
            }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.they_differ), color = t.sosFg, fontWeight = FontWeight.SemiBold)
            }
        } else if (numbers != null && id != null) {
            PrimaryButton(stringResource(R.string.numbers_match)) {
                val ok = if (isNew) model.acceptIdentity(node, id) else model.setVerified(node, id, true)
                if (ok) done() else refresh(true)
            }
            SecondaryButton(stringResource(R.string.they_differ), background = t.sosBg, foreground = t.sosFg) {
                if (isNew) model.rejectIdentity(node, id) else model.setVerified(node, id, false)
                done()
            }
        }
    }
}

/** The verification sheet opened from a conversation. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VerifySheet(model: AppModel, node: ByteArray, pending: ByteArray?, dismiss: () -> Unit) {
    val t = lichen
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(if (pending == null) R.string.safety_number else R.string.new_key), fontSize = 22.sp, color = t.tx)
            VerifyPanel(model, node, pending, dismiss)
        }
    }
}
