package com.tduhamel.lichen.ui

import android.text.format.DateFormat
import androidx.compose.foundation.background
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
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.hex
import java.util.Date

/** Settings › Privacy › Blocked (ADR-0034): who, since when, and Unblock. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BlockedScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.blocked), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                CardList {
                    if (model.blocked.isEmpty()) {
                        CardRow(stringResource(R.string.nobody_blocked))
                    }
                    model.blocked.forEachIndexed { i, b ->
                        if (i > 0) CardDivider(inset = 72.dp)
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Avatar(b.name, b.node)
                            Column(Modifier.weight(1f)) {
                                // A request's identity (ADR-0046), not the radio it came through.
                                Text(
                                    if (b.request) stringResource(R.string.request_blocked_name, b.name) else b.name,
                                    color = t.tx, fontWeight = FontWeight.SemiBold,
                                )
                                Text(
                                    stringResource(R.string.blocked_since, DateFormat.getMediumDateFormat(context).format(Date(b.atMs))),
                                    fontSize = 13.sp, color = t.tx2,
                                )
                            }
                            TextButton(onClick = { model.unblock(b.node) }) { Text(stringResource(R.string.unblock), color = t.tx) }
                        }
                    }
                }
            }
            item {
                Text(stringResource(R.string.blocked_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
        }
    }
}
