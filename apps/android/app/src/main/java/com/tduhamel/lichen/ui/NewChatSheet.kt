package com.tduhamel.lichen.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.hopsBadge

/** Who to write to: groups, then people (lichen users first), searchable. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewChatSheet(model: AppModel, dismiss: () -> Unit, open: (ChatId) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    var query by remember { mutableStateOf("") }
    var groupName by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf(setOf<String>()) }
    var error by remember { mutableStateOf<String?>(null) }
    val q = query.trim()
    val people = model.contacts
        .filter { q.isEmpty() || it.name.contains(q, ignoreCase = true) }
        .sortedWith(compareBy({ it.identity == null }, { it.name.lowercase() }))
    val groups = model.groups.filter { q.isEmpty() || it.name.contains(q, ignoreCase = true) }

    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.new_message), fontSize = 22.sp, color = t.tx)
            TextField(
                query, { query = it },
                Modifier.fillMaxWidth(),
                singleLine = true,
                leadingIcon = { Icon(Icons.Outlined.Search, null) },
                placeholder = { Text(stringResource(R.string.search_contacts)) },
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
                if (groups.isNotEmpty()) {
                    item { SectionLabel(stringResource(R.string.groups), Modifier.padding(top = 4.dp)) }
                    items(groups, key = { "g" + it.name }) { g ->
                        PickRow(g.name, g.gid, pluralStringResource(R.plurals.members_count, g.members.size, g.members.size)) {
                            open(ChatId.group(g.gid))
                        }
                    }
                }
                item { SectionLabel(stringResource(R.string.contacts), Modifier.padding(top = 8.dp)) }
                if (people.isEmpty()) {
                    item { Text(stringResource(R.string.no_contacts_yet), color = t.tx2, fontSize = 14.sp, modifier = Modifier.padding(8.dp)) }
                }
                items(people, key = { "c" + it.name + it.key.contentHashCode() }) { c ->
                    val sub = if (c.identity != null) {
                        stringResource(R.string.lichen_contact, hopsBadge(res, c.hops))
                    } else {
                        hopsBadge(res, c.hops)
                    }
                    PickRow(c.name, c.key, sub) { open(ChatId.contact(c.key)) }
                }
                item { SectionLabel(stringResource(R.string.new_group), Modifier.padding(top = 16.dp)) }
                item {
                    OutlinedTextField(
                        groupName, { groupName = it },
                        Modifier.fillMaxWidth(),
                        singleLine = true,
                        placeholder = { Text(stringResource(R.string.group_name)) },
                        shape = RoundedCornerShape(12.dp),
                        colors = fieldColors(),
                    )
                }
                // Members are people we already talked with in lichen: their identity is known.
                items(people.filter { it.identity != null }, key = { "m" + it.key.contentHashCode() }) { c ->
                    val k = c.key.contentHashCode().toString()
                    Row(
                        Modifier.fillMaxWidth().clickable { selected = if (k in selected) selected - k else selected + k },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(
                            k in selected,
                            { selected = if (it) selected + k else selected - k },
                            colors = CheckboxDefaults.colors(checkedColor = t.actBg, checkmarkColor = t.actFg, uncheckedColor = t.tx2),
                        )
                        Text(c.name, fontSize = 16.sp, color = t.tx)
                    }
                }
                item {
                    Text(stringResource(R.string.new_group_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(4.dp))
                }
                error?.let { item { Banner(BannerKind.SOS, it) } }
                item {
                    PrimaryButton(stringResource(R.string.create_group), enabled = groupName.isNotBlank()) {
                        val members = model.contacts.filter { it.key.contentHashCode().toString() in selected }.map { it.key }
                        error = model.createGroup(groupName.trim(), members)
                        if (error == null) {
                            model.groups.firstOrNull { it.name == groupName.trim() }?.let { open(ChatId.group(it.gid)) }
                        }
                    }
                }
                item { Gap(24.dp) }
            }
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
