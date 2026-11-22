package com.tduhamel.lichen.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.GroupAdd
import androidx.compose.material.icons.outlined.Language
import androidx.compose.material.icons.outlined.QrCode2
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Tag
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ChannelKind
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.core.MeshChannel
import com.tduhamel.lichen.core.hashtagChannelName
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.hopsBadge
import com.tduhamel.lichen.model.nameOrder

/** Which people New conversation lists (F-4). */
private enum class PeopleFilter { ALL, LICHEN, MESHCORE }

/**
 * New conversation (F-4): search, three actions (New group, Join a channel, Add a contact), the
 * radio's channels, the groups, then the people, last: the radio may know hundreds of MeshCore
 * radios, and nothing else should sit below them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewChatSheet(
    model: AppModel,
    dismiss: () -> Unit,
    open: (ChatId) -> Unit,
    newGroup: () -> Unit = {},
    addContact: () -> Unit = {},
) {
    val t = lichen
    val res = LocalResources.current
    var query by remember { mutableStateOf("") }
    var filter by remember { mutableStateOf(PeopleFilter.ALL) }
    var error by remember { mutableStateOf<String?>(null) }
    var joining by remember { mutableStateOf(false) }
    val q = query.trim()
    fun matches(name: String) = q.isEmpty() || name.contains(q, ignoreCase = true)
    val people = model.contacts
        .filter { matches(it.name) }
        // lichen people first, then by name as people read it (accents and case aside).
        .sortedWith(compareBy<ContactInfo> { it.identity == null }.thenBy(nameOrder()) { it.name })
    val lichenCount = people.count { it.identity != null }
    val shown = when (filter) {
        PeopleFilter.ALL -> people
        PeopleFilter.LICHEN -> people.filter { it.identity != null }
        PeopleFilter.MESHCORE -> people.filter { it.identity == null }
    }
    // `#tag` typed: the hashtag channel's name as the radio keeps it, and the channel if it
    // follows it already.
    val hashtag = if (q.startsWith("#")) hashtagChannelName(q) else null
    val followed = hashtag?.let { tag -> model.channels.firstOrNull { it.name.equals(tag, ignoreCase = true) } }
    // Public first, then by name; not the `#tag` typed, shown once on top.
    val channels = model.channels
        .filter { matches(it.name) && !(followed != null && it.id.contentEquals(followed.id)) }
        .sortedWith(compareBy<MeshChannel> { it.kind != ChannelKind.PUBLIC_CHANNEL }.thenBy(nameOrder()) { it.name })
    val groups = model.groups.filter { matches(it.name) }

    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.new_conversation), fontSize = 22.sp, color = t.tx)
            TextField(
                query, { query = it },
                Modifier.fillMaxWidth(),
                singleLine = true,
                leadingIcon = { Icon(Icons.Outlined.Search, null) },
                placeholder = { Text(stringResource(R.string.search_new_chat)) },
                shape = RoundedCornerShape(24.dp),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = t.s1, unfocusedContainerColor = t.s1,
                    focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
                    focusedTextColor = t.tx, unfocusedTextColor = t.tx, cursorColor = t.tx,
                    focusedPlaceholderColor = t.tx2, unfocusedPlaceholderColor = t.tx2,
                    focusedLeadingIconColor = t.tx2, unfocusedLeadingIconColor = t.tx2,
                ),
            )
            LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (hashtag != null) {
                    item(key = "hashtag") {
                        if (followed != null) {
                            ChannelRow(followed, open)
                        } else {
                            IconRow(Icons.Outlined.Tag, stringResource(R.string.join_tag_channel, hashtag)) {
                                model.joinChannel(ChannelKind.HASHTAG_CHANNEL, hashtag)
                                    .onSuccess(open)
                                    .onFailure { error = it.message }
                            }
                        }
                    }
                }
                item(key = "actions") {
                    val tiles: @Composable RowScope.() -> Unit = {
                        ActionTile(Icons.Outlined.GroupAdd, stringResource(R.string.new_group), Modifier.weight(1f), newGroup)
                        ActionTile(Icons.Outlined.Tag, stringResource(R.string.join_a_channel), Modifier.weight(1f)) { joining = true }
                        ActionTile(Icons.Outlined.QrCode2, stringResource(R.string.add_contact), Modifier.weight(1f), addContact)
                    }
                    // Side by side; stacked at the largest text sizes.
                    if (LocalDensity.current.fontScale >= 1.5f) {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Row(Modifier.fillMaxWidth()) {
                                ActionTile(Icons.Outlined.GroupAdd, stringResource(R.string.new_group), Modifier.weight(1f), newGroup)
                            }
                            Row(Modifier.fillMaxWidth()) {
                                ActionTile(Icons.Outlined.Tag, stringResource(R.string.join_a_channel), Modifier.weight(1f)) { joining = true }
                            }
                            Row(Modifier.fillMaxWidth()) {
                                ActionTile(Icons.Outlined.QrCode2, stringResource(R.string.add_contact), Modifier.weight(1f), addContact)
                            }
                        }
                    } else {
                        // One height for the three, the tallest label's.
                        Row(
                            Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(vertical = 4.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            content = tiles,
                        )
                    }
                }
                error?.let { item(key = "error") { Banner(BannerKind.SOS, it) } }
                if (channels.isNotEmpty()) {
                    item(key = "channels") { SectionLabel(stringResource(R.string.meshcore_channels), Modifier.padding(top = 8.dp)) }
                    items(channels, key = { "h" + it.id.hex() }) { ChannelRow(it, open) }
                }
                if (groups.isNotEmpty()) {
                    item(key = "groups") { SectionLabel(stringResource(R.string.groups), Modifier.padding(top = 8.dp)) }
                    items(groups, key = { "g" + it.gid.hex() }) { g ->
                        PickRow(g.name, g.gid, pluralStringResource(R.plurals.members_count, g.members.size, g.members.size)) {
                            open(ChatId.group(g.gid))
                        }
                    }
                }
                item(key = "people") { SectionLabel(stringResource(R.string.contacts), Modifier.padding(top = 8.dp)) }
                if (model.contacts.isNotEmpty()) {
                    item(key = "filter") {
                        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                            listOf(
                                PeopleFilter.ALL to stringResource(R.string.people_all, people.size),
                                PeopleFilter.LICHEN to stringResource(R.string.people_lichen, lichenCount),
                                PeopleFilter.MESHCORE to stringResource(R.string.people_meshcore, people.size - lichenCount),
                            ).forEachIndexed { i, (f, label) ->
                                SegmentedButton(
                                    selected = filter == f,
                                    onClick = { filter = f },
                                    shape = SegmentedButtonDefaults.itemShape(i, 3),
                                    colors = segmentedColors(),
                                    icon = {},
                                ) { Text(label, maxLines = 1, fontSize = 13.sp) }
                            }
                        }
                    }
                }
                if (model.contacts.isEmpty()) {
                    item(key = "none") { Text(stringResource(R.string.no_contacts_yet), color = t.tx2, fontSize = 14.sp, modifier = Modifier.padding(8.dp)) }
                } else if (shown.isEmpty()) {
                    item(key = "nobody") { Text(stringResource(R.string.nobody_matches), color = t.tx2, fontSize = 14.sp, modifier = Modifier.padding(8.dp)) }
                }
                // Someone without lichen gets a MeshCore chat (§15); a lichen user, a lichen one.
                items(shown, key = { "c" + it.key.hex() }) { c ->
                    val sub = if (c.identity != null) {
                        stringResource(R.string.lichen_contact, hopsBadge(res, c.hops))
                    } else {
                        stringResource(R.string.meshcore_contact, hopsBadge(res, c.hops))
                    }
                    PickRow(c.name, c.key, sub) { open(if (c.identity == null) ChatId.mesh(c.key) else ChatId.contact(c.key)) }
                }
                item(key = "note") {
                    Text(stringResource(R.string.contacts_meshcore_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(4.dp))
                }
                item(key = "end") { Gap(24.dp) }
            }
        }
    }
    if (joining) JoinChannelSheet(model, { joining = false }, open)
}

/** One of New conversation's three actions: an icon over its name. */
@Composable
private fun ActionTile(icon: ImageVector, title: String, modifier: Modifier, onClick: () -> Unit) {
    val t = lichen
    Column(
        modifier
            .fillMaxHeight()
            .heightIn(min = 72.dp)
            .background(t.s1, CardShape)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(vertical = 10.dp, horizontal = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterVertically),
    ) {
        Icon(icon, null, tint = t.tx)
        Text(title, fontSize = 13.sp, fontWeight = FontWeight.Medium, color = t.tx, textAlign = TextAlign.Center, maxLines = 2)
    }
}

/** A row with an icon and one line ("Join #tag"). */
@Composable
private fun IconRow(icon: ImageVector, text: String, onClick: () -> Unit) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 8.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(icon, null, tint = t.tx, modifier = Modifier.size(40.dp).background(t.s2, CircleShape).padding(9.dp))
        Text(text, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
    }
}

/** A channel of the radio: its name and kind. */
@Composable
private fun ChannelRow(c: MeshChannel, open: (ChatId) -> Unit) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().clickable { open(ChatId.channel(c.id)) }.padding(vertical = 8.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(
            if (c.kind == ChannelKind.PUBLIC_CHANNEL) Icons.Outlined.Language else Icons.Outlined.Tag, null,
            tint = t.tx2, modifier = Modifier.size(40.dp).background(t.s2, CircleShape).padding(9.dp),
        )
        Column {
            Text(c.name, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
            Text(channelKindLabel(LocalResources.current, c.kind), fontSize = 13.sp, color = t.tx2)
        }
    }
}

@Composable
private fun PickRow(name: String, key: ByteArray, subtitle: String, onClick: () -> Unit) {
    val t = lichen
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 8.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(name, key, 40.dp)
        Column {
            Text(name, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
            Text(subtitle, fontSize = 13.sp, color = t.tx2)
        }
    }
}

/** New group (F-4): its name, the lichen contacts to add (searchable), Create. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewGroupScreen(model: AppModel, back: () -> Unit, open: (ChatId) -> Unit) {
    val t = lichen
    var name by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf(setOf<String>()) }
    var error by remember { mutableStateOf<String?>(null) }
    val q = query.trim()
    // Members are people we already talked with in lichen: their identity is known.
    val candidates = model.contacts
        .filter { it.identity != null && (q.isEmpty() || it.name.contains(q, ignoreCase = true)) }
        .sortedWith(compareBy(nameOrder()) { it.name })
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.new_group), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            item {
                OutlinedTextField(
                    name, { name = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    placeholder = { Text(stringResource(R.string.group_name)) },
                    shape = RoundedCornerShape(12.dp),
                    colors = fieldColors(),
                )
            }
            item { SectionLabel(stringResource(R.string.members), Modifier.padding(top = 12.dp)) }
            item {
                OutlinedTextField(
                    query, { query = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Outlined.Search, null, tint = t.tx2) },
                    placeholder = { Text(stringResource(R.string.search_contacts)) },
                    shape = RoundedCornerShape(12.dp),
                    colors = fieldColors(),
                )
            }
            if (candidates.isEmpty()) {
                item { Text(stringResource(R.string.nobody_matches), color = t.tx2, fontSize = 14.sp, modifier = Modifier.padding(8.dp)) }
            }
            items(candidates, key = { "m" + it.key.hex() }) { c ->
                val k = c.key.hex()
                Row(
                    Modifier.fillMaxWidth().clickable { selected = if (k in selected) selected - k else selected + k },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Checkbox(
                        k in selected,
                        { selected = if (it) selected + k else selected - k },
                        colors = CheckboxDefaults.colors(checkedColor = t.actBg, checkmarkColor = t.actFg, uncheckedColor = t.tx2),
                    )
                    Avatar(c.name, c.key, 32.dp)
                    Text(c.name, fontSize = 16.sp, color = t.tx)
                }
            }
            item {
                Text(stringResource(R.string.new_group_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(4.dp))
            }
            error?.let { item { Banner(BannerKind.SOS, it) } }
        }
        Column(Modifier.windowInsetsPadding(WindowInsets.navigationBars).padding(16.dp)) {
            PrimaryButton(stringResource(R.string.create_group), enabled = name.isNotBlank()) {
                val members = model.contacts.filter { it.key.hex() in selected }.map { it.key }
                error = model.createGroup(name.trim(), members)
                if (error == null) {
                    model.groups.firstOrNull { it.name == name.trim() }?.let { open(ChatId.group(it.gid)) }
                }
            }
        }
    }
}
