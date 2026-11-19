package com.tduhamel.lichen.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId

/** What can be forwarded (ADR-0027): texts, stickers, GIFs, small files, voice messages. */
fun forwardable(m: StoredMessage): Boolean = when (m.message) {
    is Message.Text, is Message.Sticker, is Message.Gif, is Message.File, is Message.Voice -> true
    else -> false
}

/** Where to forward a message: a new message of ours. A MeshCore chat takes text only; files
 * and voice messages go to lichen contacts. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ForwardSheet(model: AppModel, m: StoredMessage, dismiss: () -> Unit) {
    val t = lichen
    var error by remember { mutableStateOf<String?>(null) }
    val text = m.message is Message.Text
    val any = text || m.message is Message.Sticker || m.message is Message.Gif
    val targets = buildList {
        if (any) model.groups.forEach { add(Triple(it.name, it.gid, ChatId.group(it.gid))) }
        // Blocked contacts are left out, as for a share (D-2).
        model.contacts.filter { it.identity != null && !model.isBlocked(it.key) }.forEach { add(Triple(it.name, it.key, ChatId.contact(it.key))) }
        if (text) {
            model.channels.forEach { add(Triple(it.name, it.id, ChatId.channel(it.id))) }
            model.contacts.filter { it.identity == null && !model.isBlocked(it.key) }.forEach { add(Triple(it.name, it.key, ChatId.mesh(it.key))) }
        }
    }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.forward), fontSize = 22.sp, color = t.tx)
            error?.let { Banner(BannerKind.SOS, it) }
            LazyColumn {
                items(targets, key = { it.third.route }) { (name, key, chat) ->
                    Row(
                        Modifier.fillMaxWidth().clickable {
                            val e = model.forward(m, chat)
                            if (e == null) dismiss() else error = e
                        }.padding(vertical = 8.dp, horizontal = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Avatar(name, key, 40.dp)
                        Column {
                            Text(name, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
                            if (chat.plain) Text(stringResource(R.string.meshcore), fontSize = 13.sp, color = t.tx2)
                        }
                    }
                }
            }
        }
    }
}

/** Where to send what another app shared (ADR-0034): with files or photos, a lichen contact
 * or a group (over the Internet bridge); a text alone goes anywhere, MeshCore chats included.
 * Blocked contacts are left out. The conversation then opens on the usual sheets. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareTargetSheet(model: AppModel, share: com.tduhamel.lichen.model.IncomingShare, dismiss: () -> Unit) {
    val t = lichen
    val targets = buildList {
        model.groups.forEach { add(Triple(it.name, it.gid, ChatId.group(it.gid))) }
        model.contacts.filter { it.identity != null && !model.isBlocked(it.key) }
            .forEach { add(Triple(it.name, it.key, ChatId.contact(it.key))) }
        if (!share.hasFiles) {
            model.channels.forEach { add(Triple(it.name, it.id, ChatId.channel(it.id))) }
            model.contacts.filter { it.identity == null && it.person && !model.isBlocked(it.key) }
                .forEach { add(Triple(it.name, it.key, ChatId.mesh(it.key))) }
        }
    }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.send_to), fontSize = 22.sp, color = t.tx)
            if (share.hasFiles) {
                Text(
                    androidx.compose.ui.res.pluralStringResource(R.plurals.shared_files, share.uris.size, share.uris.size),
                    fontSize = 13.sp, color = t.tx2,
                )
            }
            LazyColumn {
                items(targets, key = { it.third.route }) { (name, key, chat) ->
                    Row(
                        Modifier.fillMaxWidth().clickable { model.share(share, chat) }.padding(vertical = 8.dp, horizontal = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Avatar(name, key, 40.dp)
                        Column {
                            Text(name, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
                            if (chat.plain) Text(stringResource(R.string.meshcore), fontSize = 13.sp, color = t.tx2)
                        }
                    }
                }
            }
        }
    }
}
