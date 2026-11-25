package com.tduhamel.lichen.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.outlined.AltRoute
import androidx.compose.material.icons.outlined.Autorenew
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.CellTower
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Done
import androidx.compose.material.icons.outlined.DoneAll
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.GpsFixed
import androidx.compose.material.icons.outlined.GraphicEq
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.PersonAdd
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material.icons.outlined.Pin
import androidx.compose.material.icons.outlined.PinDrop
import androidx.compose.material.icons.outlined.Place
import androidx.compose.material.icons.outlined.QrCode2
import androidx.compose.material.icons.outlined.RestartAlt
import androidx.compose.material.icons.outlined.Save
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.SdStorage
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Sensors
import androidx.compose.material.icons.outlined.SettingsInputAntenna
import androidx.compose.material.icons.outlined.ShareLocation
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material.icons.outlined.Straighten
import androidx.compose.material.icons.outlined.Thermostat
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.RadioSetting
import com.tduhamel.lichen.core.RadioSettings
import com.tduhamel.lichen.core.SettingOutcome
import com.tduhamel.lichen.core.Telemetry
import com.tduhamel.lichen.core.meshcoreContactCode
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.bandwidth
import java.text.Normalizer
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

/**
 * LoRa presets with a firm source (ADR-0032): MeshCore's FAQ for USA/Canada, the EU/UK narrow
 * preset its communities use (869.618 MHz, as stock radios ship in Europe). Other regions: the
 * values the local community gives, by hand.
 */
private enum class RadioPreset(val label: Int, val values: Values?) {
    EU_NARROW(R.string.preset_eu_narrow, Values(869_618u, 62_500u, 8u, 8u)),
    US_CANADA(R.string.preset_us_canada, Values(910_525u, 62_500u, 7u, 5u)),
    CUSTOM(R.string.preset_custom, null);

    data class Values(val freqKhz: UInt, val bwHz: UInt, val sf: UByte, val cr: UByte)

    companion object {
        fun matching(r: RadioSettings): RadioPreset =
            entries.firstOrNull { it.values == Values(r.freqKhz, r.bwHz, r.sf, r.cr) } ?: CUSTOM
    }
}

private val BANDWIDTHS = listOf(7_800u, 10_400u, 15_600u, 20_800u, 31_250u, 41_700u, 62_500u, 125_000u, 250_000u, 500_000u)
private val INTERVALS = listOf(60u, 300u, 900u, 3_600u)

/**
 * The radio's own settings (protocol §1.1, ADR-0032): read from the radio at each connection
 * and after each change; the radio keeps them, lichen stores none. On top, the radio itself (its
 * link, battery, firmware and the hour's airtime); then a search, and the settings in sections.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun RadioSettingsScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    LaunchedEffect(Unit) { model.refreshRadio() }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.radio_settings), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        val r = model.radioSettings
        if (!model.isConnected) {
            Text(stringResource(R.string.radio_settings_none), color = t.tx2, modifier = Modifier.padding(20.dp))
            return@Column
        }
        // Linked, but the radio has not answered yet: nothing of its own to show (its values are
        // zeros waiting for it), however long it takes (D-9).
        if (r == null || !r.read) {
            Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                CircularProgressIndicator(Modifier.size(18.dp), color = t.tx2, strokeWidth = 2.dp)
                Text(stringResource(R.string.reading_radio_settings), color = t.tx2)
            }
            return@Column
        }
        // While a setting waits for the radio, the value asked for (B-user-5).
        val shown = model.radioAsked ?: r
        // The LoRa preset being chosen: here, so that the search sees the custom rows too. Kept
        // while anything else of the radio is read again (its battery), started afresh when the
        // radio's LoRa parameters change.
        val lora = listOf(shown.freqKhz, shown.bwHz, shown.sf, shown.cr)
        var preset by remember(lora) { mutableStateOf(RadioPreset.matching(shown)) }
        val sections = settingSections(model, shown, preset)
        val focus = remember { RowFocus() }
        var query by rememberSaveable { mutableStateOf("") }
        val list = rememberLazyListState()
        val searchHeight = remember { mutableIntStateOf(0) }
        // A result chosen: once the sections are back, the list goes to its row, which lights up.
        var landing by remember { mutableStateOf<Pair<String, String>?>(null) }
        LaunchedEffect(landing, query) {
            val (section, row) = landing ?: return@LaunchedEffect
            if (query.isNotEmpty()) return@LaunchedEffect
            val i = sections.indexOfFirst { it.key == section }
            // The sections laid out again first; then below the header and the search, which
            // stays on top.
            withFrameNanos { }
            if (i >= 0) {
                list.scrollToItem(i + 2)
                list.scrollBy(-searchHeight.intValue.toFloat())
                withFrameNanos { }
                focus.requesters[row]?.bringIntoView()
                focus.highlight = row
            }
            // Last: a key of this effect, which it ends.
            landing = null
        }
        LaunchedEffect(focus.highlight) {
            if (focus.highlight == null) return@LaunchedEffect
            delay(1_500)
            focus.highlight = null
        }
        val folded = fold(query.trim())
        val results = if (folded.isEmpty()) emptyList() else sections.flatMap { s ->
            if (fold(s.label).contains(folded)) s.rows.map { s to it } else s.rows.filter { fold(it.title).contains(folded) }.map { s to it }
        }
        CompositionLocalProvider(LocalRowFocus provides focus) {
            LazyColumn(
                Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
                state = list,
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                item(key = "header") { RadioHeader(model, shown) }
                stickyHeader(key = "search") {
                    SettingsSearch(query, Modifier.onSizeChanged { searchHeight.intValue = it.height }) { query = it }
                }
                if (folded.isNotEmpty()) {
                    item(key = "results") {
                        SearchResults(results) { s, row ->
                            query = ""
                            landing = s.key to row.key
                        }
                    }
                } else {
                    for (s in sections) {
                        item(key = s.key) {
                            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                SectionLabel(s.label, Modifier.padding(top = 10.dp))
                                when (s.key) {
                                    "lora" -> LoraSection(model, shown, preset) { preset = it }
                                    "power" -> PowerSection(model, shown)
                                    "position" -> PositionSection(model, shown)
                                    "readings" -> ReadingsSection(model, shown)
                                    "contacts" -> ContactsSection(model, shown)
                                    "delivery" -> DeliverySection(model, shown)
                                    "bluetooth" -> BluetoothSection(model)
                                    "device" -> DeviceSection(model, shown)
                                    "code" -> MeshCoreCode(model, shown)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    model.radioError?.let {
        AlertDialog(
            onDismissRequest = { model.radioError = null },
            title = { Text(stringResource(R.string.not_changed)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { model.radioError = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

// ----- the search -----

/** A setting the search finds: its row's key, title and tile. */
private class SettingEntry(val key: String, val title: String, val icon: ImageVector, val lora: Boolean = false)

/** A section of the settings, in the order shown, with the rows it shows now. */
private class SettingSection(val key: String, val label: String, val rows: List<SettingEntry>)

/** The sections and their rows, in the current language, as the screen shows them (the same
 * conditions as each section below: a row hidden there is not found). */
@Composable
private fun settingSections(model: AppModel, r: RadioSettings, preset: RadioPreset): List<SettingSection> {
    fun e(key: String, title: String, icon: ImageVector, lora: Boolean = false) = SettingEntry(key, title, icon, lora)
    val out = mutableListOf<SettingSection>()
    out += SettingSection(
        "lora", stringResource(R.string.lora),
        buildList {
            add(e("lora_now", stringResource(R.string.lora_now), Icons.Outlined.SettingsInputAntenna, lora = true))
            add(e("preset", stringResource(R.string.preset), Icons.Outlined.Tune))
            if (preset == RadioPreset.CUSTOM) {
                add(e("frequency", stringResource(R.string.frequency_mhz), Icons.Outlined.GraphicEq))
                add(e("bandwidth", stringResource(R.string.bandwidth), Icons.Outlined.Straighten))
                add(e("sf", stringResource(R.string.spreading_factor), Icons.Outlined.Speed))
                add(e("cr", stringResource(R.string.coding_rate), Icons.Outlined.Shield))
            }
            add(e("lora", stringResource(R.string.apply), Icons.Outlined.Done))
        },
    )
    out += SettingSection("power", stringResource(R.string.transmit_power), listOf(e("power", stringResource(R.string.transmit_power), Icons.Outlined.Bolt)))
    val noPosition = r.latE6 == 0 && r.lonE6 == 0
    val usesPhone = r.positionInAdverts && (r.gps != true || noPosition)
    out += SettingSection(
        "position", stringResource(R.string.position_and_gps),
        buildList {
            add(e("position", stringResource(R.string.position_in_adverts), Icons.Outlined.Place))
            if (r.positionInAdverts) {
                add(e("position_shown", stringResource(R.string.position_advertised), Icons.Outlined.PinDrop))
                if (usesPhone && model.myFix != null) add(e("advert_position", stringResource(R.string.use_phone_position), Icons.Outlined.PhoneAndroid))
            }
            if (r.gps != null) {
                add(e("gps", stringResource(R.string.radio_gps), Icons.Outlined.GpsFixed))
                if (r.gps == true) add(e("gps_interval", stringResource(R.string.gps_every), Icons.Outlined.Schedule))
            }
        },
    )
    out += SettingSection(
        "readings", stringResource(R.string.readings),
        listOf(
            e("telemetry_base", stringResource(R.string.telemetry_base), Icons.Outlined.BarChart),
            e("telemetry_location", stringResource(R.string.telemetry_location), Icons.Outlined.ShareLocation),
            e("telemetry_environment", stringResource(R.string.telemetry_environment), Icons.Outlined.Thermostat),
        ),
    )
    if (r.autoaddMask != null) {
        out += SettingSection(
            "contacts", stringResource(R.string.new_contacts),
            buildList {
                add(e("manual_add", stringResource(R.string.add_every_radio), Icons.Outlined.PersonAdd))
                if (r.manualAdd) AUTOADD_KINDS.forEach { (bit, label, icon) -> add(e("autoadd_$bit", stringResource(label), icon)) }
                add(e("autoadd_1", stringResource(R.string.replace_oldest), Icons.Outlined.Autorenew))
                add(e("autoadd_hops", stringResource(R.string.from_how_far), Icons.Outlined.AltRoute))
            },
        )
    }
    out += SettingSection("delivery", stringResource(R.string.delivery), listOf(e("multi_acks", stringResource(R.string.extra_acks, r.multiAcks.toInt()), Icons.Outlined.DoneAll)))
    out += SettingSection(
        "bluetooth", stringResource(R.string.bluetooth),
        listOf(
            e("pin", stringResource(R.string.ble_pin), Icons.Outlined.Pin),
            e("save_pin", stringResource(R.string.save_pin), Icons.Outlined.Save),
            e("restart", stringResource(R.string.restart_radio), Icons.Outlined.RestartAlt),
        ),
    )
    val storage = r.storageUsedKb != null && (r.storageTotalKb ?: 0u) > 0u
    if (storage || model.nodeKey != null) {
        out += SettingSection(
            "device", stringResource(R.string.this_radio),
            buildList {
                if (storage) add(e("storage", stringResource(R.string.storage), Icons.Outlined.SdStorage))
                if (model.nodeKey != null) add(e("key", stringResource(R.string.meshcore_public_key), Icons.Outlined.Key))
            },
        )
    }
    if (model.nodeKey != null) {
        out += SettingSection("code", stringResource(R.string.meshcore_code), listOf(e("code", stringResource(R.string.meshcore_code), Icons.Outlined.QrCode2)))
    }
    return out
}

/** Letters without their accents, in lower case: "émission" is found by "emission". */
private fun fold(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD).replace(Regex("\\p{Mn}+"), "").lowercase()

/** "Search radio settings": the rows' titles and their sections', in the current language. */
@Composable
private fun SettingsSearch(query: String, modifier: Modifier = Modifier, change: (String) -> Unit) {
    val t = lichen
    val label = stringResource(R.string.search_radio_settings)
    // Opaque, so that the list passes under it once it holds to the top.
    Box(modifier.fillMaxWidth().background(t.bg).padding(vertical = 4.dp)) {
        TextField(
            query, change,
            Modifier.fillMaxWidth().semantics { contentDescription = label },
            placeholder = { Text(label, color = t.tx2) },
            leadingIcon = { Icon(Icons.Outlined.Search, null, tint = t.tx2) },
            trailingIcon = {
                if (query.isNotEmpty()) IconButton(onClick = { change("") }) { Icon(Icons.Outlined.Close, stringResource(R.string.clear), tint = t.tx2) }
            },
            singleLine = true,
            shape = RoundedCornerShape(12.dp),
            colors = TextFieldDefaults.colors(
                focusedContainerColor = t.s2, unfocusedContainerColor = t.s2,
                focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
                focusedTextColor = t.tx, unfocusedTextColor = t.tx, cursorColor = t.tx,
            ),
        )
    }
}

/** What the search found: each row with its section; tapped, the list goes to it. */
@Composable
private fun SearchResults(results: List<Pair<SettingSection, SettingEntry>>, open: (SettingSection, SettingEntry) -> Unit) {
    val t = lichen
    if (results.isEmpty()) {
        Text(stringResource(R.string.no_setting_matches), color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp, vertical = 16.dp))
        return
    }
    CardList(Modifier.padding(top = 8.dp)) {
        results.forEachIndexed { i, (s, row) ->
            if (i > 0) CardDivider(TileInset)
            Box(Modifier.clickable { open(s, row) }) {
                // Its section under it, unless the row is named as its section.
                CardRow(row.title, s.label.takeIf { it != row.title }, merge = false, tile = { RowTile(row.icon, row.lora) }) { Chevron() }
            }
        }
    }
}

/** Where the search lands: the rows' places in the list, and the row lit up for a moment. */
private class RowFocus {
    var highlight by mutableStateOf<String?>(null)
    val requesters = mutableMapOf<String, BringIntoViewRequester>()
}

private val LocalRowFocus = staticCompositionLocalOf<RowFocus?> { null }

/** A row the search can land on: brought into view and lit up when it does. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Modifier.landing(key: String): Modifier {
    val focus = LocalRowFocus.current ?: return this
    val requester = remember { BringIntoViewRequester() }
    DisposableEffect(key) {
        focus.requesters[key] = requester
        onDispose { if (focus.requesters[key] === requester) focus.requesters.remove(key) }
    }
    val glow by animateColorAsState(if (focus.highlight == key) lichen.line else Color.Transparent, tween(400), label = "landing")
    return this.bringIntoViewRequester(requester).background(glow)
}

@Composable
private fun RowTile(icon: ImageVector, lora: Boolean = false) {
    val t = lichen
    if (lora) IconTile(icon, t.loraBg, t.loraFg) else IconTile(icon)
}

/** A setting's row: its tile, title and subtitle, what it holds; clickable when [onClick] is
 * given. The search lands on it by [key]. */
@Composable
private fun SettingRow(
    key: String,
    icon: ImageVector,
    title: String,
    subtitle: String? = null,
    lora: Boolean = false,
    onClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    trailing: @Composable () -> Unit = {},
) {
    Box(
        Modifier.landing(key).then(if (onClick != null) Modifier.clickable(enabled = enabled, onClick = onClick) else Modifier),
    ) {
        CardRow(title, subtitle, merge = onClick == null, tile = { RowTile(icon, lora) }, trailing = trailing)
    }
}

@Composable
private fun Chevron() = Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = lichen.tx2)

// ----- the radio -----

/** The radio on top (mockup `RadioSettings`): its name and model, its link and for how long, its
 * battery, the hour's airtime left, its firmware. (The noise floor joins them once the core
 * reports it.) */
@Composable
private fun RadioHeader(model: AppModel, r: RadioSettings) {
    val t = lichen
    val name = r.name.ifEmpty { model.nodeName.ifEmpty { stringResource(R.string.meshcore_radio) } }
    val left = model.budget.limitMs - minOf(model.budget.usedMs, model.budget.limitMs)
    Column(Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(18.dp)).padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            IconTile(painterResource(R.drawable.ic_radio_tower), t.loraBg, t.loraFg, size = 48.dp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(name, fontSize = 18.sp, fontWeight = FontWeight.Bold, color = t.tx, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (r.manufacturer.isNotEmpty() && r.manufacturer != name) {
                    Text(r.manufacturer, fontSize = 13.sp, color = t.tx2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                // The dot on the first line, however many the link takes.
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Box(Modifier.padding(top = 6.dp).size(7.dp).background(t.ok, CircleShape))
                    Text(radioLink(model) + " · " + connectedFor(model), fontSize = 13.sp, color = t.tx2)
                }
            }
            r.batteryMv?.let { mv -> BatteryRing(r.batteryPercent?.toInt() ?: batteryPercent(mv), mv) }
        }
        Box(Modifier.padding(top = 14.dp).fillMaxWidth().height(1.dp).background(t.line))
        Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Stat("${left / 1000u} s", stringResource(R.string.airtime_left_stat), Modifier.weight(1f))
            if (r.firmware.isNotEmpty()) Stat(r.firmware, stringResource(R.string.firmware_stat), Modifier.weight(1f))
        }
        Bar(left.toFloat() / maxOf(model.budget.limitMs, 1uL).toFloat(), Modifier.padding(top = 10.dp), color = t.lora)
    }
}

@Composable
private fun Stat(value: String, label: String, modifier: Modifier = Modifier) {
    Column(modifier.semantics(mergeDescendants = true) {}) {
        Text(value, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = lichen.tx, maxLines = 1, style = TextStyle(fontFeatureSettings = "tnum"))
        Text(label, fontSize = 12.sp, color = lichen.tx2, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** The battery as a ring: its level in `ok`, `warn` at 30 % and below, `sos` at 15 % and below;
 * the percentage and the voltage inside. */
@Composable
private fun BatteryRing(percent: Int, mv: UShort) {
    val t = lichen
    val volts = "%.2f".format(mv.toDouble() / 1000)
    val color = when {
        percent <= 15 -> t.sos
        percent <= 30 -> t.warn
        else -> t.ok
    }
    val said = stringResource(R.string.battery_ring, percent, volts)
    Box(Modifier.size(58.dp).clearAndSetSemantics { contentDescription = said }, contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val w = 5.dp.toPx()
            val inset = w / 2
            val box = Size(size.width - w, size.height - w)
            drawArc(t.line, 0f, 360f, false, Offset(inset, inset), box, style = Stroke(w))
            drawArc(color, -90f, 360f * percent.coerceIn(0, 100) / 100f, false, Offset(inset, inset), box, style = Stroke(w, cap = StrokeCap.Round))
        }
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("$percent %", fontSize = 14.sp, fontWeight = FontWeight.Bold, color = t.tx, style = TextStyle(fontFeatureSettings = "tnum"))
            Text("$volts V", fontSize = 10.sp, color = t.tx2, style = TextStyle(fontFeatureSettings = "tnum"))
        }
    }
}

/** 3.3 V empty, 4.2 V full: the app's estimate when the radio gives no percentage. */
private fun batteryPercent(mv: UShort): Int = (((mv.toDouble() - 3_300) / 900).coerceIn(0.0, 1.0) * 100).roundToInt()

// ----- the sections -----

private fun mhz(khz: UInt) = "%.3f".format(khz.toDouble() / 1000)

private fun summary(khz: UInt, hz: UInt, sf: UByte, cr: UByte) = "${mhz(khz)} MHz · ${bandwidth(hz)} · SF$sf · CR 4/$cr"

@Composable
private fun LoraSection(model: AppModel, r: RadioSettings, preset: RadioPreset, choose: (RadioPreset) -> Unit) {
    val t = lichen
    // What is being chosen, from the radio's LoRa parameters: kept while anything else of the
    // radio is read again (its battery), started afresh when these change.
    val lora = listOf(r.freqKhz, r.bwHz, r.sf, r.cr)
    var freq by remember(lora) { mutableStateOf(mhz(r.freqKhz)) }
    var bw by remember(lora) { mutableStateOf(r.bwHz) }
    var sf by remember(lora) { mutableStateOf(r.sf) }
    var cr by remember(lora) { mutableStateOf(r.cr) }
    var confirming by remember { mutableStateOf(false) }
    val chosen: RadioPreset.Values? = preset.values ?: freq.replace(',', '.').toDoubleOrNull()?.takeIf { it > 0 }?.let {
        RadioPreset.Values((it * 1000).roundToInt().toUInt(), bw, sf, cr)
    }
    val current = RadioPreset.Values(r.freqKhz, r.bwHz, r.sf, r.cr)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        CardList {
            SettingRow("lora_now", Icons.Outlined.SettingsInputAntenna, stringResource(R.string.lora_now), summary(r.freqKhz, r.bwHz, r.sf, r.cr), lora = true)
            CardDivider(TileInset)
            SettingRow("preset", Icons.Outlined.Tune, stringResource(R.string.preset)) {
                Choice(stringResource(preset.label), RadioPreset.entries.map { stringResource(it.label) to it }, choose)
            }
            if (preset == RadioPreset.CUSTOM) {
                CardDivider(TileInset)
                val frequency = stringResource(R.string.frequency_mhz)
                SettingRow("frequency", Icons.Outlined.GraphicEq, frequency) {
                    OutlinedTextField(
                        freq, { freq = it }, Modifier.width(120.dp).semantics { contentDescription = frequency },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        shape = RoundedCornerShape(10.dp),
                        colors = fieldColors(),
                    )
                }
                CardDivider(TileInset)
                SettingRow("bandwidth", Icons.Outlined.Straighten, stringResource(R.string.bandwidth)) {
                    Choice(bandwidth(bw), BANDWIDTHS.map { bandwidth(it) to it }) { bw = it }
                }
                CardDivider(TileInset)
                SettingRow("sf", Icons.Outlined.Speed, stringResource(R.string.spreading_factor)) {
                    Choice("SF$sf", (5..12).map { "SF$it" to it.toUByte() }) { sf = it }
                }
                CardDivider(TileInset)
                SettingRow("cr", Icons.Outlined.Shield, stringResource(R.string.coding_rate)) {
                    Choice("4/$cr", (5..8).map { "4/$it" to it.toUByte() }) { cr = it }
                }
            }
            CardDivider(TileInset)
            val enabled = chosen != null && chosen != current
            SettingRow("lora", Icons.Outlined.Done, stringResource(R.string.apply), onClick = { confirming = true }, enabled = enabled) {
                Text(if (enabled) "✓" else "", color = t.tx2)
            }
            RowStatus(model, "lora")
        }
        Text(stringResource(R.string.lora_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
    }
    if (confirming && chosen != null) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(stringResource(R.string.lora_confirm_title)) },
            text = { Text(stringResource(R.string.lora_confirm, summary(chosen.freqKhz, chosen.bwHz, chosen.sf, chosen.cr), summary(r.freqKhz, r.bwHz, r.sf, r.cr))) },
            confirmButton = {
                TextButton(onClick = {
                    confirming = false
                    model.radioSet(RadioSetting.PARAMS, "lora", { it.copy(freqKhz = chosen.freqKhz, bwHz = chosen.bwHz, sf = chosen.sf, cr = chosen.cr) }) {
                        model.core.setRadioParams(chosen.freqKhz, chosen.bwHz, chosen.sf, chosen.cr)
                    }
                }) { Text(stringResource(R.string.apply), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun PowerSection(model: AppModel, r: RadioSettings) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        CardList {
            SettingRow("power", Icons.Outlined.Bolt, stringResource(R.string.transmit_power)) {
                Stepper(r.txDbm.toInt(), -9..r.maxTxDbm.toInt(), "${r.txDbm} dBm") { v ->
                    model.radioSet(RadioSetting.TX_POWER, "power", { it.copy(txDbm = v.toByte()) }) { model.core.setTxPower(v.toByte()) }
                }
            }
            RowStatus(model, "power")
        }
        Text(stringResource(R.string.transmit_power_note, r.maxTxDbm.toInt()), fontSize = 12.sp, color = lichen.tx2, modifier = Modifier.padding(horizontal = 4.dp))
    }
}

@Composable
private fun minutes(s: UInt): String =
    if (s >= 3_600u) stringResource(R.string.hours_n, (s / 3_600u).toInt()) else stringResource(R.string.minutes_n, (s / 60u).toInt())

private fun AppModel.setPrefs(
    r: RadioSettings,
    row: String,
    manualAdd: Boolean = r.manualAdd,
    telemetry: Telemetry = r.telemetry,
    position: Boolean = r.positionInAdverts,
    multiAcks: UByte = r.multiAcks,
) = radioSet(
    RadioSetting.PREFERENCES, row,
    { it.copy(manualAdd = manualAdd, telemetry = telemetry, positionInAdverts = position, multiAcks = multiAcks) },
) { core.setRadioPreferences(manualAdd, telemetry, position, multiAcks) }

/** Position and GPS: the position in the radio's adverts (after a warning), and its GPS. */
@Composable
private fun PositionSection(model: AppModel, r: RadioSettings) {
    // A radio advertising its position takes this phone's (below) when it has none of its own:
    // without GPS, or with its GPS on but no fix yet, indoors (B-user-2). The phone's location
    // runs while that choice is on screen, as on the map. A radio whose GPS has a position keeps
    // it: its own fix is better.
    val noPosition = r.latE6 == 0 && r.lonE6 == 0
    val usesPhone = r.positionInAdverts && (r.gps != true || noPosition)
    DisposableEffect(usesPhone) {
        if (usesPhone) model.needsLocation("radio", true)
        onDispose { if (usesPhone) model.needsLocation("radio", false) }
    }
    val t = lichen
    var confirmingPosition by remember { mutableStateOf(false) }
    CardList {
        SettingRow(
            "position", Icons.Outlined.Place,
            stringResource(R.string.position_in_adverts),
            stringResource(if (r.positionInAdverts) R.string.position_in_adverts_on else R.string.position_in_adverts_off),
        ) {
            LichenSwitch(r.positionInAdverts, { on -> if (on) confirmingPosition = true else model.setPrefs(r, "position", position = false) })
        }
        RowStatus(model, "position")
        if (r.positionInAdverts) {
            CardDivider(TileInset)
            val pos = if (noPosition) stringResource(R.string.none_yet)
            // Figures any reader and any map takes, "45.12345, 5.12345", as a place in a message.
            else String.format(java.util.Locale.ROOT, "%.5f, %.5f", r.latE6 / 1e6, r.lonE6 / 1e6)
            SettingRow("position_shown", Icons.Outlined.PinDrop, stringResource(R.string.position_advertised), pos)
            // Why there is none: the GPS is on but has no fix (it needs open sky).
            if (r.gps == true && noPosition) {
                Text(
                    stringResource(R.string.advert_gps_no_fix),
                    fontSize = 13.sp,
                    color = t.tx2,
                    modifier = Modifier.padding(start = TileInset, end = 16.dp, bottom = 11.dp),
                )
            }
            val fix = model.myFix
            if (usesPhone && fix != null) {
                CardDivider(TileInset)
                SettingRow("advert_position", Icons.Outlined.PhoneAndroid, stringResource(R.string.use_phone_position), onClick = {
                    val lat = (fix.lat * 1e6).roundToInt()
                    val lon = (fix.lon * 1e6).roundToInt()
                    model.radioSet(RadioSetting.ADVERT_POSITION, "advert_position", { it.copy(latE6 = lat, lonE6 = lon) }) {
                        model.core.setAdvertPosition(lat, lon)
                    }
                })
                RowStatus(model, "advert_position")
            }
        }
        if (r.gps != null) {
            CardDivider(TileInset)
            SettingRow("gps", Icons.Outlined.GpsFixed, stringResource(R.string.radio_gps), stringResource(R.string.radio_gps_note)) {
                LichenSwitch(r.gps == true, { on -> model.radioSet(RadioSetting.GPS, "gps", { it.copy(gps = on) }) { model.core.setGps(on) } })
            }
            RowStatus(model, "gps")
            if (r.gps == true) {
                CardDivider(TileInset)
                SettingRow("gps_interval", Icons.Outlined.Schedule, stringResource(R.string.gps_every)) {
                    // The radio's own value: one it refuses is not shown (D-4).
                    val label = r.gpsIntervalS?.let { minutes(it) } ?: stringResource(R.string.not_set)
                    Choice(label, INTERVALS.map { minutes(it) to it }) { s ->
                        model.radioSet(RadioSetting.GPS_INTERVAL, "gps_interval", { it.copy(gpsIntervalS = s) }) { model.core.setGpsInterval(s) }
                    }
                }
                RowStatus(model, "gps_interval")
            }
        }
    }
    if (confirmingPosition) {
        AlertDialog(
            onDismissRequest = { confirmingPosition = false },
            title = { Text(stringResource(R.string.position_confirm_title)) },
            text = { Text(stringResource(R.string.position_confirm)) },
            confirmButton = {
                TextButton(onClick = { confirmingPosition = false; model.setPrefs(r, "position", position = true) }) {
                    Text(stringResource(R.string.show_it), color = t.sos)
                }
            },
            dismissButton = { TextButton(onClick = { confirmingPosition = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** Readings: who may ask the radio for its telemetry. */
@Composable
private fun ReadingsSection(model: AppModel, r: RadioSettings) {
    val t = lichen
    var confirmingTelemetry by remember { mutableStateOf<UByte?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        CardList {
            TelemetryRow("telemetry_base", Icons.Outlined.BarChart, stringResource(R.string.telemetry_base), r.telemetry.base) { v ->
                model.setPrefs(r, "telemetry_base", telemetry = r.telemetry.copy(base = v))
            }
            RowStatus(model, "telemetry_base")
            CardDivider(TileInset)
            TelemetryRow("telemetry_location", Icons.Outlined.ShareLocation, stringResource(R.string.telemetry_location), r.telemetry.location) { v ->
                if (v.toInt() == 0) model.setPrefs(r, "telemetry_location", telemetry = r.telemetry.copy(location = v)) else confirmingTelemetry = v
            }
            RowStatus(model, "telemetry_location")
            CardDivider(TileInset)
            TelemetryRow("telemetry_environment", Icons.Outlined.Thermostat, stringResource(R.string.telemetry_environment), r.telemetry.environment) { v ->
                model.setPrefs(r, "telemetry_environment", telemetry = r.telemetry.copy(environment = v))
            }
            RowStatus(model, "telemetry_environment")
        }
        Text(stringResource(R.string.telemetry_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
    }
    confirmingTelemetry?.let { v ->
        AlertDialog(
            onDismissRequest = { confirmingTelemetry = null },
            title = { Text(stringResource(R.string.telemetry_confirm_title)) },
            text = { Text(stringResource(R.string.telemetry_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmingTelemetry = null
                    model.setPrefs(r, "telemetry_location", telemetry = r.telemetry.copy(location = v))
                }) { Text(stringResource(R.string.allow), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { confirmingTelemetry = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun TelemetryRow(key: String, icon: ImageVector, title: String, value: UByte, set: (UByte) -> Unit) {
    val labels = listOf(stringResource(R.string.nobody), stringResource(R.string.chosen_contacts), stringResource(R.string.everyone))
    SettingRow(key, icon, title) {
        Choice(labels.getOrElse(value.toInt()) { "?" }, labels.mapIndexed { i, l -> l to i.toUByte() }, set)
    }
}

/** The kinds of node the radio adds by itself (its autoadd mask's bits), with their tiles. */
private val AUTOADD_KINDS = listOf(
    Triple(0x02, R.string.kind_companions, Icons.Outlined.Person),
    Triple(0x04, R.string.kind_repeaters, Icons.Outlined.CellTower),
    Triple(0x08, R.string.kind_rooms, Icons.Outlined.Forum),
    Triple(0x10, R.string.kind_sensors, Icons.Outlined.Sensors),
)

@Composable
private fun ContactsSection(model: AppModel, r: RadioSettings) {
    val mask = r.autoaddMask ?: 0u
    val hops = r.autoaddMaxHops ?: 0u
    fun setMask(bit: Int, on: Boolean) {
        val m = (if (on) mask.toInt() or bit else mask.toInt() and bit.inv()).toUByte()
        model.radioSet(RadioSetting.AUTOADD, "autoadd_$bit", { it.copy(autoaddMask = m, autoaddMaxHops = hops) }) { model.core.setAutoadd(m, hops) }
    }
    CardList {
        SettingRow(
            "manual_add", Icons.Outlined.PersonAdd,
            stringResource(R.string.add_every_radio),
            stringResource(if (r.manualAdd) R.string.add_every_radio_off else R.string.add_every_radio_on),
        ) {
            LichenSwitch(!r.manualAdd, { on -> model.setPrefs(r, "manual_add", manualAdd = !on) })
        }
        RowStatus(model, "manual_add")
        if (r.manualAdd) {
            AUTOADD_KINDS.forEach { (bit, label, icon) ->
                CardDivider(TileInset)
                SettingRow("autoadd_$bit", icon, stringResource(label)) { LichenSwitch(mask.toInt() and bit != 0, { setMask(bit, it) }) }
                RowStatus(model, "autoadd_$bit")
            }
        }
        CardDivider(TileInset)
        SettingRow("autoadd_1", Icons.Outlined.Autorenew, stringResource(R.string.replace_oldest)) {
            LichenSwitch(mask.toInt() and 0x01 != 0, { setMask(0x01, it) })
        }
        RowStatus(model, "autoadd_1")
        CardDivider(TileInset)
        SettingRow("autoadd_hops", Icons.Outlined.AltRoute, stringResource(R.string.from_how_far)) {
            val choices = (listOf(0, 1, 2, 3, 4, 6) + hops.toInt()).distinct().sorted()
            Choice(hopLabel(hops.toInt()), choices.map { hopLabel(it) to it.toUByte() }) { h ->
                model.radioSet(RadioSetting.AUTOADD, "autoadd_hops", { it.copy(autoaddMaxHops = h) }) { model.core.setAutoadd(mask, h) }
            }
        }
        RowStatus(model, "autoadd_hops")
    }
}

/** 0 any distance, 1 direct only, N up to N − 1 hops (the firmware's rule). */
@Composable
private fun hopLabel(h: Int): String = when (h) {
    0 -> stringResource(R.string.any_distance)
    1 -> stringResource(R.string.direct_only)
    else -> pluralStringResource(R.plurals.up_to_hops, h - 1, h - 1)
}

@Composable
private fun DeliverySection(model: AppModel, r: RadioSettings) {
    CardList {
        SettingRow("multi_acks", Icons.Outlined.DoneAll, stringResource(R.string.extra_acks, r.multiAcks.toInt()), stringResource(R.string.extra_acks_note)) {
            Stepper(r.multiAcks.toInt(), 0..2) { v -> model.setPrefs(r, "multi_acks", multiAcks = v.toUByte()) }
        }
        RowStatus(model, "multi_acks")
    }
}

@Composable
private fun BluetoothSection(model: AppModel) {
    val t = lichen
    var pin by remember { mutableStateOf("") }
    var saved by remember { mutableStateOf(false) }
    var confirmingReboot by remember { mutableStateOf(false) }
    val valid = pin.isEmpty() || (pin.length == 6 && pin.all(Char::isDigit))
    CardList {
        val pinLabel = stringResource(R.string.ble_pin)
        SettingRow("pin", Icons.Outlined.Pin, pinLabel, stringResource(if (saved) R.string.pin_saved else R.string.pin_note)) {
            OutlinedTextField(
                pin, { pin = it.take(6) }, Modifier.width(110.dp).semantics { contentDescription = pinLabel },
                singleLine = true,
                placeholder = { Text("123456") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
                shape = RoundedCornerShape(10.dp),
                colors = fieldColors(),
            )
        }
        CardDivider(TileInset)
        SettingRow("save_pin", Icons.Outlined.Save, stringResource(R.string.save_pin), enabled = valid, onClick = {
            model.radioSet(RadioSetting.PIN, "pin", { it }) { model.core.setBlePin(pin.toUIntOrNull() ?: 0u) }
            saved = model.radioError == null
        })
        RowStatus(model, "pin")
        CardDivider(TileInset)
        SettingRow("restart", Icons.Outlined.RestartAlt, stringResource(R.string.restart_radio), onClick = { confirmingReboot = true })
    }
    if (confirmingReboot) {
        AlertDialog(
            onDismissRequest = { confirmingReboot = false },
            title = { Text(stringResource(R.string.restart_radio_confirm_title)) },
            text = { Text(stringResource(R.string.restart_radio_confirm)) },
            confirmButton = {
                TextButton(onClick = { confirmingReboot = false; model.rebootRadio() }) {
                    Text(stringResource(R.string.restart), color = t.sos)
                }
            },
            dismissButton = { TextButton(onClick = { confirmingReboot = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** This radio: its storage, and its MeshCore public key to copy or share (F-1). */
@Composable
private fun DeviceSection(model: AppModel, r: RadioSettings) {
    CardList {
        val used = r.storageUsedKb
        val total = r.storageTotalKb
        val storage = used != null && total != null && total > 0u
        if (used != null && total != null && total > 0u) {
            SettingRow("storage", Icons.Outlined.SdStorage, stringResource(R.string.storage), stringResource(R.string.radio_storage, used.toInt(), total.toInt()))
        }
        model.nodeKey?.let { key ->
            if (storage) CardDivider(TileInset)
            Box(Modifier.landing("key")) { MeshCoreKeyRow(key, share = true, tile = { RowTile(Icons.Outlined.Key) }) }
        }
    }
}

/** The stock MeshCore app's contact code for this radio (F-1, §15.3): a QR code a MeshCore user
 * scans to add it. */
@Composable
private fun MeshCoreCode(model: AppModel, r: RadioSettings) {
    val t = lichen
    val key = model.nodeKey ?: return
    val code = remember(r.name, key.toList()) { runCatching { meshcoreContactCode(r.name, key, NodeKind.COMPANION) }.getOrNull() } ?: return
    Column(
        Modifier.fillMaxWidth().clip(CardShape).background(t.s1).landing("code").padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        QrImage(code, stringResource(R.string.meshcore_code), Modifier.size(220.dp))
        Text(stringResource(R.string.meshcore_code_note), fontSize = 13.sp, color = t.tx2, textAlign = TextAlign.Center)
    }
}

/**
 * What became of the setting changed from [row] (B-user-5): a spinner while the radio has not
 * answered, then "Saved on the radio" for 2 s, or why the value shown is still the radio's
 * (kept, refused, unanswered) until the next change.
 */
@Composable
private fun RowStatus(model: AppModel, row: String) {
    val state = model.radioRows[row] ?: return
    val t = lichen
    val outcome = state.outcome
    if (outcome == SettingOutcome.SAVED) LaunchedEffect(state) {
        delay(2_000)
        model.clearRadioRow(row)
    }
    val waiting = stringResource(R.string.radio_waiting)
    Row(
        // Under the row's title, past its tile.
        Modifier.padding(start = TileInset, end = 16.dp, bottom = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        when (outcome) {
            null -> CircularProgressIndicator(
                Modifier.size(16.dp).semantics { contentDescription = waiting }, color = t.tx2, strokeWidth = 2.dp,
            )
            SettingOutcome.SAVED -> {
                Icon(Icons.Filled.Check, null, tint = t.ok, modifier = Modifier.size(16.dp))
                Text(stringResource(R.string.radio_saved), fontSize = 13.sp, color = t.tx)
            }
            else -> {
                Icon(Icons.Outlined.Info, null, tint = t.warn, modifier = Modifier.size(16.dp))
                val why = when (outcome) {
                    SettingOutcome.NOT_KEPT -> R.string.radio_not_kept
                    SettingOutcome.REFUSED -> R.string.radio_refused
                    else -> R.string.radio_no_answer
                }
                Text(stringResource(why), fontSize = 13.sp, color = t.tx)
            }
        }
    }
}

/** A value and a menu of the others. */
@Composable
private fun <T> Choice(label: String, options: List<Pair<String, T>>, pick: (T) -> Unit) {
    val t = lichen
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }) { Text(label, color = t.tx) }
        DropdownMenu(open, { open = false }, containerColor = t.s1) {
            options.forEach { (text, value) ->
                DropdownMenuItem(text = { Text(text, color = t.tx) }, onClick = { open = false; pick(value) })
            }
        }
    }
}

/** − value + within `range`, on one tinted pill (mockup `RadioSettings`); [value] in the middle
 * when given. */
@Composable
private fun Stepper(current: Int, range: IntRange, value: String? = null, set: (Int) -> Unit) {
    val t = lichen
    Row(Modifier.clip(RoundedCornerShape(10.dp)).background(t.s2), verticalAlignment = Alignment.CenterVertically) {
        StepButton("−", stringResource(R.string.lower), current > range.first) { set(current - 1) }
        if (value != null) {
            Text(value, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx, style = TextStyle(fontFeatureSettings = "tnum"))
        }
        StepButton("+", stringResource(R.string.higher), current < range.last) { set(current + 1) }
    }
}

@Composable
private fun StepButton(sign: String, label: String, enabled: Boolean, onClick: () -> Unit) {
    val t = lichen
    Box(
        Modifier.size(48.dp).clickable(enabled = enabled, onClickLabel = label, onClick = onClick).semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Text(sign, color = if (enabled) t.tx else t.tx2.copy(alpha = 0.5f), fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
    }
}
