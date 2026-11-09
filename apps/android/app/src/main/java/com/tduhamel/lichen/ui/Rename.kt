package com.tduhamel.lichen.ui

import android.content.Context
import android.text.format.DateFormat
import android.text.format.DateUtils
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import java.time.Instant
import java.time.ZoneId
import java.util.Date

/**
 * Names a contact on this phone only (a nickname, kept in the app's database): never sent, the
 * radio's contact list and the other phones keep their names. Blank: the node's name again.
 */
@Composable
fun RenameDialog(model: AppModel, node: ByteArray, dismiss: () -> Unit) {
    val t = lichen
    val c = model.contact(node) ?: return
    val renamed = c.name != c.nodeName
    var name by remember { mutableStateOf(if (renamed) c.name else "") }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.rename)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(stringResource(R.string.rename_note, c.nodeName), fontSize = 14.sp, color = t.tx2)
                OutlinedTextField(
                    name, { name = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    placeholder = { Text(c.nodeName) },
                    shape = RoundedCornerShape(12.dp),
                    colors = fieldColors(),
                )
                if (renamed) {
                    TextButton(onClick = {
                        model.renameContact(node, null)
                        dismiss()
                    }) { Text(stringResource(R.string.use_node_name, c.nodeName), color = t.tx) }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                model.renameContact(node, name)
                dismiss()
            }) { Text(stringResource(R.string.save), color = t.tx) }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text(stringResource(R.string.cancel), color = t.tx) } },
        containerColor = t.s1,
    )
}

/** A row that swipes from the end to ask for its deletion (the confirmation comes after). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SwipeToDelete(onDelete: () -> Unit, content: @Composable () -> Unit) {
    val t = lichen
    val state = rememberSwipeToDismissBoxState()
    LaunchedEffect(state.currentValue) {
        if (state.currentValue == SwipeToDismissBoxValue.EndToStart) {
            onDelete()
            state.snapTo(SwipeToDismissBoxValue.Settled)
        }
    }
    SwipeToDismissBox(
        state,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Box(Modifier.fillMaxSize().background(t.sosBg).padding(horizontal = 24.dp), contentAlignment = Alignment.CenterEnd) {
                Icon(Icons.Outlined.Delete, stringResource(R.string.delete), tint = t.sosFg)
            }
        },
    ) { content() }
}

/** The local day of a time (Unix ms), to group messages. */
fun dayOf(atMs: Long): Long = Instant.ofEpochMilli(atMs).atZone(ZoneId.systemDefault()).toLocalDate().toEpochDay()

/** "11:20", in the phone's format. */
fun timeOf(context: Context, atMs: Long): String = DateFormat.getTimeFormat(context).format(Date(atMs))

/** Its short date and time, in the user's formats. */
fun dateTimeOf(context: Context, atMs: Long): String =
    DateFormat.getMediumDateFormat(context).format(Date(atMs)) + " " + timeOf(context, atMs)

/** "Today", "Yesterday", or the date. */
@Composable
fun DaySeparator(context: Context, atMs: Long) {
    val t = lichen
    val today = dayOf(System.currentTimeMillis())
    val label = when (dayOf(atMs)) {
        today -> stringResource(R.string.today)
        today - 1 -> stringResource(R.string.yesterday)
        else -> DateUtils.formatDateTime(context, atMs, DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_WEEKDAY)
    }
    Box(Modifier.fillMaxWidth().padding(vertical = 4.dp), contentAlignment = Alignment.Center) {
        Text(
            label, fontSize = 12.sp, color = t.tx2,
            modifier = Modifier.background(t.s2, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 3.dp),
        )
    }
}
