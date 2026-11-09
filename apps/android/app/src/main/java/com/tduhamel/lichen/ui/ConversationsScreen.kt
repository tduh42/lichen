package com.tduhamel.lichen.ui

import android.text.format.DateFormat
import android.text.format.DateUtils
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.QrCode2
import androidx.compose.material.icons.filled.NotificationsOff
import androidx.compose.material.icons.outlined.NotificationsOff
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.IconButton
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.AppModel.RadioState
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.bandwidth
import com.tduhamel.lichen.model.display
import com.tduhamel.lichen.model.hopsBadge
import com.tduhamel.lichen.model.isWaiting
import java.util.Date

private data class Row(
    val id: ChatId,
    val title: String,
    val key: ByteArray,
    val preview: String,
    val atMs: Long?,
    val badge: String,
    val unread: Int,
    /** Our last message is still waiting (§6.1). */
    val waiting: Boolean = false,
    /** Pinned since (Unix ms), and muted now. */
    val pinnedAt: Long? = null,
    val muted: Boolean = false,
) {
    /** A contact (can be renamed), not a group. */
    val contact: Boolean get() = !id.group
}

/** "Messages": radio, airtime left, then every conversation (mockup `conversations`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationsScreen(
    model: AppModel,
    open: (ChatId) -> Unit,
    pair: () -> Unit,
    addContact: () -> Unit,
    search: () -> Unit,
) {
    val t = lichen
    val res = LocalResources.current
    var composing by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Row?>(null) }
    var renaming by remember { mutableStateOf<Row?>(null) }

    val rows = buildList {
        for (g in model.groups) {
            val chat = ChatId.group(g.gid)
            val last = model.last(chat)
            add(
                Row(
                    chat, g.name, g.gid,
                    last?.let { preview(model, it, res) } ?: res.getQuantityString(R.plurals.members_count, g.members.size, g.members.size),
                    last?.atMs, res.getString(R.string.group_flood), model.unread(chat), last?.isWaiting == true,
                    model.chatPrefs[chat]?.pinnedAtMs, model.isMuted(chat),
                ),
            )
        }
        // A contact appears once there is something to show: radios the node heard but nobody
        // wrote to stay in "New message".
        for (c in model.contacts) {
            val chat = ChatId.contact(c.key)
            val last = model.last(chat) ?: continue
            add(
                Row(
                    chat, c.name, c.key, preview(model, last, res), last.atMs, hopsBadge(res, c.hops), model.unread(chat),
                    last.isWaiting, model.chatPrefs[chat]?.pinnedAtMs, model.isMuted(chat),
                ),
            )
        }
        // Pinned first (the latest pinned on top), then the most recent.
    }.sortedWith(compareByDescending<Row> { it.pinnedAt ?: Long.MIN_VALUE }.thenByDescending { it.atMs ?: 0L }.thenBy { it.title })

    Scaffold(
        containerColor = t.bg,
        // The tab bar below takes the navigation bar.
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.tab_messages), fontSize = 22.sp) },
                actions = {
                    IconButton(onClick = search) { Icon(Icons.Outlined.Search, stringResource(R.string.search_messages), tint = t.tx) }
                    IconButton(onClick = addContact) { Icon(Icons.Outlined.QrCode2, stringResource(R.string.add_contact), tint = t.tx) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx),
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { composing = true },
                icon = { Icon(Icons.Filled.Add, null) },
                text = { Text(stringResource(R.string.new_short)) },
                containerColor = t.actBg,
                contentColor = t.actFg,
                modifier = Modifier.semantics { contentDescription = res.getString(R.string.new_message) },
            )
        },
    ) { padding ->
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 88.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { RadioCard(model) }
            item { StatusBanners(model, pair) }
            if (rows.isEmpty()) {
                item { EmptyConversations() }
            } else {
                item {
                    CardList {
                        rows.forEachIndexed { i, row ->
                            if (i > 0) CardDivider(inset = 72.dp)
                            SwipeToDelete({ deleting = row }) {
                                ChatRow(
                                    row, open = { open(row.id) }, delete = { deleting = row }, rename = { renaming = row },
                                    pin = { model.setPinned(row.id, row.pinnedAt == null) },
                                    mute = { hours -> if (hours == 0) model.unmute(row.id) else model.mute(row.id, hours) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (composing) {
        NewChatSheet(model, dismiss = { composing = false }, open = {
            composing = false
            open(it)
        })
    }
    renaming?.let { row -> RenameDialog(model, row.key) { renaming = null } }
    deleting?.let { row ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text(stringResource(R.string.delete_conversation_with, row.title)) },
            text = { Text(stringResource(R.string.delete_conversation_note)) },
            confirmButton = {
                TextButton(onClick = {
                    model.deleteConversation(row.id)
                    deleting = null
                }) { Text(stringResource(R.string.delete), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** One line for the conversation list ("Camille: …" in groups). */
private fun preview(model: AppModel, m: com.tduhamel.lichen.core.StoredMessage, res: android.content.res.Resources): String {
    val text = m.message.display(res)
    val author = m.author
    return if (author != null && !m.outgoing) "${model.name(author)}: $text" else text
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ChatRow(
    row: Row,
    open: () -> Unit,
    delete: () -> Unit,
    rename: () -> Unit,
    pin: () -> Unit,
    /** Hours to mute for, null: until turned back on, 0: unmute. */
    mute: (Int?) -> Unit,
) {
    val t = lichen
    val context = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    var muteMenu by remember { mutableStateOf(false) }
    Box {
    DropdownMenu(menu, { menu = false }, containerColor = t.s1) {
        DropdownMenuItem(
            text = { Text(stringResource(if (row.pinnedAt != null) R.string.unpin else R.string.pin)) },
            leadingIcon = { Icon(Icons.Outlined.PushPin, null) },
            onClick = {
                menu = false
                pin()
            },
        )
        DropdownMenuItem(
            text = { Text(stringResource(if (row.muted) R.string.unmute else R.string.mute)) },
            leadingIcon = { Icon(if (row.muted) Icons.Outlined.Notifications else Icons.Outlined.NotificationsOff, null) },
            onClick = {
                menu = false
                if (row.muted) mute(0) else muteMenu = true
            },
        )
        if (row.contact) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.rename)) },
                leadingIcon = { Icon(Icons.Outlined.Edit, null) },
                onClick = {
                    menu = false
                    rename()
                },
            )
        }
        DropdownMenuItem(
            text = { Text(stringResource(R.string.delete_conversation), color = t.sos) },
            leadingIcon = { Icon(Icons.Outlined.Delete, null, tint = t.sos) },
            onClick = {
                menu = false
                delete()
            },
        )
    }
    // SOS alerts are never muted.
    DropdownMenu(muteMenu, { muteMenu = false }, containerColor = t.s1) {
        listOf(1 to R.string.mute_1h, 8 to R.string.mute_8h, null to R.string.mute_always).forEach { (hours, label) ->
            DropdownMenuItem(
                text = { Text(stringResource(label)) },
                onClick = {
                    muteMenu = false
                    mute(hours)
                },
            )
        }
    }
    Row(
        Modifier
            .fillMaxWidth()
            .background(t.s1)
            .combinedClickable(onClick = open, onLongClick = { menu = true })
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(row.title, row.key)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // The name and its glyphs take the room left; the time stays at the end.
                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        row.title, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false),
                    )
                    if (row.pinnedAt != null) {
                        Icon(Icons.Filled.PushPin, stringResource(R.string.pinned), tint = t.tx2, modifier = Modifier.padding(start = 6.dp).size(14.dp))
                    }
                    if (row.muted) {
                        Icon(Icons.Filled.NotificationsOff, stringResource(R.string.muted), tint = t.tx2, modifier = Modifier.padding(start = 4.dp).size(14.dp))
                    }
                }
                row.atMs?.let { Text(shortTime(context, it), fontSize = 13.sp, color = t.tx2) }
            }
            if (row.preview.isNotEmpty()) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (row.waiting) {
                        Icon(Icons.Outlined.Schedule, stringResource(R.string.waiting_to_be_sent), tint = t.tx2, modifier = Modifier.size(14.dp))
                    }
                    Text(row.preview, fontSize = 14.sp, color = t.tx2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                TransportBadge(Transport.LORA, row.badge)
                Spacer(Modifier.weight(1f))
                if (row.unread > 0) {
                    val label = pluralStringResource(R.plurals.unread, row.unread, row.unread)
                    Box(
                        Modifier
                            .heightIn(min = 20.dp)
                            .widthIn(min = 20.dp)
                            // Grey when muted.
                            .background(if (row.muted) t.s2 else t.actBg, CircleShape)
                            .padding(horizontal = 6.dp)
                            .semantics { contentDescription = label },
                        contentAlignment = Alignment.Center,
                    ) { Text("${row.unread}", fontSize = 12.sp, fontWeight = FontWeight.Bold, color = if (row.muted) t.tx2 else t.actFg) }
                }
            }
        }
    }
}
}

/** "11:20" today, "Yesterday", or a short date. */
fun shortTime(context: android.content.Context, atMs: Long): String = when {
    DateUtils.isToday(atMs) -> DateFormat.getTimeFormat(context).format(Date(atMs))
    DateUtils.isToday(atMs + DateUtils.DAY_IN_MILLIS) ->
        DateUtils.getRelativeTimeSpanString(atMs, System.currentTimeMillis(), DateUtils.DAY_IN_MILLIS).toString()
    else -> DateFormat.getDateFormat(context).format(Date(atMs))
}

/** Radio, airtime left this hour, transports (ADR-0007). */
@Composable
fun RadioCard(model: AppModel) {
    val t = lichen
    val headline = when (val s = model.radioState) {
        is RadioState.Connected -> stringResource(R.string.radio_connected, model.nodeName.ifEmpty { s.name })
        is RadioState.Connecting -> stringResource(R.string.radio_connecting, s.name)
        RadioState.Scanning -> stringResource(R.string.radio_scanning)
        RadioState.Off -> stringResource(R.string.bluetooth_off)
        RadioState.Idle -> stringResource(R.string.no_radio)
    }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(CardShape)
            .background(t.s1)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.size(8.dp).background(if (model.isConnected) t.ok else t.tx2, CircleShape))
            Text(
                headline, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f),
            )
            val r = model.radio
            if (r != null && model.isConnected) {
                Text("${model.linkKind} · SF${r.sf} / ${bandwidth(r.bwHz)}", fontSize = 13.sp, color = t.tx2)
            }
        }
        AirtimeMeter(model.budget.usedMs, model.budget.limitMs)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            TransportBadge(Transport.LORA, "LoRa")
            Text(
                stringResource(if (model.isConnected) R.string.available else R.string.unavailable),
                fontSize = 12.sp, color = t.tx2, modifier = Modifier.weight(1f),
            )
            if (model.bridgeOnline) TransportBadge(Transport.IROH, "iroh")
            if (model.torStatus == AppModel.TorStatus.READY) TransportBadge(Transport.TOR, "Tor")
            if (!model.bridgeOnline && model.torStatus != AppModel.TorStatus.READY) {
                Text(stringResource(R.string.no_internet_bridge), fontSize = 11.sp, color = t.tx2, maxLines = 1)
            }
        }
    }
}

/** Degraded states that concern the whole app (mockup `etats`): radio, budget. */
@Composable
fun StatusBanners(model: AppModel, pair: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (!model.isConnected && !model.isDemo) {
            Banner(
                BannerKind.WARN,
                stringResource(R.string.radio_disconnected),
                stringResource(
                    if (model.radioState == RadioState.Off) R.string.turn_bluetooth_on else R.string.messages_wait_for_radio,
                ),
                stringResource(R.string.connect) to pair,
            )
        }
        // Someone is searching for us (protocol.md §11): our phone answers their pings.
        if (model.searchedBy.isNotEmpty()) {
            val names = model.searchedBy.map { model.name(it.by) }.distinct().joinToString(", ")
            val power = model.loweredPower
            Banner(
                BannerKind.SOS,
                stringResource(R.string.searching_for_you, names),
                if (power != null) stringResource(R.string.power_lowered, power.toInt()) else stringResource(R.string.searched_note),
            )
        }
        if (model.isConnected && model.airtimeLeftMs < 10_000uL) {
            Banner(BannerKind.LORA, stringResource(R.string.budget_spent), stringResource(R.string.budget_spent_note))
        }
    }
}

/** No conversation yet (mockup `etats`, bottom). */
@Composable
private fun EmptyConversations() {
    val t = lichen
    Column(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .border(1.dp, t.line, CardShape)
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ChirpMark(t.tx2, Modifier.size(40.dp))
        Text(stringResource(R.string.no_conversations), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
        Text(stringResource(R.string.no_conversations_note), fontSize = 14.sp, color = t.tx2, textAlign = TextAlign.Center)
    }
}
