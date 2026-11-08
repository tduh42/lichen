package com.tduhamel.lichen.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.clickable
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.core.GroupInfo
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.searchConsent
import com.tduhamel.lichen.model.setSearchConsent
import com.tduhamel.lichen.model.startSearch

/** A group: members and their keys, diffusion (mockup `groupe`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
/** `left`: after leaving the group, back to the conversations. */
fun GroupScreen(model: AppModel, chat: ChatId, back: () -> Unit, left: () -> Unit = back) {
    val t = lichen
    val g = model.group(chat)
    var removing by remember { mutableStateOf<ContactInfo?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var consent by remember(g?.gid?.hex()) { mutableStateOf(g?.let { model.searchConsent(it.gid) } ?: false) }
    var inviting by remember { mutableStateOf(false) }
    var leaving by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = {
                Column {
                    Text(g?.name.orEmpty(), fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                    val n = g?.members?.size ?: 0
                    Text(pluralStringResource(R.plurals.group_members_flood, n, n), fontSize = 12.sp, color = t.tx2)
                }
            },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.s1, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        if (g == null) return
        val isAdmin = model.nodeKey?.contentEquals(g.admin) == true
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { KeysBanner(g) }
            item { SectionLabel(stringResource(R.string.members), Modifier.padding(top = 6.dp)) }
            item {
                CardList {
                    g.members.forEachIndexed { i, m ->
                        if (i > 0) CardDivider(inset = 68.dp)
                        MemberRow(model, m, g, removable = isAdmin, remove = { removing = m }) {
                            // Search for a member: the Search tab opens on them.
                            error = model.startSearch(m.key)
                            if (error == null) model.openSearch = m.key.hex()
                        }
                    }
                }
            }
            if (isAdmin) {
                item { Text(stringResource(R.string.remove_hint), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp)) }
            }
            item { SectionLabel(stringResource(R.string.location), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.let_group_locate), stringResource(R.string.let_group_locate_note)) {
                        LichenSwitch(consent, {
                            consent = it
                            model.setSearchConsent(g.gid, it)
                        })
                    }
                    CardDivider()
                    Box(Modifier.clickable { sharing = true }) {
                        CardRow(stringResource(R.string.precision_shared_with_group)) {
                            Text(
                                model.share(ChatId.group(g.gid))?.let { precisionName(it.precision) } ?: stringResource(R.string.not_shared),
                                fontSize = 15.sp, color = t.tx2,
                            )
                        }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.diffusion), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.hop_limit), stringResource(R.string.hop_limit_note)) {
                        Text("8", style = figure(17.sp), color = t.tx2)
                    }
                    CardDivider()
                    CardRow(stringResource(R.string.media), stringResource(R.string.media_never_flooded))
                }
            }
            item {
                Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (isAdmin) SecondaryButton(stringResource(R.string.invite), Modifier.weight(1f)) { inviting = true }
                    SecondaryButton(
                        stringResource(if (isAdmin) R.string.close_group else R.string.leave_group), Modifier.weight(1f),
                        background = t.sosBg, foreground = t.sosFg,
                    ) { leaving = true }
                }
            }
        }
    }
    if (sharing && g != null) ChatShareSheet(model, ChatId.group(g.gid)) { sharing = false }
    if (inviting && g != null) InviteDialog(model, g) { inviting = false }
    if (leaving && g != null) {
        val admin = model.nodeKey?.contentEquals(g.admin) == true
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text(stringResource(if (admin) R.string.close_group_q else R.string.leave_group_q)) },
            text = { Text(stringResource(if (admin) R.string.close_group_note else R.string.leave_group_note)) },
            confirmButton = {
                TextButton(onClick = {
                    leaving = false
                    error = model.leaveGroup(g.gid)
                    if (error == null) left()
                }) { Text(stringResource(if (admin) R.string.close_group else R.string.leave_group), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { leaving = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
    removing?.let { m ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.remove_from_group)) },
            text = { Text(m.name) },
            confirmButton = {
                TextButton(onClick = {
                    error = g?.let { model.removeMember(m.key, it.gid) }
                    removing = null
                }) { Text(stringResource(R.string.remove_from_group), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
    error?.let {
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text(stringResource(R.string.not_changed)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { error = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun KeysBanner(g: GroupInfo) {
    val others = maxOf(g.members.size - 1, 0)
    val known = g.keysKnown.toInt()
    if (known >= others) {
        Banner(BannerKind.OK, stringResource(R.string.sender_keys_up_to_date), stringResource(R.string.sender_keys_received, known, others))
    } else {
        Banner(BannerKind.WARN, stringResource(R.string.waiting_for_sender_keys), stringResource(R.string.sender_keys_waiting, known, others))
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MemberRow(model: AppModel, m: ContactInfo, g: GroupInfo, removable: Boolean, remove: () -> Unit, search: () -> Unit) {
    val t = lichen
    val me = model.nodeKey?.contentEquals(m.key) == true
    val admin = m.key.contentEquals(g.admin)
    val name = if (me) stringResource(R.string.you) else m.name
    var menu by remember { mutableStateOf(false) }
    DropdownMenu(menu, { menu = false }) {
        DropdownMenuItem({ Text(stringResource(R.string.search_for, m.name)) }, {
            menu = false
            search()
        })
        if (removable) {
            DropdownMenuItem({ Text(stringResource(R.string.remove_from_group), color = t.sos) }, {
                menu = false
                remove()
            })
        }
    }
    Row(
        Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = { if (!me) menu = true }, onLongClick = { if (removable && !me) remove() })
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(name, m.key, 40.dp)
        Column(Modifier.weight(1f)) {
            Text(name, fontSize = 16.sp, color = t.tx)
            Text(stringResource(if (admin) R.string.admin else R.string.member), fontSize = 13.sp, color = t.tx2)
        }
        if (!me) {
            if (m.verified) {
                Badge(stringResource(R.string.verified), t.okBg, t.okFg)
            } else {
                Badge(stringResource(R.string.to_verify), t.warnBg, t.warnFg)
            }
        }
    }
}

/** Invite contacts into the group (the admin): people whose identity is known, not yet in it. */
@Composable
private fun InviteDialog(model: AppModel, g: GroupInfo, dismiss: () -> Unit) {
    val t = lichen
    val members = g.members.map { it.key.hex() }.toSet()
    val candidates = model.contacts.filter { it.identity != null && it.key.hex() !in members }
    var picked by remember { mutableStateOf(setOf<String>()) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.invite)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (candidates.isEmpty()) Text(stringResource(R.string.invite_none), color = t.tx2)
                for (c in candidates) {
                    val k = c.key.hex()
                    Row(
                        Modifier.fillMaxWidth().clickable { picked = if (k in picked) picked - k else picked + k }.padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Checkbox(k in picked, { picked = if (it) picked + k else picked - k })
                        Text(c.name, color = t.tx)
                    }
                }
                if (candidates.isNotEmpty()) Text(stringResource(R.string.invite_note), fontSize = 12.sp, color = t.tx2)
                error?.let { Text(it, color = t.sosFg) }
            }
        },
        confirmButton = {
            TextButton(enabled = picked.isNotEmpty(), onClick = {
                error = model.addMembers(candidates.filter { it.key.hex() in picked }.map { it.key }, g.gid)
                if (error == null) dismiss()
            }) { Text(stringResource(R.string.invite), color = t.tx) }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text(stringResource(R.string.cancel), color = t.tx) } },
        containerColor = t.s1,
    )
}
