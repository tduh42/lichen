package com.tduhamel.lichen.ui

import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.border
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.automirrored.outlined.Forward
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.ui.text.font.FontStyle
import com.tduhamel.lichen.core.QuoteKind
import com.tduhamel.lichen.core.TextTarget
import com.tduhamel.lichen.core.textLinks
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Unarchive
import androidx.compose.material.icons.outlined.Block
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import com.tduhamel.lichen.core.ChatRef
import com.tduhamel.lichen.core.Quote
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.automirrored.outlined.Reply
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.height
import kotlinx.coroutines.delay
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import android.content.ClipData
import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.outlined.LocationOn
import androidx.compose.material.icons.outlined.Mic
import androidx.compose.material.icons.outlined.NoEncryption
import androidx.compose.material.icons.outlined.Photo
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material.icons.outlined.EmojiEmotions
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.draw.clip
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.derivedStateOf
import androidx.core.content.ContextCompat
import com.tduhamel.lichen.core.PreparedImage
import com.tduhamel.lichen.core.prepareImage
import com.tduhamel.lichen.model.pickPhoto
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
import com.tduhamel.lichen.model.noticeText
import com.tduhamel.lichen.model.timerLabel
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Timer
import androidx.compose.material.icons.outlined.Timer
import androidx.compose.material3.HorizontalDivider
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.material.icons.outlined.GppMaybe
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.FailReason
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.core.WaitReason
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.VoiceRecorder
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.Notifications
import com.tduhamel.lichen.model.display
import com.tduhamel.lichen.model.fragmentsText
import com.tduhamel.lichen.model.hopsBadge
import com.tduhamel.lichen.model.isWaiting
import com.tduhamel.lichen.model.secondsText
import com.tduhamel.lichen.model.unhex
import com.tduhamel.lichen.model.writtenAtMs
import com.tduhamel.lichen.model.writtenEarlierMs

/** One conversation (mockup `chat`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(model: AppModel, chat: ChatId, back: () -> Unit, details: () -> Unit, media: () -> Unit = {}) {
    val t = lichen
    var error by remember { mutableStateOf<String?>(null) }
    val noLongerHere = stringResource(R.string.message_no_longer_here)
    // The verification sheet: an empty array for the current identity, else a pending one.
    var verifying by remember { mutableStateOf<ByteArray?>(null) }
    var renaming by remember { mutableStateOf(false) }
    /** What a MeshCore chat is (§15.6). */
    var plainInfo by remember { mutableStateOf(false) }
    /** Blocking the other side, to confirm (ADR-0034). */
    var blocking by remember { mutableStateOf(false) }
    val messages = model.chats[chat].orEmpty()
    val list = rememberLazyListState()

    val context = LocalContext.current
    // On screen: its messages need no notification.
    DisposableEffect(chat) {
        model.visibleChat = chat
        onDispose {
            if (model.visibleChat == chat) model.visibleChat = null
            // A recording belongs to its conversation: leaving it stops the recording, which
            // waits there to be sent or discarded (ADR-0034).
            model.keepRecording(chat)
        }
    }
    // The first message that was unread when the chat opened, and how many were: the divider
    // stays there until the chat is left. A message opened from a search is highlighted.
    var dividerBefore by remember(chat) { mutableStateOf<Long?>(null) }
    var unreadAtOpen by remember(chat) { mutableIntStateOf(0) }
    var highlight by remember(chat) { mutableStateOf<Long?>(null) }
    // The message being answered (§6.2), kept with the draft (ADR-0034).
    var replyTo by remember(chat) { mutableStateOf<StoredMessage?>(null) }
    var replyRestored by remember(chat) { mutableStateOf(false) }
    LaunchedEffect(replyTo?.id, replyRestored) { if (replyRestored) model.setReplyDraft(chat, replyTo?.id) }
    /** Our message being edited (§6.4). */
    var editing by remember(chat) { mutableStateOf<StoredMessage?>(null) }
    LaunchedEffect(chat) {
        model.load(chat)
        model.focus?.takeIf { it.first == chat }?.let { (_, id) ->
            model.focus = null
            if (model.loadUntil(chat, id)) highlight = id else error = noLongerHere
        }
        // The message the draft answers, kept with it (ADR-0034).
        model.replyDraft(chat)?.let { id ->
            val m = if (model.loadUntil(chat, id)) model.chats[chat].orEmpty().firstOrNull { it.id == id } else null
            if (m != null && m.referable) replyTo = m else model.setReplyDraft(chat, null)
        }
        replyRestored = true
        // Before marking them read: where the unread messages start.
        val unread = model.chats[chat].orEmpty().filter { !it.outgoing && !it.read }
        dividerBefore = unread.firstOrNull()?.id
        unreadAtOpen = unread.size
        model.markRead(chat)
        Notifications.clear(context, chat)
        // Debug extra `screen`: the group's details, or the verification sheet.
        when (model.debugScreen) {
            "group", "invite" -> if (chat.group) details()
            "verify" -> if (chat.contactKey != null) verifying = ByteArray(0)
        }
    }
    // The list's rows: the history state, the session line, a separator per day, the messages,
    // a failure banner.
    val days = messages.indices.count { it == 0 || dayOf(messages[it - 1].atMs) != dayOf(messages[it].atMs) }
    val failedLast = messages.lastOrNull { it.outgoing }?.status is MessageStatus.Failed
    val top = (if (model.historyError[chat] != null) 1 else 0) + (if (model.olderAvailable[chat] == true) 1 else 0)
    val divider = if (messages.any { it.id == dividerBefore }) 1 else 0
    val rows = top + 1 + days + divider + messages.size + (if (failedLast) 1 else 0)
    var opened by remember(chat) { mutableStateOf(false) }
    var unseen by remember(chat) { mutableIntStateOf(0) }
    // At the bottom: the last row (or the one before it) is on screen.
    val scope = rememberCoroutineScope()
    var keepAt by remember(chat) { mutableStateOf<Long?>(null) }
    LaunchedEffect(messages.firstOrNull()?.id) {
        keepAt?.let { list.scrollToItem(rowOf(model, chat, it, dividerBefore)) }
        keepAt = null
    }
    val atBottom by remember {
        derivedStateOf {
            val info = list.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()
            last == null || last.index >= info.totalItemsCount - 2
        }
    }
    var lastSeen by remember(chat) { mutableStateOf<Long?>(null) }
    LaunchedEffect(messages.lastOrNull()?.id) {
        // Opens on the latest messages at once. A new last message is followed when we were at
        // the bottom or sent it; else it counts in the pill. Older pages move nothing (keys).
        val last = messages.lastOrNull() ?: return@LaunchedEffect
        if (!opened) {
            // On a message from a search, or on the unread divider, else at the end.
            // (A message from a search is scrolled to once its page is laid out, below.)
            val d = dividerBefore
            when {
                highlight != null -> Unit
                d != null -> list.scrollToItem(rowOf(model, chat, d, d) - 1)
                else -> list.scrollToItem(rows - 1)
            }
            opened = true
        } else if (last.id != lastSeen) {
            if (atBottom || last.outgoing) {
                list.animateScrollToItem(rows - 1)
                unseen = 0
            } else {
                unseen++
            }
        }
        lastSeen = last.id
        model.markRead(chat)
    }
    LaunchedEffect(atBottom) { if (atBottom) unseen = 0 }
    // A request for the full variant of our photo grows its bubble (§13.3): followed like a new
    // message when we were at the bottom, so its buttons are on screen.
    val fullAsked = model.fullRequests[chat]?.size ?: 0
    LaunchedEffect(fullAsked) {
        if (opened && fullAsked > 0 && atBottom) list.animateScrollToItem(rows - 1)
    }
    // A message from a search: once the earlier pages holding it are in the list.
    LaunchedEffect(highlight, messages.size) {
        val h = highlight ?: return@LaunchedEffect
        if (messages.none { it.id == h }) return@LaunchedEffect
        list.scrollToItem(maxOf(0, rowOf(model, chat, h, dividerBefore) - 2))
    }
    LaunchedEffect(highlight) {
        if (highlight != null) {
            delay(2_000)
            highlight = null
        }
    }

    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = {
                IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
            },
            title = {
                // A contact's name: tap to rename it on this phone.
                Column(if (chat.nodeKey == null) Modifier else Modifier.clickable { renaming = true }) {
                    Text(model.title(chat), fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                    Text(subtitle(model, chat), fontSize = 12.sp, color = t.tx2)
                }
            },
            actions = {
                TimerMenu(model, chat) { error = it }
                MoreMenu(model, chat, block = { blocking = true }, archived = back, media = media)
                if (chat.group) {
                    IconButton(onClick = details) { Icon(Icons.Outlined.Info, stringResource(R.string.group_details), tint = t.tx) }
                } else if (chat.plain) {
                    IconButton(onClick = { plainInfo = true }) {
                        Icon(
                            Icons.Outlined.Info,
                            stringResource(
                                when {
                                    chat.channel -> R.string.channel_details
                                    chat.kind == ChatId.Kind.ROOM -> R.string.room_details
                                    else -> R.string.about_meshcore_chats
                                },
                            ),
                            tint = t.tx,
                        )
                    }
                } else {
                    // Their key, always within reach: to verify, verified, or changed.
                    val c = chat.contactKey?.let { model.contact(it) }
                    val pending = model.conflicts[chat.hex]
                    if (c?.identity != null) {
                        IconButton(onClick = { verifying = pending ?: ByteArray(0) }) {
                            when {
                                pending != null -> Icon(Icons.Outlined.GppMaybe, stringResource(R.string.key_changed_compare), tint = t.sos)
                                c.verified -> Icon(Icons.Outlined.Lock, stringResource(R.string.key_verified), tint = t.tx2)
                                else -> Icon(Icons.Outlined.LockOpen, stringResource(R.string.verify_their_key), tint = t.warn)
                            }
                        }
                    }
                }
            },
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = t.s1, titleContentColor = t.tx, navigationIconContentColor = t.tx,
            ),
        )
        // A new key waiting for the user stays in sight, above the messages.
        if (chat.contactKey != null && model.conflicts.containsKey(chat.hex)) {
            Box(Modifier.padding(horizontal = 14.dp).padding(top = 8.dp)) {
                Banner(
                    BannerKind.SOS,
                    stringResource(R.string.key_changed_title, model.title(chat)),
                    stringResource(R.string.key_changed_note),
                    stringResource(R.string.compare) to { model.conflicts[chat.hex]?.let { verifying = it } },
                )
            }
        }
        Box(Modifier.weight(1f)) {
        LazyColumn(
            Modifier.fillMaxSize(),
            state = list,
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            model.historyError[chat]?.let { e ->
                item(key = "history-error") {
                    Banner(BannerKind.WARN, e, action = stringResource(R.string.try_again) to { model.load(chat) })
                }
            }
            if (model.olderAvailable[chat] == true) {
                item(key = "older") {
                    TextButton({
                        // Stay on the message that was at the top (once the page is laid out).
                        keepAt = messages.firstOrNull()?.id
                        model.loadOlder(chat)
                    }, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.earlier_messages), color = t.tx, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
            item(key = "session") { SessionLine(model, chat, { plainInfo = true }) { verifying = it } }
            messages.forEachIndexed { i, m ->
                // A separator when the day changes: "Today", "Yesterday", the date.
                if (i == 0 || dayOf(messages[i - 1].atMs) != dayOf(m.atMs)) {
                    item(key = "day" + dayOf(m.atMs)) { DaySeparator(context, m.atMs) }
                }
                if (m.id == dividerBefore) item(key = "unread") { UnreadDivider(unreadAtOpen) }
                val notice = m.message as? Message.Notice
                if (notice != null) {
                    item(key = m.id) { NoticeLine(model, notice) }
                    return@forEachIndexed
                }
                // Shown inside its LoRa preview's card: an empty row keeps its place (rowOf).
                val announce = m.message as? Message.Announce
                if (announce != null && model.preview(announce.mediaId, chat) != null) {
                    item(key = m.id) {}
                    return@forEachIndexed
                }
                item(key = m.id) {
                    MessageBubble(
                        model, m, chat.group || chat.channel || chat.kind == ChatId.Kind.ROOM, highlight == m.id,
                        reply = { replyTo = m; editing = null },
                        edit = { editing = m; replyTo = null },
                        // A quote tapped: its original, loading earlier pages if needed.
                        openQuote = { id ->
                            if (model.loadUntil(chat, id)) highlight = id else error = noLongerHere
                        },
                    ) { error = it }
                }
            }
            val failed = messages.lastOrNull { it.outgoing }?.takeIf { it.status is MessageStatus.Failed }
            if (failed != null) {
                item { FailureBanner(model, failed) { error = model.retry(failed) } }
            }
        }
            if (unseen > 0) {
                Row(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = 10.dp)
                        .heightIn(min = 44.dp)
                        .clip(RoundedCornerShape(50))
                        .background(t.actBg)
                        .clickable {
                            scope.launch { list.animateScrollToItem(rows - 1) }
                            unseen = 0
                        }
                        .padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Icon(Icons.Filled.ArrowDownward, null, tint = t.actFg, modifier = Modifier.size(18.dp))
                    Text(pluralStringResource(R.plurals.new_messages, unseen, unseen), color = t.actFg, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                }
            }
        }
        val blockedNode = chat.nodeKey?.takeIf { model.isBlocked(it) }
        if (blockedNode != null) {
            // Nothing to write with: they are blocked (ADR-0034).
            BlockedBar(model.title(chat)) { model.unblock(blockedNode) }
        } else {
            Composer(model, chat, replyTo, { replyTo = null }, editing, { editing = null }) { error = it }
        }
    }
    if (blocking) {
        AlertDialog(
            onDismissRequest = { blocking = false },
            title = { Text(stringResource(R.string.block_q, model.title(chat))) },
            text = { Text(stringResource(R.string.block_note)) },
            confirmButton = {
                TextButton(onClick = {
                    blocking = false
                    chat.nodeKey?.let { model.block(it) }?.let { error = it }
                }) { Text(stringResource(R.string.block), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { blocking = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }

    if (renaming) chat.nodeKey?.let { RenameDialog(model, it) { renaming = false } }
    if (plainInfo) PlainInfoSheet(model, chat, { plainInfo = false }) { plainInfo = false; back() }
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

/** Media, archive and block (ADR-0034); archiving goes back to the list (`archived`). */
@Composable
private fun MoreMenu(model: AppModel, chat: ChatId, block: () -> Unit, archived: () -> Unit, media: () -> Unit) {
    val t = lichen
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) { Icon(Icons.Outlined.MoreVert, stringResource(R.string.more), tint = t.tx) }
        DropdownMenu(open, { open = false }, containerColor = t.s1) {
            // A MeshCore chat carries text only (§15).
            if (!chat.plain) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.media_files_voice)) },
                    leadingIcon = { Icon(Icons.Outlined.Photo, null) },
                    onClick = {
                        open = false
                        media()
                    },
                )
            }
            val isArchived = model.isArchived(chat)
            DropdownMenuItem(
                text = { Text(stringResource(if (isArchived) R.string.unarchive else R.string.archive)) },
                leadingIcon = { Icon(if (isArchived) Icons.Outlined.Unarchive else Icons.Outlined.Archive, null) },
                onClick = {
                    open = false
                    model.setArchived(chat, !isArchived)
                    if (!isArchived) archived()
                },
            )
            chat.nodeKey?.let { node ->
                if (model.isBlocked(node)) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.unblock)) },
                        leadingIcon = { Icon(Icons.Outlined.Block, null) },
                        onClick = {
                            open = false
                            model.unblock(node)
                        },
                    )
                } else {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.block), color = t.sos) },
                        leadingIcon = { Icon(Icons.Outlined.Block, null, tint = t.sos) },
                        onClick = {
                            open = false
                            block()
                        },
                    )
                }
            }
        }
    }
}

/** In place of the composer for someone the user blocked (ADR-0034). */
@Composable
private fun BlockedBar(name: String, unblock: () -> Unit) {
    val t = lichen
    Column(
        Modifier
            .fillMaxWidth()
            .background(t.s1)
            .windowInsetsPadding(WindowInsets.navigationBars)
            .padding(14.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(stringResource(R.string.you_blocked, name), fontSize = 13.sp, color = t.tx2, textAlign = TextAlign.Center)
        SecondaryButton(stringResource(R.string.unblock), onClick = unblock)
    }
}

/** The list row of message `id` (the rows: history state, session line, a separator per day,
 * the unread divider before message `divider`, the messages). */
private fun rowOf(model: AppModel, chat: ChatId, id: Long, divider: Long?): Int {
    val messages = model.chats[chat].orEmpty()
    var row = (if (model.historyError[chat] != null) 1 else 0) + (if (model.olderAvailable[chat] == true) 1 else 0) + 1
    for ((i, m) in messages.withIndex()) {
        if (i == 0 || dayOf(messages[i - 1].atMs) != dayOf(m.atMs)) row++
        if (m.id == divider) row++
        if (m.id == id) return row
        row++
    }
    return row
}

@Composable
private fun subtitle(model: AppModel, chat: ChatId): String {
    val res = LocalResources.current
    if (chat.channel) return model.channel(chat)?.let { channelKindLabel(res, it.kind) } ?: stringResource(R.string.meshcore_channel)
    if (chat.kind == ChatId.Kind.MESH) return stringResource(R.string.meshcore_hops, hopsBadge(res, model.contact(chat.hex.unhex())?.hops))
    if (chat.kind == ChatId.Kind.ROOM) return roomSubtitle(model, chat)
    if (!chat.group) {
        val key = chat.contactKey ?: return ""
        val c = model.contact(key)
        // Renamed: the node's own name first.
        val hops = listOfNotNull(c?.nodeName?.takeIf { it != c.name }, hopsBadge(res, c?.hops)).joinToString(" · ")
        // The signal is a radio detail (Settings › Show radio details).
        val snr = model.lastSnr[chat.hex]?.takeIf { model.radioDetails } ?: return hops
        return stringResource(R.string.hops_snr, hops, "%.0f".format(snr))
    }
    val g = model.group(chat) ?: return ""
    // What it is and how many; the flood and the keys are radio details.
    if (!model.radioDetails) return pluralStringResource(R.plurals.group_members, g.members.size, g.members.size)
    val others = maxOf(g.members.size - 1, 0)
    return if (g.keysKnown.toInt() < others) {
        stringResource(R.string.group_keys, g.members.size, g.keysKnown.toInt(), others)
    } else {
        pluralStringResource(R.plurals.group_members_flood, g.members.size, g.members.size)
    }
}

@Composable
private fun SessionLine(model: AppModel, chat: ChatId, explain: () -> Unit, verify: (ByteArray) -> Unit) {
    val t = lichen
    // Not lichen-encrypted, said where the lock would be (§15.6).
    if (chat.plain) {
        Box(Modifier.fillMaxWidth().clickable(onClick = explain)) {
            TintedLine(
                Icons.Outlined.NoEncryption, t.warn,
                stringResource(
                    when {
                        chat.channel -> R.string.meshcore_channel_not_lichen
                        chat.kind == ChatId.Kind.ROOM -> R.string.meshcore_room_not_lichen
                        else -> R.string.meshcore_not_lichen
                    },
                ),
                t.warnFg,
            )
        }
        return
    }
    if (chat.group) {
        TintedLine(Icons.Outlined.Lock, t.ok, stringResource(R.string.group_signed), t.tx2)
        return
    }
    val key = chat.contactKey ?: return
    val c = model.contact(key)
    when {
        // Pinned above the list instead.
        model.conflicts.containsKey(chat.hex) -> Unit
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
    val action = if (m.message is Message.Text || m.message is Message.Voice) stringResource(R.string.resend) to resend else null
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
        FailReason.EXPIRED -> Banner(BannerKind.WARN, stringResource(R.string.expired), stringResource(R.string.expired_note), action)
    }
}

/** "3 unread messages", before the first one that was unread when the chat opened. */
@Composable
private fun UnreadDivider(count: Int) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp).semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(Modifier.weight(1f).height(1.dp).background(t.line))
        Text(pluralStringResource(R.plurals.unread_messages, count, count), fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx2)
        Box(Modifier.weight(1f).height(1.dp).background(t.line))
    }
}

@Composable
private fun MessageBubble(
    model: AppModel,
    m: StoredMessage,
    isGroup: Boolean,
    highlighted: Boolean,
    reply: () -> Unit,
    edit: () -> Unit,
    openQuote: (Long) -> Unit,
    onError: (String) -> Unit,
) {
    val t = lichen
    val res = LocalResources.current
    val author = m.author
    /** Deleting for everyone (true) or for this phone (false), to confirm (§6.4). */
    var deleting by remember { mutableStateOf<Boolean?>(null) }
    var forwarding by remember { mutableStateOf(false) }
    val status = m.status
    val text = (m.message as? Message.Text)?.text
    // Long press: copy or select a text; cancel one that is still waiting, send a failed one
    // again (§6.1).
    val cancellable = status is MessageStatus.Waiting && !status.started
    val retriable = m.outgoing && status is MessageStatus.Failed && (m.message is Message.Text || m.message is Message.Voice)
    var menu by remember { mutableStateOf(false) }
    var selecting by remember { mutableStateOf(false) }
    /** Long press › Info: when, how it went, fragments, airtime, signal. */
    var info by remember { mutableStateOf(false) }
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    // Every message can at least be deleted from this phone.
    val press = Modifier.combinedClickable(onClick = {}, onLongClick = { menu = true })
    // A text reads as one: author, text, time and status. Media keep their buttons.
    val merge = if (text != null) Modifier.semantics(mergeDescendants = true) {} else Modifier
    Column(
        Modifier
            .fillMaxWidth()
            .background(if (highlighted) t.warnBg else Color.Transparent, RoundedCornerShape(20.dp))
            .padding(start = if (m.outgoing) 64.dp else 0.dp, end = if (m.outgoing) 0.dp else 64.dp)
            .then(press)
            .then(merge),
        horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        DropdownMenu(menu, { menu = false }, containerColor = t.s1) {
            if (m.referable) {
                // React (§6.2): six choices, what it costs, ours again removes it.
                val mine = m.reactions.firstOrNull { it.mine }?.emoji
                Row(Modifier.padding(horizontal = 8.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                    ReactionChoices.forEach { e ->
                        Box(
                            Modifier
                                .size(44.dp)
                                .clip(CircleShape)
                                .background(if (e == mine) t.s2 else Color.Transparent)
                                .clickable {
                                    menu = false
                                    model.react(m, if (e == mine) "" else e)?.let(onError)
                                },
                            contentAlignment = Alignment.Center,
                        ) { Text(e, fontSize = 22.sp) }
                    }
                }
                model.reactionCost(ChatId.of(m.chat))?.let { cost ->
                    Text(
                        stringResource(R.string.reaction_cost, secondsText(cost.airtimeMs)),
                        fontSize = 11.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp),
                    )
                }
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.reply)) },
                    onClick = {
                        menu = false
                        reply()
                    },
                )
            }
            if (text != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.copy)) },
                    onClick = {
                        menu = false
                        scope.launch { clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("lichen", text))) }
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.select_text)) },
                    onClick = {
                        menu = false
                        selecting = true
                    },
                )
                // Coordinates in the text (ADR-0034).
                places(text).firstOrNull()?.let { (lat, lon) ->
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.show_on_map)) },
                        leadingIcon = { Icon(Icons.Outlined.Map, null) },
                        onClick = {
                            menu = false
                            model.showOnMap(lat, lon)
                        },
                    )
                }
            }
            if (cancellable) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.cancel_sending)) },
                    onClick = {
                        menu = false
                        model.cancel(m)?.let(onError)
                    },
                )
            }
            if (retriable) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.send_again)) },
                    onClick = {
                        menu = false
                        model.retry(m)?.let(onError)
                    },
                )
            }
            // A new message of ours with the same content (ADR-0027).
            if (forwardable(m)) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.forward)) },
                    leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Forward, null) },
                    onClick = {
                        menu = false
                        forwarding = true
                    },
                )
            }
            // Ours within 24 hours (§6.4).
            if (m.editable) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.edit)) },
                    leadingIcon = { Icon(Icons.Outlined.Edit, null) },
                    onClick = {
                        menu = false
                        edit()
                    },
                )
            }
            if (m.deletable) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.delete_for_everyone), color = t.sos) },
                    leadingIcon = { Icon(Icons.Outlined.Delete, null, tint = t.sos) },
                    onClick = {
                        menu = false
                        deleting = true
                    },
                )
            }
            DropdownMenuItem(
                text = { Text(stringResource(R.string.message_info)) },
                leadingIcon = { Icon(Icons.Outlined.Info, null) },
                onClick = {
                    menu = false
                    info = true
                },
            )
            DropdownMenuItem(
                text = { Text(stringResource(R.string.delete_for_me), color = t.sos) },
                leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = t.sos) },
                onClick = {
                    menu = false
                    deleting = false
                },
            )
        }
        if (forwarding) ForwardSheet(model, m) { forwarding = false }
        if (info) MessageInfoDialog(m, isGroup) { info = false }
        deleting?.let { everyone ->
            AlertDialog(
                onDismissRequest = { deleting = null },
                title = { Text(stringResource(if (everyone) R.string.delete_for_everyone_q else R.string.delete_for_me_q)) },
                text = { Text(stringResource(if (everyone) R.string.delete_for_everyone_note else R.string.delete_for_me_note)) },
                confirmButton = {
                    TextButton(onClick = {
                        deleting = null
                        (if (everyone) model.deleteForEveryone(m) else model.deleteForMe(m))?.let(onError)
                    }) { Text(stringResource(if (everyone) R.string.delete_for_everyone else R.string.delete_for_me), color = t.sos) }
                },
                dismissButton = { TextButton(onClick = { deleting = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
                containerColor = t.s1,
            )
        }
        // A group member, or a channel post's author as its radio calls itself (§15.1).
        val authorName = m.authorName ?: author?.let { model.name(it) }
        if (isGroup && !m.outgoing && authorName != null) {
            Text(
                authorName, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx2,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
        }
        m.quote?.let { q ->
            QuoteBox(model, q, ChatId.of(m.chat), Modifier.clickable(enabled = q.messageId != null) { q.messageId?.let(openQuote) })
        }
        if (m.message is Message.Deleted) {
            // Taken back by its author (§6.4): its place stays.
            Row(
                Modifier
                    .border(1.dp, t.line, RoundedCornerShape(18.dp))
                    .padding(horizontal = 14.dp, vertical = 9.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(Icons.Outlined.Delete, null, tint = t.tx2, modifier = Modifier.size(16.dp))
                Text(stringResource(R.string.message_deleted), fontSize = 16.sp, color = t.tx2, fontStyle = FontStyle.Italic)
            }
        } else if (!MediaContent(model, m)) {
            val color = if (m.outgoing) t.actFg else t.tx
            Text(
                // Links open in the browser when tapped, places on the map (ADR-0034).
                if (text != null) linked(model, text, color) else AnnotatedString(m.message.display(res)),
                fontSize = 16.sp,
                color = color,
                modifier = Modifier
                    .background(if (m.outgoing) t.actBg else t.s2, RoundedCornerShape(18.dp))
                    .padding(horizontal = 14.dp, vertical = 9.dp),
            )
        }
        if (m.reactions.isNotEmpty()) ReactionChips(model, m, onError)
        if (m.outgoing && !isGroup && status is MessageStatus.Sending && status.total.toInt() > 1) {
            // A long message's progress; its fragments are a radio detail.
            if (model.radioDetails) {
                FragmentProgress(status.sent.toInt(), status.total.toInt(), Modifier.width(220.dp).padding(top = 2.dp))
            } else {
                Bar(status.sent.toFloat() / status.total.toFloat(), Modifier.width(120.dp).padding(top = 2.dp))
            }
        }
        // When it was sent (ours) or received, then how it went: a word, or with the radio
        // details, the fragments and the airtime.
        val context = LocalContext.current
        val time = timeOf(context, m.atMs)
        val written = m.writtenEarlierMs
        val edited = if (m.edited) stringResource(R.string.edited) else null
        val footer = when {
            m.outgoing -> listOfNotNull(time, edited, if (model.radioDetails) caption(m, isGroup) else statusWord(m))
                .joinToString(" · ")
            edited != null -> "$time · $edited"
            // Lost on the air and handed on by a member (§10.6): shown where it arrived.
            m.late -> stringResource(R.string.arrived_late, time, timeOf(context, m.writtenAtMs ?: m.atMs))
            // Written well before it arrived: it waited (§6.1).
            written != null && dayOf(written) == dayOf(m.atMs) -> stringResource(R.string.written_at, time, timeOf(context, written))
            written != null -> stringResource(R.string.written_on, time, dateTimeOf(context, written))
            else -> time
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            // Decorative: the footer says it waits ("Waiting", or why).
            if (m.isWaiting) Icon(Icons.Outlined.Schedule, null, tint = t.tx2, modifier = Modifier.size(12.dp))
            Text(footer, fontSize = 11.sp, color = t.tx2)
        }
    }
    if (selecting && text != null) SelectTextDialog(text) { selecting = false }
}

/** A message's text with its links and places tappable (ADR-0034): a link opens in the
 * browser when the user taps it (nothing is fetched before, no preview), a place on lichen's
 * map. Only `http` and `https` addresses are links. */
@Composable
private fun linked(model: AppModel, text: String, color: Color): AnnotatedString = remember(text, color) {
    buildAnnotatedString {
        append(text)
        val style = TextLinkStyles(SpanStyle(color = color, textDecoration = TextDecoration.Underline))
        for (l in textLinks(text)) {
            val start = l.start.toInt()
            val end = l.end.toInt()
            if (start < 0 || end > text.length || start >= end) continue
            when (val target = l.target) {
                is TextTarget.Web -> addLink(LinkAnnotation.Url(target.url, style), start, end)
                is TextTarget.Place ->
                    addLink(LinkAnnotation.Clickable("place", style) { model.showOnMap(target.lat, target.lon) }, start, end)
            }
        }
    }
}

/** The places written in `text`. */
private fun places(text: String): List<Pair<Double, Double>> =
    textLinks(text).mapNotNull { (it.target as? TextTarget.Place)?.let { p -> p.lat to p.lon } }

/** The reactions offered (§6.2). */
private val ReactionChoices = listOf("👍", "❤️", "😂", "😮", "😢", "🙏")

/** What a reply answers (§6.2): its author and the start of it, or that it is not here. */
@Composable
private fun QuoteBox(model: AppModel, q: Quote, chat: ChatId, modifier: Modifier = Modifier) {
    val t = lichen
    val author = when {
        q.outgoing -> stringResource(R.string.you)
        q.author != null -> model.name(q.author!!)
        else -> model.title(chat)
    }
    val excerpt = when {
        q.messageId == null -> stringResource(R.string.not_on_this_phone)
        q.kind == QuoteKind.TEXT -> q.excerpt
        q.kind == QuoteKind.VOICE -> stringResource(R.string.voice_message)
        q.kind == QuoteKind.PHOTO -> stringResource(R.string.photo)
        q.kind == QuoteKind.FILE -> stringResource(R.string.file_named, q.excerpt)
        q.kind == QuoteKind.STICKER -> stringResource(R.string.sticker_named, q.excerpt)
        q.kind == QuoteKind.GIF -> if (q.excerpt.isEmpty()) stringResource(R.string.gif) else stringResource(R.string.gif_named, q.excerpt)
        else -> stringResource(R.string.a_message)
    }
    Row(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(t.s1)
            .border(1.dp, t.line, RoundedCornerShape(12.dp))
            .height(IntrinsicSize.Min)
            .padding(horizontal = 10.dp, vertical = 6.dp)
            .semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(t.tx2, RoundedCornerShape(2.dp)))
        Column {
            if (q.messageId != null) Text(author, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            Text(excerpt, fontSize = 12.sp, color = t.tx2, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** The reactions to a message, by emoji; tapping one adds or removes ours. */
@Composable
private fun ReactionChips(model: AppModel, m: StoredMessage, onError: (String) -> Unit) {
    val t = lichen
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        m.reactions.forEach { r ->
            Text(
                if (r.count.toInt() > 1) "${r.emoji} ${r.count}" else r.emoji,
                fontSize = 14.sp,
                color = t.tx,
                modifier = Modifier
                    .clip(CircleShape)
                    .background(t.s2)
                    .border(if (r.mine) 1.5.dp else 1.dp, if (r.mine) t.tx else t.line, CircleShape)
                    .clickable { model.react(m, if (r.mine) "" else r.emoji)?.let(onError) }
                    .heightIn(min = 32.dp)
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            )
        }
    }
}

/** A message's text, to select part of it. */
@Composable
private fun SelectTextDialog(text: String, close: () -> Unit) {
    val t = lichen
    AlertDialog(
        onDismissRequest = close,
        title = { Text(stringResource(R.string.select_text)) },
        text = {
            SelectionContainer(Modifier.verticalScroll(rememberScrollState())) {
                Text(text, fontSize = 16.sp, color = t.tx)
            }
        },
        confirmButton = { TextButton(onClick = close) { Text(stringResource(R.string.done), color = t.tx) } },
        containerColor = t.s1,
    )
}

/** How one of ours went, in a word: the details are in long press › Info. */
@Composable
private fun statusWord(m: StoredMessage): String? = when (val s = m.status) {
    MessageStatus.Received -> null
    is MessageStatus.Waiting -> stringResource(if (s.reason == WaitReason.Radio && s.started) R.string.state_paused else R.string.state_waiting)
    is MessageStatus.Sending -> stringResource(R.string.state_sending)
    MessageStatus.Delivered -> stringResource(R.string.state_delivered)
    is MessageStatus.Failed -> stringResource(R.string.state_not_delivered)
    MessageStatus.Flooded -> stringResource(R.string.state_sent)
}

/** Long press › Info: when it was sent or received (and written, when earlier), how it went,
 * its fragments, airtime and signal. */
@Composable
private fun MessageInfoDialog(m: StoredMessage, isGroup: Boolean, close: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val res = LocalResources.current
    val rows = buildList {
        add(stringResource(if (m.outgoing) R.string.info_sent else R.string.info_received) to dateTimeOf(context, m.atMs))
        val written = if (m.late) m.writtenAtMs else m.writtenEarlierMs
        if (written != null && written != m.atMs) add(stringResource(R.string.info_written) to dateTimeOf(context, written))
        // Delivered in a word: its fragments and airtime have their own rows.
        (if (m.status == MessageStatus.Delivered) statusWord(m) else caption(m, isGroup))?.let {
            add(stringResource(R.string.info_status) to it)
        }
        if (m.late) add(stringResource(R.string.info_status) to stringResource(R.string.info_arrived_late))
        if (m.fragments.toInt() > 0) add(stringResource(R.string.info_fragments) to fragmentsText(res, m.fragments.toInt()))
        if (m.airtimeMs > 0u) add(stringResource(R.string.info_airtime) to stringResource(R.string.seconds_value, secondsText(m.airtimeMs)))
        m.snrDb?.let { add(stringResource(R.string.info_signal) to stringResource(R.string.snr_value, "%.0f".format(it))) }
        if (m.edited) add(stringResource(R.string.info_edited) to stringResource(R.string.yes))
    }
    AlertDialog(
        onDismissRequest = close,
        title = { Text(stringResource(R.string.message_info)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                rows.forEach { (label, value) ->
                    Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(label, fontSize = 14.sp, color = t.tx2, modifier = Modifier.width(110.dp))
                        Text(value, fontSize = 14.sp, color = t.tx, modifier = Modifier.weight(1f))
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = close) { Text(stringResource(R.string.done), color = t.tx) } },
        containerColor = t.s1,
    )
}

@Composable
private fun caption(m: StoredMessage, isGroup: Boolean): String? {
    val res = LocalResources.current
    val context = LocalContext.current
    return when (val s = m.status) {
        MessageStatus.Received -> null
        is MessageStatus.Waiting -> when (val r = s.reason) {
            WaitReason.Radio ->
                if (s.started) {
                    stringResource(R.string.paused_radio)
                } else {
                    stringResource(R.string.waiting_for_radio, timeLeft(s.expiresAtMs))
                }
            WaitReason.Earlier -> stringResource(R.string.after_earlier)
            WaitReason.Path -> stringResource(R.string.looking_for_path)
            WaitReason.Handshake -> stringResource(R.string.opening_session)
            is WaitReason.Budget -> stringResource(R.string.waiting_for_airtime, timeOf(context, r.atMs))
        }
        is MessageStatus.Sending ->
            if (m.chat is ChatRef.Channel) {
                stringResource(R.string.sending_to_channel)
            } else if (m.chat is ChatRef.Mesh) {
                stringResource(R.string.waiting_for_radio_ack)
            } else if (m.chat is ChatRef.Room) {
                stringResource(R.string.waiting_room_ack)
            } else if (isGroup) {
                stringResource(R.string.sending_to_group)
            } else if (s.total.toInt() > 1) {
                stringResource(R.string.waiting_for_ack)
            } else {
                stringResource(R.string.sending_fragments, minOf(s.sent.toInt(), s.total.toInt()), s.total.toInt())
            }
        MessageStatus.Delivered -> if (m.chat is ChatRef.Mesh) {
            stringResource(R.string.delivered_airtime, secondsText(m.airtimeMs))
        } else if (m.chat is ChatRef.Room) {
            stringResource(R.string.kept_by_room, secondsText(m.airtimeMs))
        } else {
            pluralStringResource(
                R.plurals.delivered_fragments, m.fragments.toInt(), m.fragments.toInt(), secondsText(m.airtimeMs),
            )
        }
        is MessageStatus.Failed -> stringResource(R.string.not_delivered, s.reason.display(res))
        MessageStatus.Flooded ->
            stringResource(if (m.chat is ChatRef.Channel) R.string.sent_to_channel else R.string.sent_to_group)
    }
}

@Composable
private fun Composer(
    model: AppModel,
    chat: ChatId,
    replyTo: StoredMessage?,
    clearReply: () -> Unit,
    editing: StoredMessage?,
    clearEdit: () -> Unit,
    onError: (String) -> Unit,
) {
    val t = lichen
    val res = LocalResources.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // Kept per conversation, across screens and restarts.
    var draft by remember(chat) { mutableStateOf(model.draft(chat)) }
    // A recording stopped and not sent yet; it belongs to this conversation. One left behind
    // there comes back under review (ADR-0034).
    var review by remember(chat) {
        mutableStateOf(
            model.voiceDrafts.remove(chat)?.let {
                VoiceReview(it.pcm, it.mode, com.tduhamel.lichen.core.prepareVoice(it.pcm.toList(), it.mode.toUByte()))
            },
        )
    }
    val currentReview by androidx.compose.runtime.rememberUpdatedState(review)
    DisposableEffect(chat) {
        onDispose {
            if (model.player.playing == com.tduhamel.lichen.model.VoicePlayer.REVIEW) model.player.stop()
            currentReview?.let { model.voiceDrafts[chat] = AppModel.VoiceDraft(it.pcm, it.mode) }
        }
    }
    LaunchedEffect(draft) { model.setDraft(chat, draft) }
    var menu by remember { mutableStateOf(false) }
    var preparing by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<Pair<PreparedImage, ByteArray>?>(null) }
    var locating by remember { mutableStateOf(false) }
    // The sticker picker; a GIPHY link in the draft goes as a GIF unless asked otherwise (§6.3).
    var stickers by remember { mutableStateOf(false) }
    val gifLink = remember(draft, editing) { if (chat.plain || editing != null) null else com.tduhamel.lichen.core.gifLink(draft) }
    // Editing starts from the message's current text.
    LaunchedEffect(editing?.id) { (editing?.message as? Message.Text)?.let { draft = it.text } }
    // Bytes over a plain chat's limit (§15.2, §15.3); 0 within it.
    val plainMax = if (chat.plain) model.plainTextMax(chat) else 0
    val plainOver = if (chat.plain) maxOf(0, draft.toByteArray().size - plainMax) else 0
    var linkAsText by remember(chat) { mutableStateOf(false) }
    var gifData by remember { mutableStateOf<ByteArray?>(null) }
    LaunchedEffect(gifLink?.id) {
        if (gifLink == null) linkAsText = false
        gifData = null
        gifData = gifLink?.let { model.loadGif(it.id) }
    }
    // A file picked, waiting for its route (LoRa when small, else the bridge, ADR-0022).
    var file by remember { mutableStateOf<FileDraft?>(null) }
    val pickFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            // Never more than the bridge carries (recheck R04): the size the provider reports
            // first, then reading stops one byte past the limit whatever it said.
            val picked = withContext(Dispatchers.IO) { readPicked(context, uri) }
            when (picked) {
                null -> onError(res.getString(R.string.file_unreadable))
                TOO_LARGE -> onError(res.getString(R.string.file_too_large))
                else -> file = picked
            }
        }
    }
    // Debug extra `screen file` / `fileBig`: the file sheet with a 3 KB note, or a 244 KB document;
    // `screen media`: the photo sheet with a drawn landscape, in a contact's chat.
    LaunchedEffect(Unit) {
        val screen = model.debugScreen
        if (screen == "file" || screen == "fileBig") {
            val line = "Refuge de la Pra: code 4521, wood in the shed, water 200 m north.\n".toByteArray()
            val n = if (screen == "fileBig") 3_700 else 45
            val data = ByteArray(line.size * n) { line[it % line.size] }
            file = FileDraft(if (screen == "fileBig") "Topo Vercors 2026.pdf" else "Refuge notes.txt", data)
        }
        if (screen == "media" && chat.kind == ChatId.Kind.CONTACT) {
            picked = withContext(Dispatchers.Default) {
                val photo = com.tduhamel.lichen.model.samplePhoto()
                runCatching { prepareImage(photo.rgba, photo.width.toUInt(), photo.height.toUInt()) to photo.original }.getOrNull()
            }
        }
    }
    fun preparePhoto(uri: android.net.Uri) {
        preparing = true
        scope.launch {
            // The core encodes the AVIF thumbnails (a few seconds): off the main thread.
            val result = withContext(Dispatchers.Default) {
                val photo = pickPhoto(context.contentResolver, uri) ?: return@withContext null
                runCatching { prepareImage(photo.rgba, photo.width.toUInt(), photo.height.toUInt()) to photo.original }.getOrNull()
            }
            preparing = false
            if (result == null) onError(res.getString(R.string.err_bad_image)) else picked = result
        }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) preparePhoto(uri)
    }
    // Something shared into lichen for this conversation (ADR-0034): its text joins the draft;
    // each photo goes to the photo sheet, each file to the file sheet, one after the other, as
    // if picked here. File names are only labels: sanitised before anything shows them.
    val shared = model.sharedFor[chat]
    LaunchedEffect(shared, picked, file, preparing) {
        val s = shared ?: return@LaunchedEffect
        if (picked != null || file != null || preparing) return@LaunchedEffect
        s.text?.let { draft = if (draft.isBlank()) it else "$draft $it" }
        val uri = s.uris.firstOrNull()
        val rest = s.uris.drop(1)
        if (rest.isEmpty()) model.sharedFor.remove(chat) else model.sharedFor[chat] = com.tduhamel.lichen.model.IncomingShare(null, rest)
        if (uri == null) return@LaunchedEffect
        if (chat.plain) {
            model.sharedFor.remove(chat)
            onError(res.getString(R.string.plain_text_only))
            return@LaunchedEffect
        }
        val type = runCatching { context.contentResolver.getType(uri) }.getOrNull()
        if (type?.startsWith("image/") == true) {
            preparePhoto(uri)
        } else {
            // In the composer's scope: this effect starts again as the queue shrinks.
            preparing = true
            scope.launch {
                val read = withContext(Dispatchers.IO) { readPicked(context, uri) }
                preparing = false
                when (read) {
                    null -> onError(res.getString(R.string.file_unreadable))
                    TOO_LARGE -> onError(res.getString(R.string.file_too_large))
                    else -> file = FileDraft(com.tduhamel.lichen.core.fitFileName(read.name, 64u).ifEmpty { "file" }, read.data)
                }
            }
        }
    }
    val askMic = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (!ok) {
            onError(res.getString(R.string.mic_denied))
        } else if (!model.recorder.start(chat, if (chat.group) AppModel.GROUP_VOICE_SECONDS else VoiceRecorder.MAX_SECONDS)) {
            onError(res.getString(R.string.mic_unavailable))
        }
    }
    val bridgeAvailable = model.bridgeProviders(anonymous = false).isNotEmpty()
    val text = draft.trim()
    // Before every radio send: what it costs us. A group message is flooded: relays add their
    // own airtime, and nobody acknowledges it.
    val estimate = model.estimate(chat, draft)
    val cost = when {
        chat.plain && plainOver > 0 -> stringResource(R.string.plain_too_long, plainOver, plainMax)
        chat.plain && estimate == null -> if (chat.channel) {
            stringResource(R.string.channel_flooded_no_ack)
        } else {
            stringResource(R.string.plain_up_to, plainMax)
        }
        chat.plain && estimate != null ->
            stringResource(R.string.plain_bytes_cost, draft.toByteArray().size, plainMax, secondsText(estimate.airtimeMs))
        estimate == null -> if (chat.group) stringResource(R.string.flooded_to_every_member) else null
        chat.group -> stringResource(R.string.group_cost, fragmentsText(res, estimate.fragments.toInt()), secondsText(estimate.airtimeMs))
        else -> stringResource(R.string.estimated_cost, fragmentsText(res, estimate.fragments.toInt()), secondsText(estimate.airtimeMs))
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
        if (model.recorder.recording && model.recorder.chat == chat) {
            RecordingBar(model, chat, onReview = { pcm ->
                val mode = model.recordMode(chat)
                review = VoiceReview(pcm, mode, com.tduhamel.lichen.core.prepareVoice(pcm.toList(), mode.toUByte()))
            }, onError = onError)
            return@Column
        }
        review?.let { r ->
            ReviewBar(model, chat, r, change = { review = it }, onError = onError)
            return@Column
        }
        if (editing != null) {
            Row(Modifier.padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.Edit, null, tint = t.tx2, modifier = Modifier.size(18.dp))
                Text(
                    stringResource(R.string.editing_your_message), fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                )
                IconButton(onClick = { clearEdit(); draft = "" }) { Icon(Icons.Filled.Close, stringResource(R.string.cancel_edit), tint = t.tx2) }
            }
        }
        replyTo?.let { r ->
            val res2 = LocalResources.current
            val who = when {
                r.outgoing -> stringResource(R.string.replying_to_yourself)
                r.author != null -> stringResource(R.string.replying_to, model.name(r.author!!))
                else -> stringResource(R.string.replying_to, model.title(chat))
            }
            Row(Modifier.padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.AutoMirrored.Outlined.Reply, null, tint = t.tx2, modifier = Modifier.size(18.dp))
                Column(Modifier.weight(1f).padding(start = 8.dp)) {
                    Text(who, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(r.message.display(res2), fontSize = 12.sp, color = t.tx2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                IconButton(onClick = clearReply) { Icon(Icons.Filled.Close, stringResource(R.string.cancel_reply), tint = t.tx2) }
            }
        }
        if (gifLink != null && !linkAsText) {
            Row(Modifier.padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(64.dp, 48.dp).clip(RoundedCornerShape(8.dp)).background(t.s2), contentAlignment = Alignment.Center) {
                    val shown = gifData
                    if (shown != null) AnimatedImage(shown, Modifier.fillMaxSize()) else CircularProgressIndicator(Modifier.size(16.dp), color = t.tx2, strokeWidth = 2.dp)
                }
                Column(Modifier.weight(1f).padding(start = 10.dp)) {
                    Text(
                        if (gifLink.title.isEmpty()) stringResource(R.string.gif) else stringResource(R.string.gif_named, gifLink.title),
                        fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                    Text(stringResource(R.string.gif_link_note), fontSize = 12.sp, color = t.tx2, maxLines = 2)
                }
                IconButton(onClick = { linkAsText = true }) { Icon(Icons.Filled.Close, stringResource(R.string.gif_link_as_text), tint = t.tx2) }
            }
        }
        model.share(chat)?.let { share ->
            val until = timeOf(context, share.untilS.toLong() * 1000)
            Row(
                Modifier.padding(start = 8.dp).clickable { locating = true },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(Icons.Filled.LocationOn, null, tint = t.tx2, modifier = Modifier.size(14.dp))
                Text(stringResource(R.string.sharing_until, precisionName(share.precision), until), fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.tx2)
            }
        }
        if (chat.channel && !model.isConnected) {
            Text(stringResource(R.string.channel_needs_radio_now), fontSize = 11.sp, color = t.tx2, modifier = Modifier.padding(start = 8.dp))
        } else if (!model.isConnected) {
            // Texts wait for the radio (§6.1).
            Row(
                Modifier.padding(start = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Icon(Icons.Outlined.Schedule, null, tint = t.tx2, modifier = Modifier.size(12.dp))
                Text(stringResource(R.string.radio_offline_waits), fontSize = 11.sp, color = t.tx2)
            }
        } else if (cost != null) {
            Text(cost, fontSize = 11.sp, color = t.tx2, modifier = Modifier.padding(start = 8.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            // A MeshCore chat carries text only (§15).
            if (!chat.plain) Box {
                IconButton(
                    onClick = { menu = true },
                    enabled = !preparing,
                    modifier = Modifier.size(48.dp),
                    colors = IconButtonDefaults.iconButtonColors(containerColor = t.s2, contentColor = t.tx),
                ) {
                    if (preparing) {
                        CircularProgressIndicator(Modifier.size(18.dp), color = t.tx2, strokeWidth = 2.dp)
                    } else {
                        Icon(Icons.Filled.Add, stringResource(R.string.attach))
                    }
                }
                DropdownMenu(menu, { menu = false }, containerColor = t.s1) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.photo)) },
                        leadingIcon = { Icon(Icons.Outlined.Photo, null) },
                        enabled = (!chat.group || bridgeAvailable) && model.isConnected,
                        onClick = {
                            menu = false
                            pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.file)) },
                        leadingIcon = { Icon(Icons.AutoMirrored.Outlined.InsertDriveFile, null) },
                        enabled = !chat.group || bridgeAvailable,
                        onClick = {
                            menu = false
                            pickFile.launch(arrayOf("*/*"))
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.sticker)) },
                        leadingIcon = { Icon(Icons.Outlined.EmojiEmotions, null) },
                        onClick = {
                            menu = false
                            stickers = true
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.share_my_location)) },
                        leadingIcon = { Icon(Icons.Outlined.LocationOn, null) },
                        onClick = {
                            menu = false
                            locating = true
                        },
                    )
                }
            }
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
            // Voice notes: to a contact, or short ones to a group (ADR-0035).
            if ((chat.contactKey != null || chat.group) && draft.isEmpty()) {
                IconButton(
                    onClick = {
                        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
                            PackageManager.PERMISSION_GRANTED
                        if (!granted) {
                            askMic.launch(Manifest.permission.RECORD_AUDIO)
                        } else if (!model.recorder.start(chat, if (chat.group) AppModel.GROUP_VOICE_SECONDS else VoiceRecorder.MAX_SECONDS)) {
                            onError(res.getString(R.string.mic_unavailable))
                        }
                    },
                    enabled = model.isConnected,
                    modifier = Modifier.size(48.dp),
                ) { Icon(Icons.Outlined.Mic, stringResource(R.string.record_voice), tint = if (model.isConnected) t.lora else t.tx2) }
            } else {
                IconButton(
                    onClick = {
                        val link = gifLink
                        val e = when {
                            link != null && !linkAsText -> model.sendGif(link, gifData?.let { imageSize(it) }, chat)
                            editing != null -> model.edit(text, editing)
                            replyTo != null -> model.reply(text, replyTo)
                            else -> model.send(text, chat)
                        }
                        if (e != null) {
                            onError(e)
                        } else {
                            draft = ""
                            clearReply()
                            clearEdit()
                        }
                    },
                    // Texts wait for the radio (§6.1); not with a throwaway identity.
                    enabled = text.isNotEmpty() && model.identityProblem == null && plainOver == 0 &&
                        !(chat.channel && !model.isConnected),
                    modifier = Modifier.size(48.dp),
                    colors = IconButtonDefaults.iconButtonColors(
                        containerColor = t.actBg, contentColor = t.actFg,
                        disabledContainerColor = t.s2, disabledContentColor = t.tx2,
                    ),
                ) { Icon(Icons.Filled.ArrowUpward, stringResource(R.string.send)) }
            }
        }
    }
    if (locating) ChatShareSheet(model, chat) { locating = false }
    if (stickers) StickerSheet({ emoji -> model.sendSticker(emoji, chat)?.let(onError) }) { stickers = false }
    file?.let { draft ->
        SendFileSheet(model, chat, draft) { e ->
            file = null
            e?.let(onError)
        }
    }
    picked?.let { (image, original) ->
        SendPhotoSheet(model, chat, image, original) { e ->
            picked = null
            e?.let(onError)
        }
    }
}

/** "24 h", "35 min": how long a waiting text still has before it expires (§6.1). */
private fun timeLeft(expiresAtMs: Long): String {
    val s = maxOf(0L, expiresAtMs - System.currentTimeMillis()) / 1000.0
    return if (s >= 3600) "${kotlin.math.ceil(s / 3600).toInt()} h" else "${maxOf(1, kotlin.math.ceil(s / 60).toInt())} min"
}

/** A membership change in a group (ADR-0021): "Léa joined", "Camille removed Bob". */
@Composable
private fun NoticeLine(model: AppModel, n: Message.Notice) {
    val res = LocalContext.current.resources
    Text(
        n.noticeText(res, model.nodeKey) { node, name -> model.contact(node)?.name ?: name },
        Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 4.dp),
        fontSize = 12.sp,
        color = lichen.tx2,
        textAlign = TextAlign.Center,
    )
}

/** Disappearing messages (§6.5): the choices, and what the timer cannot do. */
@Composable
private fun TimerMenu(model: AppModel, chat: ChatId, failed: (String) -> Unit) {
    val t = lichen
    var open by remember { mutableStateOf(false) }
    val current = model.timers[chat] ?: 0u
    Box {
        IconButton(onClick = { open = true }) {
            Icon(
                if (current == 0u) Icons.Outlined.Timer else Icons.Filled.Timer,
                if (current == 0u) stringResource(R.string.disappearing_off)
                else stringResource(R.string.disappearing_on, timerLabel(LocalContext.current.resources, current)),
                tint = if (current == 0u) t.tx2 else t.tx,
            )
        }
        DropdownMenu(open, onDismissRequest = { open = false }, containerColor = t.s1) {
            Text(
                stringResource(R.string.disappearing_messages),
                Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.tx2,
            )
            val res = LocalContext.current.resources
            AppModel.TIMER_CHOICES.forEach { s ->
                DropdownMenuItem(
                    text = { Text(if (s == 0u) stringResource(R.string.off) else timerLabel(res, s), color = t.tx) },
                    onClick = {
                        open = false
                        model.setTimer(chat, s)?.let(failed)
                    },
                    trailingIcon = { if (s == current) Icon(Icons.Filled.Check, stringResource(R.string.chosen_option), tint = t.tx) },
                )
            }
            HorizontalDivider(color = t.line)
            Text(
                stringResource(if (chat.plain) R.string.timer_note_plain else R.string.timer_note),
                Modifier.padding(horizontal = 16.dp, vertical = 8.dp).widthIn(max = 280.dp),
                fontSize = 13.sp, color = t.tx2,
            )
        }
    }
}
