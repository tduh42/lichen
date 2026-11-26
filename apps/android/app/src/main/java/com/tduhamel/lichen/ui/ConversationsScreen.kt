package com.tduhamel.lichen.ui

import androidx.compose.runtime.LaunchedEffect
import com.tduhamel.lichen.core.RadioReading
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalView
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
import com.tduhamel.lichen.model.radioBatteryLow
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.unhex
import com.tduhamel.lichen.core.GroupInvitation
import com.tduhamel.lichen.core.IdentityRequest
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Unarchive
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.foundation.clickable
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
    /** Out of the list, under "Archived" (ADR-0034). */
    val archived: Boolean = false,
    /** The core keeps its node as a contact, a person or not: it can be renamed. A group, a
     * channel or a room cannot. */
    val renamable: Boolean = false,
)

/** "Messages": radio, airtime left, then every conversation (mockup `conversations`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationsScreen(
    model: AppModel,
    open: (ChatId) -> Unit,
    pair: () -> Unit,
    addContact: () -> Unit,
    search: () -> Unit,
    newGroup: () -> Unit = {},
) {
    val t = lichen
    val res = LocalResources.current
    var composing by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Row?>(null) }
    var renaming by remember { mutableStateOf<Row?>(null) }

    var showingArchived by remember { mutableStateOf(false) }
    val all = buildList {
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
        for (ch in model.channels) {
            val chat = ChatId.channel(ch.id)
            val last = model.last(chat)
            add(
                Row(
                    chat, ch.name, ch.id, last?.let { preview(model, it, res) } ?: channelKindLabel(res, ch.kind),
                    last?.atMs, res.getString(R.string.meshcore_channel), model.unread(chat), last?.isWaiting == true,
                    model.chatPrefs[chat]?.pinnedAtMs, model.isMuted(chat),
                ),
            )
        }
        // Every other conversation once it has a message, whatever the radio's contact list
        // says (§15.8): lichen and MeshCore chats, room servers, and a group or a channel the
        // radio no longer has. Radios the node heard but nobody wrote to stay in "New message".
        val listed = map { it.id }.toSet()
        for (conv in model.conversations) {
            val chat = ChatId.of(conv.chat)
            if (chat in listed) continue
            val last = conv.last
            add(
                Row(
                    chat, model.conversationTitle(chat, conv.name), chat.hex.unhex(), preview(model, last, res), last.atMs,
                    badge(model, chat, res), conv.unread.toInt(), last.isWaiting,
                    model.chatPrefs[chat]?.pinnedAtMs, model.isMuted(chat),
                ),
            )
        }
    }.map { row ->
        val node = row.id.nodeKey
        row.copy(
            archived = model.isArchived(row.id),
            renamable = node != null && model.coreContacts.any { it.key.contentEquals(node) },
        )
    }
        // Pinned first (the latest pinned on top), then the most recent.
        .sortedWith(compareByDescending<Row> { it.pinnedAt ?: Long.MIN_VALUE }.thenByDescending { it.atMs ?: 0L }.thenBy { it.title })
    val rows = all.filter { !it.archived }
    val archived = all.filter { it.archived }

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
            // The radio in one line; its card (airtime, transports) a tap away, or always with
            // Settings › Show radio details.
            item { RadioLine(model) }
            item { StatusBanners(model, pair) }
            items(model.invitations, key = { "inv" + it.gid.hex() }) { InvitationCard(model, it) }
            items(model.requests, key = { "req" + it.identity.hex() }) { RequestCard(model, it) }
            @Composable
            fun rowsOf(list: List<Row>) = list.forEachIndexed { i, row ->
                if (i > 0) CardDivider(inset = 72.dp)
                SwipeToDelete({ deleting = row }) {
                    ChatRow(
                        row, open = { open(row.id) }, delete = { deleting = row }, rename = { renaming = row },
                        pin = { model.setPinned(row.id, row.pinnedAt == null) },
                        mute = { hours -> if (hours == 0) model.unmute(row.id) else model.mute(row.id, hours) },
                        archive = { model.setArchived(row.id, !row.archived) },
                    )
                }
            }
            if (rows.isEmpty() && archived.isEmpty()) {
                item { EmptyConversations(model) }
            } else if (rows.isNotEmpty()) {
                item { CardList { rowsOf(rows) } }
            }
            // Archived conversations (ADR-0034): one row, opened in place.
            if (archived.isNotEmpty()) {
                item {
                    CardList {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 52.dp)
                                .clickable { showingArchived = !showingArchived }
                                .padding(horizontal = 16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Icon(Icons.Outlined.Archive, null, tint = t.tx2)
                            Text(stringResource(R.string.archived), color = t.tx, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                            Text("${archived.size}", color = t.tx2)
                            Icon(if (showingArchived) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, null, tint = t.tx2)
                        }
                        if (showingArchived) {
                            CardDivider()
                            rowsOf(archived)
                        }
                    }
                }
            }
        }
    }

    if (composing) {
        NewChatSheet(
            model,
            dismiss = { composing = false },
            open = {
                composing = false
                open(it)
            },
            newGroup = {
                composing = false
                newGroup()
            },
            addContact = {
                composing = false
                addContact()
            },
        )
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

/** How a conversation goes: over LoRa (with the contact's hops when known), MeshCore. */
private fun badge(model: AppModel, chat: ChatId, res: android.content.res.Resources): String = when (chat.kind) {
    ChatId.Kind.CONTACT -> hopsBadge(res, model.contact(chat.hex.unhex())?.hops)
    ChatId.Kind.GROUP -> res.getString(R.string.group_flood)
    ChatId.Kind.MESH -> res.getString(R.string.meshcore)
    ChatId.Kind.CHANNEL -> res.getString(R.string.meshcore_channel)
    ChatId.Kind.ROOM -> res.getString(R.string.meshcore_room)
}

/** One line for the conversation list ("Camille: …" in groups and channels). */
private fun preview(model: AppModel, m: com.tduhamel.lichen.core.StoredMessage, res: android.content.res.Resources): String {
    // A MeshCore text's mentions as "@name" (§15.9).
    val text = m.message.display(res, model.nodeKey)
        .let { if (ChatId.of(m.chat).plain) com.tduhamel.lichen.core.meshcoreDisplayText(it) else it }
    val author = m.author
    return when {
        m.authorName != null && !m.outgoing -> "${m.authorName}: $text"
        author != null && !m.outgoing && !ChatId.of(m.chat).plain -> "${model.name(author)}: $text"
        else -> text
    }
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
    archive: () -> Unit,
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
        DropdownMenuItem(
            text = { Text(stringResource(if (row.archived) R.string.unarchive else R.string.archive)) },
            leadingIcon = { Icon(if (row.archived) Icons.Outlined.Unarchive else Icons.Outlined.Archive, null) },
            onClick = {
                menu = false
                archive()
            },
        )
        if (row.renamable) {
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
                // A MeshCore chat is marked as such (§15.6), never in LoRa orange alone.
                TransportBadge(if (row.id.plain) Transport.NEUTRAL else Transport.LORA, row.badge)
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

/** "Demo radio connected", "No radio connected"…; while a connection still reads the radio,
 * what it reads ("Reading the radio · contacts 34 of 206"). */
@Composable
private fun radioHeadline(model: AppModel): String = radioReadingText(model) ?: when (val s = model.radioState) {
    is RadioState.Connected -> stringResource(R.string.radio_connected, model.nodeName.ifEmpty { s.name })
    is RadioState.Connecting -> stringResource(R.string.radio_connecting, s.name)
    RadioState.Scanning -> stringResource(R.string.radio_scanning)
    RadioState.Off -> stringResource(R.string.bluetooth_off)
    RadioState.Idle -> stringResource(R.string.no_radio)
}

/** The radio's state in one line, under the title; a tap opens its card (the airtime left,
 * the transports) and another closes it. With Settings › Show radio details: the card. */
@Composable
fun RadioLine(model: AppModel) {
    val t = lichen
    var open by remember { mutableStateOf(false) }
    AnnounceFetching(model)
    if (model.radioDetails) {
        RadioCard(model)
        return
    }
    if (open) {
        RadioCard(model) { open = false }
        return
    }
    val hint = stringResource(R.string.radio_line_hint)
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(CardShape)
            .clickable(onClickLabel = hint) { open = true }
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.size(8.dp).background(if (model.isConnected) t.ok else t.tx2, CircleShape))
        val headline = radioHeadline(model)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            // On two lines with larger text rather than cut: its count is the point while reading.
            Text(
                headline, fontSize = 14.sp, color = t.tx2, maxLines = 2, overflow = TextOverflow.Ellipsis,
                // Said as one line, its parts apart: "Reading the radio, contacts 34 of 206".
                modifier = Modifier.semantics { contentDescription = said(headline) },
            )
            RadioReadingBar(model)
        }
        Icon(Icons.Outlined.ExpandMore, null, tint = t.tx2)
    }
}

/** What a connection is still reading (protocol §1.1, after `APP_START`), as the radio line
 * says it: "Reading the radio…", "… · contacts 34 of 206", "… · channels", "Fetching waiting
 * messages · 3"; null once it is over, or without a radio. No time left, no percentage: only
 * what the radio's answers count. */
@Composable
fun radioReadingText(model: AppModel): String? {
    if (!model.isConnected) return null
    return when (val r = model.radioReading) {
        null -> null
        is RadioReading.Settings -> stringResource(R.string.reading_radio)
        is RadioReading.Contacts -> stringResource(R.string.reading_radio_contacts, r.done.toInt(), r.total.toInt())
        is RadioReading.Channels -> stringResource(R.string.reading_radio_channels)
        is RadioReading.Messages -> stringResource(R.string.fetching_waiting, r.fetched.toInt())
    }
}

/** A line's parts as TalkBack says them: "·" read as a pause. */
fun said(line: String): String = line.replace(" · ", ", ")

/** Under the radio line while the radio's contacts are read: done of total, thin, in `lora`;
 * nothing without a total. Silent: the line says it. */
@Composable
fun RadioReadingBar(model: AppModel, modifier: Modifier = Modifier) {
    val r = model.radioReading as? RadioReading.Contacts ?: return
    if (!model.isConnected || r.total == 0u) return
    val t = lichen
    Box(
        modifier.fillMaxWidth().height(3.dp).clip(CircleShape).background(t.s2).testTag(READING_BAR).clearAndSetSemantics {},
    ) {
        Box(Modifier.fillMaxWidth((r.done.toFloat() / r.total.toFloat()).coerceIn(0f, 1f)).fillMaxHeight().background(t.lora))
    }
}

const val READING_BAR = "radio-reading-bar"

/** TalkBack hears the steps, not every count: once, as the messages that waited come in. */
@Composable
private fun AnnounceFetching(model: AppModel) {
    val view = LocalView.current
    val fetching = model.isConnected && model.radioReading is RadioReading.Messages
    val said = stringResource(R.string.fetching_waiting_said)
    LaunchedEffect(fetching) {
        @Suppress("DEPRECATION")
        if (fetching) view.announceForAccessibility(said)
    }
}

/** Radio, airtime left this hour, transports (ADR-0007); `close` folds it back into its line. */
@Composable
fun RadioCard(model: AppModel, close: (() -> Unit)? = null) {
    val t = lichen
    val closeLabel = stringResource(R.string.radio_line_close)
    Column(
        Modifier
            .fillMaxWidth()
            .clip(CardShape)
            .background(t.s1)
            .then(if (close != null) Modifier.clickable(onClickLabel = closeLabel, onClick = close) else Modifier)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.size(8.dp).background(if (model.isConnected) t.ok else t.tx2, CircleShape))
            val headline = radioHeadline(model)
            Text(
                headline, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).semantics { contentDescription = said(headline) },
            )
            val r = model.radio
            if (r != null && model.isConnected) {
                Text("${model.linkKind} · SF${r.sf} / ${bandwidth(r.bwHz)}", fontSize = 13.sp, color = t.tx2)
            }
            if (close != null) Icon(Icons.Outlined.ExpandLess, null, tint = t.tx2)
        }
        RadioReadingBar(model)
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
/** A group that lists us, from someone not verified in person: joined only if the user says
 * so (§10.1). */
@Composable
fun InvitationCard(model: AppModel, invitation: GroupInvitation) {
    val t = lichen
    val note = stringResource(R.string.invitation_unverified)
    val members = if (invitation.members.isEmpty()) {
        stringResource(R.string.invitation_nobody_else, note)
    } else {
        stringResource(R.string.invitation_with, invitation.members.joinToString(", "), note)
    }
    Column(
        Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(14.dp)).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            stringResource(R.string.invites_you_to, invitation.adminName, invitation.name),
            fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
        )
        Text(members, fontSize = 13.sp, color = t.tx2)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            PrimaryButton(stringResource(R.string.join), Modifier.weight(1f)) { model.acceptGroup(invitation.gid) }
            SecondaryButton(stringResource(R.string.decline), Modifier.weight(1f)) { model.declineGroup(invitation.gid) }
        }
    }
}

/** Someone using a contact's radio, with an identity it does not have, wants to write
 * (protocol.md §4.6, ADR-0046): what they sent, as text lines (nothing decoded or fetched),
 * and Accept, Block or Delete. */
@Composable
fun RequestCard(model: AppModel, request: IdentityRequest) {
    val t = lichen
    val res = LocalContext.current.resources
    val shown = 3
    Column(
        Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(14.dp)).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            stringResource(R.string.request_title, request.name),
            fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
        )
        Text(stringResource(R.string.request_note), fontSize = 13.sp, color = t.tx2)
        if (request.messages.size > shown) {
            Text(stringResource(R.string.request_earlier, request.messages.size - shown), fontSize = 13.sp, color = t.tx2)
        }
        request.messages.takeLast(shown).forEach { line ->
            Text(
                line.message.display(res, model.nodeKey),
                fontSize = 15.sp, color = t.tx, maxLines = 4,
                modifier = Modifier.fillMaxWidth().background(t.s2, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 6.dp),
            )
        }
        // Accept on a line of its own: three buttons side by side cut their words in French.
        PrimaryButton(stringResource(R.string.accept)) { model.acceptRequest(request.identity) }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            SecondaryButton(stringResource(R.string.block), Modifier.weight(1f)) { model.blockRequest(request.identity) }
            SecondaryButton(stringResource(R.string.delete), Modifier.weight(1f)) { model.deleteRequest(request.identity) }
        }
    }
}

@Composable
fun StatusBanners(model: AppModel, pair: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        // The demo's simulated radio switched off (Settings › Demo, debug `offline`) is shown as
        // a radio that dropped; "Connect" switches it back on.
        if (!model.isConnected) {
            // Never paired yet (D-1): what to do first; a radio that was connected: it dropped.
            val never = !model.isDemo && model.savedRadios.isEmpty()
            Banner(
                BannerKind.WARN,
                stringResource(if (never) R.string.no_radio_yet else R.string.radio_disconnected),
                stringResource(
                    when {
                        model.radioState == RadioState.Off -> R.string.turn_bluetooth_on
                        never -> R.string.connect_radio_to_use
                        else -> R.string.messages_wait_for_radio
                    },
                ),
                stringResource(R.string.connect) to { if (model.isDemo) model.setDemoRadio(true) else pair() },
            )
        }
        // A contact speaks a newer protocol version (ADR-0029).
        if (model.newerPeers.isNotEmpty()) {
            val names = model.newerPeers.map { model.name(it.unhex()) }.sorted().joinToString(", ")
            Banner(BannerKind.WARN, stringResource(R.string.update_lichen), stringResource(R.string.newer_version_in_use, names))
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
        // The radio's battery (ADR-0042).
        val battery = model.radioSettings?.batteryPercent
        if (model.radioBatteryLow && battery != null) {
            Banner(BannerKind.WARN, stringResource(R.string.radio_battery_low, battery.toInt()), stringResource(R.string.radio_battery_low_note))
        }
        if (model.isConnected && model.airtimeLeftMs < 10_000uL) {
            Banner(BannerKind.LORA, stringResource(R.string.budget_spent), stringResource(R.string.budget_spent_note))
        }
    }
}

/** No conversation yet (mockup `etats`, bottom); while a connection still reads the radio,
 * not "no conversations" but that it is reading (its channels and chats are not all in yet). */
@Composable
private fun EmptyConversations(model: AppModel) {
    val t = lichen
    val reading = radioReadingText(model) != null
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
        if (reading) {
            Text(stringResource(R.string.reading_radio), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
        } else {
            Text(stringResource(R.string.no_conversations), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            Text(stringResource(R.string.no_conversations_note), fontSize = 14.sp, color = t.tx2, textAlign = TextAlign.Center)
        }
    }
}
