package com.tduhamel.lichen.ui

import android.content.Context
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.problemReport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** One entry of `assets/licences/licences.json` (made by `scripts/licences.py`). */
data class LicenceEntry(
    val name: String,
    val version: String,
    val licence: String,
    val url: String,
    val platforms: List<String>?,
    val texts: List<Int>,
) {
    val title: String get() = if (version.isEmpty()) name else "$name $version"
}

/** lichen, what it includes, the libraries of each app, and the Rust crates of the core. */
class Licences(val components: List<LicenceEntry>, val crates: List<LicenceEntry>, val texts: List<String>) {
    companion object {
        fun load(context: Context): Licences? = runCatching {
            val json = JSONObject(context.assets.open("licences.json").bufferedReader().use { it.readText() })
            fun JSONArray.strings() = List(length()) { getString(it) }
            fun JSONArray.entries() = List(length()) { i ->
                val o = getJSONObject(i)
                LicenceEntry(
                    o.getString("name"), o.getString("version"), o.getString("licence"), o.getString("url"),
                    o.optJSONArray("platforms")?.strings(),
                    o.getJSONArray("texts").let { a -> List(a.length()) { a.getInt(it) } },
                )
            }
            Licences(json.getJSONArray("components").entries(), json.getJSONArray("crates").entries(), json.getJSONArray("texts").strings())
        }.getOrNull()
    }
}

/** Settings › About › Licences; a tap opens an entry's licence files. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LicencesScreen(back: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val licences by produceState<Licences?>(null) { value = withContext(Dispatchers.IO) { Licences.load(context) } }
    var open by remember { mutableStateOf<LicenceEntry?>(null) }
    open?.let { e ->
        BackHandler { open = null }
        LicenceScreen(e, e.texts.mapNotNull { licences?.texts?.getOrNull(it) }) { open = null }
        return
    }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.licences), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        val l = licences
        if (l == null) {
            Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = t.tx2) }
            return@Column
        }
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
        ) {
            item { Text(stringResource(R.string.licences_note), fontSize = 14.sp, color = t.tx2, modifier = Modifier.padding(4.dp)) }
            item { SectionLabel(stringResource(R.string.licences_included), Modifier.padding(top = 14.dp, bottom = 8.dp)) }
            licenceRows(l.components.filter { it.platforms?.contains("android") ?: true }) { open = it }
            item { SectionLabel(stringResource(R.string.licences_crates, l.crates.size), Modifier.padding(top = 14.dp, bottom = 8.dp)) }
            licenceRows(l.crates) { open = it }
        }
    }
}

/** Rows of a card, one item each so a long list stays lazy. */
private fun androidx.compose.foundation.lazy.LazyListScope.licenceRows(entries: List<LicenceEntry>, open: (LicenceEntry) -> Unit) {
    itemsIndexed(entries, key = { _, e -> "${e.name} ${e.version} ${e.platforms}" }) { i, e ->
        val t = lichen
        val shape = when {
            entries.size == 1 -> RoundedCornerShape(14.dp)
            i == 0 -> RoundedCornerShape(topStart = 14.dp, topEnd = 14.dp)
            i == entries.lastIndex -> RoundedCornerShape(bottomStart = 14.dp, bottomEnd = 14.dp)
            else -> RoundedCornerShape(0.dp)
        }
        Column(Modifier.fillMaxWidth().background(t.s1, shape)) {
            if (i > 0) CardDivider()
            Row(Modifier.fillMaxWidth().clickable { open(e) }, verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) { CardRow(e.title, e.licence) }
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
            }
        }
    }
}

/** One component or crate: its licence, its link, and the licence files it ships. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LicenceScreen(e: LicenceEntry, texts: List<String>, back: () -> Unit) {
    val t = lichen
    val uri = LocalUriHandler.current
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(e.name, fontSize = 20.sp, maxLines = 1) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { Text(e.licence, fontSize = 17.sp, color = t.tx) }
            item {
                Text(e.url, fontSize = 14.sp, color = t.iroh, modifier = Modifier.clickable { runCatching { uri.openUri(e.url) } })
            }
            if (texts.isEmpty()) item { Text(stringResource(R.string.licence_on_page), fontSize = 14.sp, color = t.tx2) }
            items(texts.size) { i ->
                SelectionContainer {
                    Text(
                        texts[i], fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = t.tx,
                        modifier = Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(14.dp)).padding(12.dp),
                    )
                }
            }
        }
    }
}

/**
 * Settings › About › Report a problem (ADR-0045): the whole report on screen, then the share
 * sheet. No message, key, name or position is in it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReportScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val report = remember { model.problemReport() }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.report_problem), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { Text(stringResource(R.string.report_note), fontSize = 14.sp, color = t.tx2) }
            item { PrimaryButton(stringResource(R.string.report_share)) { shareReport(context, report) } }
            item {
                SelectionContainer {
                    Text(
                        report, fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = t.tx,
                        modifier = Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(14.dp)).padding(12.dp),
                    )
                }
            }
        }
    }
}

/** Shares the report as a text file, in a folder of its own (the FileProvider's). */
private fun shareReport(context: Context, report: String) {
    val file = uniqueShareFile(context, "lichen-report.txt") ?: return
    file.writeText(report)
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND)
        .setType("text/plain")
        .putExtra(Intent.EXTRA_STREAM, uri)
        .putExtra(Intent.EXTRA_SUBJECT, context.getString(R.string.report_subject))
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, null))
}
