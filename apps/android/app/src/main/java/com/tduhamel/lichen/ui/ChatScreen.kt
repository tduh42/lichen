package com.tduhamel.lichen.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.LockOpen
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.FailReason
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.Notifications
import com.tduhamel.lichen.model.display
import com.tduhamel.lichen.model.fragmentsText
import com.tduhamel.lichen.model.hopsBadge
import com.tduhamel.lichen.model.secondsText

/** One conversation (mockup `chat`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(model: AppModel, chat: ChatId, back: () -> Unit, details: () -> Unit) {
    val t = lichen
    var error by remember { mutableStateOf<String?>(null) }
    // The verification sheet: an empty array for the current identity, else a pending one.
    var verifying by remember { mutableStateOf<ByteArray?>(null) }
    val messages = model.chats[chat].orEmpty()
    val list = rememberLazyListState()

    val context = LocalContext.current
    LaunchedEffect(chat) {
        model.load(chat)
        model.markRead(chat)
        Notifications.clear(context, chat)
        // Debug extra `screen`: the group's details, or the verification sheet.
        when (model.debugScreen) {
            "group" -> if (chat.group) details()
            "verify" -> if (!chat.group) verifying = ByteArray(0)
        }
    }
    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) list.animateScrollToItem(messages.size)
        model.markRead(chat)
    }

    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = {
                IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
            },
            title = {
                Column {
                    Text(model.title(chat), fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                    Text(subtitle(model, chat), fontSize = 12.sp, color = t.tx2)
                }
            },
            actions = {
                if (chat.group) {
                    IconButton(onClick = details) { Icon(Icons.Outlined.Info, stringResource(R.string.group_details), tint = t.tx) }
                }
            },
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = t.s1, titleContentColor = t.tx, navigationIconContentColor = t.tx,
            ),
        )
        LazyColumn(
            Modifier.weight(1f),
            state = list,
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item { SessionLine(model, chat) { verifying = it } }
            items(messages, key = { it.id }) { m -> MessageBubble(model, m, chat.group) }
            val failed = messages.lastOrNull { it.outgoing }?.takeIf { it.status is MessageStatus.Failed }
            if (failed != null) {
                item { FailureBanner(model, failed) { error = model.resend(failed) } }
            }
        }
        Composer(model, chat) { error = it }
    }

    val node = chat.contactKey
    val v = verifying
    if (node != null && v != null) {
        VerifySheet(model, node, v.takeIf { it.isNotEmpty() }) { verifying = null }
    }
    error?.let {
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text(stringResource(R.string.not_sent)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { error = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun subtitle(model: AppModel, chat: ChatId): String {
    val res = LocalResources.current
    if (!chat.group) {
        val key = chat.contactKey ?: return ""
        val hops = hopsBadge(res, model.contact(key)?.hops)
        val snr = model.lastSnr[chat.hex] ?: return hops
        return stringResource(R.string.hops_snr, hops, "%.0f".format(snr))
    }
    val g = model.group(chat) ?: return ""
    val others = maxOf(g.members.size - 1, 0)
    return if (g.keysKnown.toInt() < others) {
        stringResource(R.string.group_keys, g.members.size, g.keysKnown.toInt(), others)
    } else {
        pluralStringResource(R.plurals.group_members_flood, g.members.size, g.members.size)
    }
}

@Composable
private fun SessionLine(model: AppModel, chat: ChatId, verify: (ByteArray) -> Unit) {
    val t = lichen
    if (chat.group) {
        TintedLine(Icons.Outlined.Lock, t.ok, stringResource(R.string.group_signed), t.tx2)
        return
    }
    val key = chat.contactKey ?: return
    val c = model.contact(key)
    when {
        model.conflicts.containsKey(chat.hex) -> Banner(
            BannerKind.SOS,
            stringResource(R.string.key_changed_title, model.title(chat)),
            stringResource(R.string.key_changed_note),
            stringResource(R.string.compare) to { model.conflicts[chat.hex]?.let(verify) },
        )
        c?.identity != null -> Box(Modifier.fillMaxWidth().clickable { verify(ByteArray(0)) }) {
            TintedLine(
                if (c.verified) Icons.Outlined.Lock else Icons.Outlined.LockOpen,
                if (c.verified) t.ok else t.warn,
                stringResource(if (c.verified) R.string.session_verified else R.string.session_to_verify),
                if (c.verified) t.tx2 else t.warnFg,
            )
        }
        chat.hex in model.secured -> TintedLine(Icons.Outlined.Lock, t.ok, stringResource(R.string.session_encrypted), t.tx2)
    }
}

@Composable
private fun TintedLine(icon: ImageVector, tint: Color, text: String, color: Color) {
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(14.dp))
        Text(text, fontSize = 12.sp, color = color, modifier = Modifier.padding(start = 6.dp))
    }
}

/** Why the last message did not go, and a way to send it again (mockup `etats`). */
@Composable
private fun FailureBanner(model: AppModel, m: StoredMessage, resend: () -> Unit) {
    val reason = (m.status as? MessageStatus.Failed)?.reason ?: return
    val res = LocalResources.current
    val title = model.title(ChatId.of(m.chat))
    val action = if (m.message is Message.Text) stringResource(R.string.resend) to resend else null
    when (reason) {
        FailReason.NO_PATH -> Banner(BannerKind.WARN, stringResource(R.string.no_path_to, title), stringResource(R.string.no_path_note), action)
        FailReason.UNREACHABLE -> Banner(BannerKind.WARN, stringResource(R.string.unreachable_title, title), stringResource(R.string.unreachable_note), action)
        FailReason.TIMEOUT -> Banner(
            BannerKind.SOS, stringResource(R.string.send_abandoned),
            pluralStringResource(R.plurals.no_ack_after, m.fragments.toInt(), m.fragments.toInt()), action,
        )
        FailReason.REFUSED, FailReason.TOO_LARGE -> Banner(BannerKind.SOS, stringResource(R.string.refused), reason.display(res))
        FailReason.INTERRUPTED -> Banner(BannerKind.WARN, stringResource(R.string.not_sent), stringResource(R.string.fail_interrupted_note), action)
        FailReason.IDENTITY_CHANGED -> Banner(
            BannerKind.SOS, stringResource(R.string.not_sent_identity_changed), stringResource(R.string.compare_before_accepting, title),
        )
    }
}

@Composable
private fun MessageBubble(model: AppModel, m: StoredMessage, isGroup: Boolean) {
    val t = lichen
    val res = LocalResources.current
    val author = m.author
    Column(
        Modifier.fillMaxWidth().padding(start = if (m.outgoing) 64.dp else 0.dp, end = if (m.outgoing) 0.dp else 64.dp),
        horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        if (isGroup && !m.outgoing && author != null) {
            Text(
                model.name(author), fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx2,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
        }
        Text(
            m.message.display(res),
            fontSize = 16.sp,
            color = if (m.outgoing) t.actFg else t.tx,
            modifier = Modifier
                .background(if (m.outgoing) t.actBg else t.s2, RoundedCornerShape(18.dp))
                .padding(horizontal = 14.dp, vertical = 9.dp),
        )
        val status = m.status
        if (m.outgoing && status is MessageStatus.Sending && status.total.toInt() > 1) {
            FragmentProgress(status.sent.toInt(), status.total.toInt(), Modifier.width(220.dp).padding(top = 2.dp))
        }
        if (m.outgoing) caption(m)?.let { Text(it, fontSize = 11.sp, color = t.tx2) }
    }
}

@Composable
private fun caption(m: StoredMessage): String? {
    val res = LocalResources.current
    return when (val s = m.status) {
        MessageStatus.Received -> null
        is MessageStatus.Sending ->
            if (s.total.toInt() > 1) {
                stringResource(R.string.waiting_for_ack)
            } else {
                stringResource(R.string.sending_fragments, minOf(s.sent.toInt(), s.total.toInt()), s.total.toInt())
            }
        MessageStatus.Delivered -> pluralStringResource(
            R.plurals.delivered_fragments, m.fragments.toInt(), m.fragments.toInt(), secondsText(m.airtimeMs),
        )
        is MessageStatus.Failed -> stringResource(R.string.not_delivered, s.reason.display(res))
        MessageStatus.Flooded -> stringResource(R.string.sent_to_group)
    }
}

@Composable
private fun Composer(model: AppModel, chat: ChatId, onError: (String) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    var draft by remember(chat) { mutableStateOf("") }
    val text = draft.trim()
    val cost = if (chat.group) {
        stringResource(R.string.flooded_to_every_member)
    } else {
        model.estimate(chat, draft)?.let {
            stringResource(R.string.estimated_cost, fragmentsText(res, it.fragments.toInt()), secondsText(it.airtimeMs))
        }
    }
    Column(
        Modifier
            .fillMaxWidth()
            .background(t.s1)
            // Above the keyboard when it is up, else above the navigation bar.
            .windowInsetsPadding(WindowInsets.navigationBars.union(WindowInsets.ime))
            .padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (cost != null) Text(cost, fontSize = 11.sp, color = t.tx2, modifier = Modifier.padding(start = 8.dp))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextField(
                draft,
                { draft = it },
                Modifier.weight(1f),
                placeholder = { Text(stringResource(R.string.message_placeholder)) },
                maxLines = 5,
                shape = RoundedCornerShape(24.dp),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = t.bg, unfocusedContainerColor = t.bg,
                    focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
                    focusedTextColor = t.tx, unfocusedTextColor = t.tx, cursorColor = t.tx,
                    focusedPlaceholderColor = t.tx2, unfocusedPlaceholderColor = t.tx2,
                ),
            )
            IconButton(
                onClick = {
                    val e = model.send(text, chat)
                    if (e != null) onError(e) else draft = ""
                },
                enabled = text.isNotEmpty() && model.isConnected,
                modifier = Modifier.size(48.dp),
                colors = IconButtonDefaults.iconButtonColors(
                    containerColor = t.actBg, contentColor = t.actFg,
                    disabledContainerColor = t.s2, disabledContentColor = t.tx2,
                ),
            ) { Icon(Icons.Filled.ArrowUpward, stringResource(R.string.send)) }
        }
    }
}
