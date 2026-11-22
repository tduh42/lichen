package com.tduhamel.lichen.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.background
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ChannelKind
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.unhex

// MeshCore chats (protocol.md §15, ADR-0026): plain text with people who use a stock MeshCore
// app, kept apart from lichen's own chats and said to be weaker.

/** "MeshCore channel · public". */
fun channelKindLabel(res: android.content.res.Resources, kind: ChannelKind): String = res.getString(
    when (kind) {
        ChannelKind.PUBLIC_CHANNEL -> R.string.channel_kind_public
        ChannelKind.HASHTAG_CHANNEL -> R.string.channel_kind_hashtag
        ChannelKind.PRIVATE_CHANNEL -> R.string.channel_kind_private
    },
)

/** What a MeshCore chat is, and for a channel: its kind and leaving it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlainInfoSheet(model: AppModel, chat: ChatId, dismiss: () -> Unit, left: () -> Unit) {
    val t = lichen
    val channel = model.channel(chat)
    val room = chat.kind == ChatId.Kind.ROOM
    var confirm by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                stringResource(
                    when {
                        room -> R.string.room_details
                        channel == null -> R.string.meshcore_chat
                        else -> R.string.channel
                    },
                ),
                fontSize = 22.sp, color = t.tx,
            )
            val text = when {
                room -> R.string.plain_room_explainer
                channel == null -> R.string.plain_direct_explainer
                channel.kind == ChannelKind.PUBLIC_CHANNEL -> R.string.plain_public_explainer
                else -> R.string.plain_channel_explainer
            }
            Banner(BannerKind.WARN, stringResource(R.string.not_lichen_encrypted), stringResource(text))
            // Their node's MeshCore public key, to copy (F-1).
            if (channel == null) CardList { MeshCoreKeyRow(chat.hex.unhex()) }
            // A room's login (§15.8).
            if (room) ServerLoginCard(model, chat.hex.unhex(), room = true)
            if (channel != null) {
                CardList { CardRow(channel.name, channelKindLabel(LocalResources.current, channel.kind)) }
                // In the stock app's format (§15.3).
                SecondaryButton(stringResource(R.string.share_by_qr)) { sharing = true }
                if (channel.kind == ChannelKind.HASHTAG_CHANNEL) {
                    Text(stringResource(R.string.hashtag_readers, channel.name), fontSize = 13.sp, color = t.tx2)
                }
                SecondaryButton(stringResource(R.string.leave_channel), background = t.sosBg, foreground = t.sosFg) {
                    confirm = true
                }
            }
            Text(stringResource(R.string.plain_lichen_apart), fontSize = 13.sp, color = t.tx2)
        }
    }
    if (sharing && channel != null) ChannelQrSheet(model, channel) { sharing = false }
    if (confirm && channel != null) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.leave_named, channel.name)) },
            text = { Text(stringResource(R.string.leave_channel_note)) },
            confirmButton = {
                TextButton(onClick = {
                    confirm = false
                    model.leaveChannel(channel.id)
                    left()
                }) { Text(stringResource(R.string.leave), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { confirm = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** Joins a MeshCore channel: Public, a hashtag, or a private one from its secret (§15.3). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JoinChannelSheet(model: AppModel, dismiss: () -> Unit, open: (ChatId) -> Unit) {
    val t = lichen
    var tag by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var secret by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var scanning by remember { mutableStateOf(false) }
    val hasPublic = model.channels.any { it.kind == ChannelKind.PUBLIC_CHANNEL }
    fun join(kind: ChannelKind, label: String) {
        model.joinChannel(kind, label.trim(), secret.trim())
            .onSuccess { dismiss(); open(it) }
            .onFailure { error = it.message }
    }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(stringResource(R.string.join_a_channel), fontSize = 22.sp, color = t.tx)
            // A channel's QR code from a MeshCore app or lichen (§15.3).
            SecondaryButton(stringResource(R.string.scan_channel_button)) { scanning = true }
            Text(stringResource(R.string.scan_channel_note), fontSize = 12.sp, color = t.tx2)
            SecondaryButton(
                stringResource(if (hasPublic) R.string.public_already_joined else R.string.join_public),
                enabled = !hasPublic,
            ) { join(ChannelKind.PUBLIC_CHANNEL, "") }
            Text(stringResource(R.string.public_channel_note), fontSize = 12.sp, color = t.tx2)

            SectionLabel(stringResource(R.string.hashtag_channel), Modifier.padding(top = 8.dp))
            OutlinedTextField(
                tag, { tag = it }, Modifier.fillMaxWidth(), singleLine = true,
                placeholder = { Text(stringResource(R.string.hashtag_placeholder)) },
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                shape = RoundedCornerShape(12.dp), colors = fieldColors(),
            )
            SecondaryButton(stringResource(R.string.join_hashtag), enabled = tag.isNotBlank()) {
                join(ChannelKind.HASHTAG_CHANNEL, tag)
            }
            Text(stringResource(R.string.hashtag_note), fontSize = 12.sp, color = t.tx2)

            SectionLabel(stringResource(R.string.private_channel), Modifier.padding(top = 8.dp))
            OutlinedTextField(
                name, { name = it }, Modifier.fillMaxWidth(), singleLine = true,
                placeholder = { Text(stringResource(R.string.name)) },
                shape = RoundedCornerShape(12.dp), colors = fieldColors(),
            )
            OutlinedTextField(
                secret, { secret = it }, Modifier.fillMaxWidth(), singleLine = true,
                placeholder = { Text(stringResource(R.string.channel_secret_placeholder)) },
                textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                shape = RoundedCornerShape(12.dp), colors = fieldColors(),
            )
            SecondaryButton(stringResource(R.string.join_private), enabled = name.isNotBlank() && secret.isNotBlank()) {
                join(ChannelKind.PRIVATE_CHANNEL, name)
            }
            Text(stringResource(R.string.private_channel_note), fontSize = 12.sp, color = t.tx2)
            error?.let { Banner(BannerKind.SOS, it) }
        }
    }
    if (scanning) ScanChannelSheet(model, { scanning = false }) { dismiss(); open(it) }
}
