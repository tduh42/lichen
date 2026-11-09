package com.tduhamel.lichen.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.display
import java.text.Normalizer

/** Search in the messages on this phone: conversations by name, then text messages, newest
 * first, case and accents aside. Nothing goes on the air. `open` takes a message id when a
 * message was picked. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchMessagesScreen(model: AppModel, back: () -> Unit, open: (ChatId, Long?) -> Unit) {
    val t = lichen
    var query by rememberSaveable { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val keyboard = LocalSoftwareKeyboardController.current
    val go: (ChatId, Long?) -> Unit = { chat, id ->
        keyboard?.hide()
        open(chat, id)
    }
    val q = query.trim()
    val results = if (q.length >= 2) model.searchMessages(q) else emptyList()
    val chats = if (q.length < 2) {
        emptyList()
    } else {
        (model.groups.map { ChatId.group(it.gid) } + model.contacts.map { ChatId.contact(it.key) })
            .filter { fold(model.title(it)).contains(fold(q)) }
    }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = {
                IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
            },
            title = {
                TextField(
                    query,
                    { query = it },
                    Modifier.fillMaxWidth().focusRequester(focus),
                    placeholder = { Text(stringResource(R.string.search_messages)) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null) },
                    trailingIcon = {
                        if (query.isNotEmpty()) {
                            IconButton(onClick = { query = "" }) { Icon(Icons.Outlined.Close, stringResource(R.string.clear)) }
                        }
                    },
                    singleLine = true,
                    shape = RoundedCornerShape(24.dp),
                    colors = TextFieldDefaults.colors(
                        focusedContainerColor = t.s1, unfocusedContainerColor = t.s1,
                        focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
                        focusedTextColor = t.tx, unfocusedTextColor = t.tx, cursorColor = t.tx,
                        focusedPlaceholderColor = t.tx2, unfocusedPlaceholderColor = t.tx2,
                        focusedLeadingIconColor = t.tx2, unfocusedLeadingIconColor = t.tx2,
                    ),
                )
            },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars.union(WindowInsets.ime)),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (chats.isNotEmpty()) {
                item { SectionLabel(stringResource(R.string.conversations)) }
                item {
                    CardList {
                        chats.forEachIndexed { i, chat ->
                            if (i > 0) CardDivider()
                            Text(
                                model.title(chat), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable { go(chat, null) }
                                    .heightIn(min = 48.dp)
                                    .padding(horizontal = 16.dp, vertical = 14.dp),
                            )
                        }
                    }
                }
            }
            if (results.isNotEmpty()) {
                item { SectionLabel(stringResource(R.string.messages_section)) }
                item {
                    CardList {
                        results.forEachIndexed { i, m ->
                            if (i > 0) CardDivider()
                            ResultRow(model, m, q) { go(ChatId.of(m.chat), m.id) }
                        }
                    }
                }
            } else if (q.length >= 2 && chats.isEmpty()) {
                item { Text(stringResource(R.string.no_messages_found), fontSize = 14.sp, color = t.tx2) }
            }
        }
    }
}

/** A found message: its conversation, when, and the text around the match in bold. */
@Composable
private fun ResultRow(model: AppModel, m: StoredMessage, query: String, open: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val res = LocalResources.current
    val who = when {
        m.outgoing -> stringResource(R.string.you) + ": "
        m.author != null -> model.name(m.author!!) + ": "
        else -> ""
    }
    val text = (m.message as? Message.Text)?.text ?: m.message.display(res)
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = open)
            .padding(horizontal = 16.dp, vertical = 12.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                model.title(ChatId.of(m.chat)), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx,
                maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f),
            )
            Text(shortTime(context, m.atMs), fontSize = 13.sp, color = t.tx2)
        }
        Text(snippet(who, text, query, t.tx), fontSize = 14.sp, color = t.tx2, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

/** "Camille: …the text around the match…", the match in bold. Case and accents aside: the
 * folded text keeps one character per original one for Latin letters, so positions match. */
private fun snippet(who: String, text: String, query: String, strong: Color): AnnotatedString {
    var shown = text
    var at = fold(shown).indexOf(fold(query))
    if (at > 40) {
        shown = "…" + text.substring(at - 30)
        at = fold(shown).indexOf(fold(query))
    }
    return buildAnnotatedString {
        append(who)
        if (at < 0 || at + query.length > shown.length) {
            append(shown)
        } else {
            append(shown.substring(0, at))
            pushStyle(SpanStyle(fontWeight = FontWeight.Bold, color = strong))
            append(shown.substring(at, at + query.length))
            pop()
            append(shown.substring(at + query.length))
        }
    }
}

/** Lower case without accents, one character per character for precomposed Latin letters
 * (as the core's search folds). */
private fun fold(s: String): String = buildString {
    for (c in s) {
        val base = Normalizer.normalize(c.toString(), Normalizer.Form.NFD).firstOrNull() ?: c
        append(base.lowercaseChar())
    }
}
