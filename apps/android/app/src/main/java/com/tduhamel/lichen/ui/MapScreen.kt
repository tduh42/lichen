package com.tduhamel.lichen.ui

import android.Manifest
import android.text.format.DateUtils
import android.text.format.Formatter
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.CloudDownload
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Layers
import androidx.compose.material.icons.outlined.MyLocation
import androidx.compose.material.icons.outlined.Place
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.OfflineArea
import androidx.compose.material3.AlertDialog
import com.tduhamel.lichen.core.PeerPosition
import com.tduhamel.lichen.core.ShareInfo
import com.tduhamel.lichen.core.SharePrecision
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.AppModel.MapMode
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.OfflineMaps
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.initials
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.maplibre.android.geometry.LatLngBounds

/** Precision names, and the nominal precision of a received radius. */
@Composable
fun precisionName(p: SharePrecision): String = stringResource(
    when (p) {
        SharePrecision.EXACT -> R.string.exact
        SharePrecision.M100 -> R.string.m100
        SharePrecision.KM1 -> R.string.km1
        SharePrecision.KM10 -> R.string.km10
    },
)

/** "1 km" for a position rounded to the 1 km grid (its radius is at least 0.7 cell), null for
 * an exact one. */
@Composable
fun nominal(radius: Double): String? = when {
    radius >= 7_000 -> precisionName(SharePrecision.KM10)
    radius >= 700 -> precisionName(SharePrecision.KM1)
    radius >= 70 -> precisionName(SharePrecision.M100)
    else -> null
}

/** "now", "3 min ago". */
@Composable
fun ago(unixS: ULong): String {
    val ms = unixS.toLong() * 1000
    if (System.currentTimeMillis() - ms < 60_000) return stringResource(R.string.now)
    return DateUtils.getRelativeTimeSpanString(ms, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()
}

/** The Map tab, in one of its two modes: everyone's positions, or Locate (the radio search,
 * §11). Its SOS button is there in both. */
@Composable
fun MapScreen(model: AppModel, sos: () -> Unit) {
    when (model.mapMode) {
        MapMode.POSITIONS -> PositionsMap(model, sos)
        MapMode.LOCATE -> Column(Modifier.fillMaxSize().background(lichen.bg)) {
            MapBar(model, sos, Modifier.windowInsetsPadding(WindowInsets.statusBars).padding(horizontal = 16.dp, vertical = 8.dp)) {
                // Nothing to layer here: the room the layers take, so that the modes stay put.
                Box(Modifier.size(48.dp))
            }
            LocateContent(model, Modifier.weight(1f))
        }
    }
}

/** The Map's bar: the SOS button, the two modes, and [end] (the layers). */
@Composable
private fun MapBar(model: AppModel, sos: () -> Unit, modifier: Modifier = Modifier, end: @Composable () -> Unit) {
    val t = lichen
    Row(modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(
            Modifier
                .heightIn(min = 48.dp)
                .shadow(4.dp, CircleShape)
                .background(t.sosBtn, CircleShape)
                .clickable(onClick = sos)
                .padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(Icons.Outlined.WarningAmber, null, tint = Color.White, modifier = Modifier.size(18.dp))
            Text(stringResource(if (model.sos) R.string.sos_on else R.string.sos), color = Color.White, fontWeight = FontWeight.Bold, fontSize = 15.sp, maxLines = 1)
        }
        Box(Modifier.weight(1f), contentAlignment = Alignment.Center) { MapModes(model) }
        end()
    }
}

/** Positions | Locate: a segmented control, each mode a tab. */
@Composable
private fun MapModes(model: AppModel) {
    val t = lichen
    Row(Modifier.shadow(4.dp, CircleShape).background(t.s1, CircleShape).selectableGroup()) {
        for ((mode, label) in listOf(MapMode.POSITIONS to R.string.map_positions, MapMode.LOCATE to R.string.map_locate)) {
            val on = model.mapMode == mode
            Box(
                Modifier
                    .weight(1f, fill = false)
                    .heightIn(min = 48.dp)
                    .selectable(on, role = Role.Tab) { model.mapMode = mode }
                    .padding(4.dp)
                    .background(if (on) t.actBg else Color.Transparent, CircleShape)
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                // Smaller rather than cut on a narrow phone, in French with the SOS on.
                Text(
                    stringResource(label), color = if (on) t.actFg else t.tx, fontWeight = FontWeight.SemiBold, maxLines = 1,
                    autoSize = TextAutoSize.StepBased(minFontSize = 11.sp, maxFontSize = 14.sp),
                )
            }
        }
    }
}

/** "Map": positions shared with us, ours, SOS, and our shares (mockup `carte`). */
@Composable
private fun PositionsMap(model: AppModel, sos: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val offline = remember { OfflineMaps(context) }
    val prefs = remember { context.getSharedPreferences("lichen", android.content.Context.MODE_PRIVATE) }
    var style by remember {
        mutableStateOf(runCatching { MapStyle.valueOf(prefs.getString("map.style", null) ?: "") }.getOrDefault(MapStyle.AUTO))
    }
    var recenter by remember { mutableIntStateOf(0) }
    var seen by remember { mutableStateOf(setOf<String>()) }
    var camera by remember { mutableStateOf<Pair<LatLngBounds, Double>?>(null) }
    var editing by remember { mutableStateOf(false) }
    var layers by remember { mutableStateOf(false) }
    var showingOffline by remember { mutableStateOf(false) }
    // MeshCore nodes at the positions they advertise (protocol.md §16.1).
    var showNodes by remember { mutableStateOf(prefs.getBoolean("map.nodes", true)) }
    val askLocation = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { model.locationReady() }
    DisposableEffect(Unit) {
        model.needsLocation("map", true)
        model.refreshPositions()
        if (!model.isDemo && model.locationDenied) {
            askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
        }
        onDispose { model.needsLocation("map", false) }
    }
    LaunchedEffect(Unit) {
        while (true) {
            delay(5_000)
            model.refreshPositions()
        }
    }
    val items = mapItems(model, showNodes)
    // Someone new on the map: reframe around everyone (nodes do not count). A place from a
    // message is centred on by itself (`focus`), and keeps the map on it.
    val people = items.filter { it.kind != MapItem.Kind.NODE && it.kind != MapItem.Kind.PLACE }.map { it.id }
    LaunchedEffect(people) {
        val ids = people.toSet()
        if ((ids - seen).isNotEmpty()) {
            val first = seen.isEmpty()
            seen = seen + ids
            if (!(first && (model.mapPin != null || model.mapFocus != null))) recenter++
        }
    }

    Column(Modifier.fillMaxSize().background(t.bg)) {
        Box(Modifier.weight(1f)) {
            MapGate(model) {
                // The latest of a place from a message and a node from its page.
                val focus = listOfNotNull(model.mapPin, model.mapFocus).maxByOrNull { it.version }
                LichenMap(items, style.resolved(t.dark), recenter, Modifier.fillMaxSize(), focus = focus) { b, z -> camera = b to z }
            }
            Column(
                Modifier.fillMaxWidth().windowInsetsPadding(WindowInsets.statusBars).padding(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
            MapBar(model, sos) {
                Box {
                    MapIcon(Icons.Outlined.Layers, stringResource(R.string.map_layers), t.tx) { layers = true }
                    DropdownMenu(layers, { layers = false }, containerColor = t.s1) {
                        Text(stringResource(R.string.map_style), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
                        for (s in MapStyle.entries) {
                            DropdownMenuItem(
                                text = { Text(styleName(s), fontWeight = if (s == style) FontWeight.Bold else FontWeight.Normal) },
                                onClick = {
                                    style = s
                                    prefs.edit().putString("map.style", s.name).apply()
                                    layers = false
                                },
                            )
                        }
                        HorizontalDivider(color = t.line)
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.map_nodes)) },
                            leadingIcon = { Checkbox(showNodes, null) },
                            onClick = {
                                showNodes = !showNodes
                                prefs.edit().putBoolean("map.nodes", showNodes).apply()
                                layers = false
                            },
                        )
                        HorizontalDivider(color = t.line)
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.download_area)) },
                            leadingIcon = { Icon(Icons.Outlined.CloudDownload, null) },
                            enabled = camera != null && offline.progress == null,
                            onClick = {
                                camera?.let { (b, z) -> offline.download(b, z, style.resolved(t.dark).url, style.resolved(t.dark).key) }
                                layers = false
                            },
                        )
                        if (offline.areas.isNotEmpty()) {
                            DropdownMenuItem(
                                text = {
                                    Text(stringResource(R.string.offline_maps_count, offline.areas.size, Formatter.formatShortFileSize(context, offline.bytes)))
                                },
                                leadingIcon = { Icon(Icons.Outlined.Layers, null) },
                                onClick = {
                                    showingOffline = true
                                    layers = false
                                },
                            )
                        }
                        Text(stringResource(R.string.offline_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
                    }
                }
            }
            Box(Modifier.align(Alignment.End)) {
                MapIcon(Icons.Outlined.MyLocation, stringResource(R.string.show_everyone), t.iroh) { recenter++ }
            }
            model.mapPin?.let { PinBar(model, it) }
            }
            offline.progress?.let { (done, total) ->
                Row(
                    Modifier
                        .align(Alignment.TopCenter)
                        .padding(top = 96.dp)
                        .background(t.s1, CircleShape)
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    LinearProgressIndicator({ done.toFloat() / total }, Modifier.width(80.dp), color = t.tx, trackColor = t.s2)
                    Text(stringResource(R.string.downloading_map, done, total), fontSize = 12.sp, color = t.tx)
                }
            }
        }
        SharePanel(model) {
            editing = true
        }
    }
    if (editing) ShareSheet(model) { editing = false }
    if (showingOffline) OfflineMapsSheet(offline) { showingOffline = false }
    offline.lastError?.let {
        AlertDialog(
            onDismissRequest = { offline.lastError = null },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { offline.lastError = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun styleName(s: MapStyle): String = stringResource(
    when (s) {
        MapStyle.AUTO -> R.string.follow_theme
        MapStyle.DARK -> R.string.dark
        MapStyle.LIBERTY -> R.string.outdoor
        MapStyle.POSITRON -> R.string.light
        MapStyle.BRIGHT -> R.string.bright
    },
)

@Composable
private fun MapIcon(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, tint: Color, onClick: () -> Unit) {
    IconButton(
        onClick = onClick,
        modifier = Modifier.size(48.dp).shadow(4.dp, CircleShape),
        colors = IconButtonDefaults.iconButtonColors(containerColor = lichen.s2, contentColor = tint),
    ) { Icon(icon, label) }
}

private const val PLACE_ID = "place"

/** The place from a message on the map (ADR-0034): its coordinates, to copy, and a way to put
 * it away. */
@Composable
private fun PinBar(model: AppModel, pin: com.tduhamel.lichen.model.MapPin) {
    val t = lichen
    val clipboard = androidx.compose.ui.platform.LocalClipboard.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .shadow(4.dp, CircleShape)
            .background(t.s2, CircleShape)
            .padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(Icons.Outlined.Place, null, tint = t.iroh, modifier = Modifier.size(18.dp))
        Text(
            pin.label, color = t.tx, fontSize = 14.sp, fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
            modifier = Modifier.weight(1f),
        )
        IconButton(onClick = {
            scope.launch {
                clipboard.setClipEntry(androidx.compose.ui.platform.ClipEntry(android.content.ClipData.newPlainText("lichen", pin.label)))
            }
        }) { Icon(Icons.Outlined.ContentCopy, stringResource(R.string.copy), tint = t.tx) }
        IconButton(onClick = { model.mapPin = null }) { Icon(Icons.Outlined.Close, stringResource(R.string.remove_pin), tint = t.tx) }
    }
}

/** The markers: everyone's last position (SOS first in colour), ours, and a place from a
 * message. */
@Composable
private fun mapItems(model: AppModel, showNodes: Boolean): List<MapItem> {
    val t = lichen
    val out = mutableListOf<MapItem>()
    if (showNodes) {
        for (n in model.nodes) {
            val lat = n.lat ?: continue
            val lon = n.lon ?: continue
            val color = if (n.kind == com.tduhamel.lichen.core.NodeKind.REPEATER) t.lora.toArgb() else 0xFFD6D6D0.toInt()
            out += MapItem("n:" + n.key.hex(), MapItem.Kind.NODE, lat, lon, 0.0, initials(n.name), n.name, color, t.dark)
        }
    }
    model.mapPin?.let { pin ->
        out += MapItem(PLACE_ID, MapItem.Kind.PLACE, pin.lat, pin.lon, 0.0, "", "", t.iroh.toArgb(), t.dark)
    }
    for (p in model.positions) {
        val name = model.name(p.from)
        val color = if (p.sos) t.sosBtn else t.marker(p.from)
        out += MapItem(
            p.from.hex(), if (p.sos) MapItem.Kind.SOS else MapItem.Kind.PEER, p.lat, p.lon, p.radiusM,
            initials(name), label(p, name), color.toArgb(), t.dark,
        )
    }
    model.myFix?.let { me ->
        out += MapItem("me", MapItem.Kind.ME, me.lat, me.lon, me.accuracyM, "", "", t.iroh.toArgb(), t.dark)
    }
    return out
}

@Composable
private fun label(p: PeerPosition, name: String): String {
    if (p.sos) return "SOS · $name"
    val coarse = nominal(p.radiusM)
    return if (coarse != null) "$name · ±$coarse" else "$name · ${ago(p.atS)}"
}

/** Bottom card: our shares and their precision (mockup `carte`). */
@Composable
private fun SharePanel(model: AppModel, edit: () -> Unit) {
    val t = lichen
    var expanded by remember { mutableStateOf(true) }
    val askLocation = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { model.locationReady() }
    Column(
        Modifier
            .fillMaxWidth()
            .background(t.s1, RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
            .padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val handle = stringResource(if (expanded) R.string.hide_details else R.string.show_details)
        Box(
            Modifier.fillMaxWidth().height(12.dp).clickable { expanded = !expanded }.semantics { contentDescription = handle },
            contentAlignment = Alignment.Center,
        ) { Box(Modifier.size(width = 36.dp, height = 5.dp).background(t.line, CircleShape)) }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.location_sharing), fontSize = 20.sp, fontWeight = FontWeight.Bold, color = t.tx)
                Text(shareStatus(model), fontSize = 13.sp, color = if (model.sos) t.sosFg else t.tx2)
            }
            LichenSwitch(model.shares.isNotEmpty(), { on -> if (on) edit() else model.stopAllShares() }, label = stringResource(R.string.location_sharing))
        }
        if (expanded) {
            LocationAccessBanner(model) {
                askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
            }
            Column(Modifier.fillMaxWidth().background(t.s2, RoundedCornerShape(12.dp))) {
                model.shares.forEachIndexed { i, s ->
                    if (i > 0) CardDivider()
                    ShareRow(model, s, edit)
                }
                if (model.shares.isNotEmpty()) CardDivider()
                Row(
                    Modifier.fillMaxWidth().clickable(onClick = edit).padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Box(Modifier.size(32.dp).background(t.s1, CircleShape), contentAlignment = Alignment.Center) {
                        Icon(Icons.Filled.Add, null, tint = t.tx, modifier = Modifier.size(18.dp))
                    }
                    Text(stringResource(if (model.shares.isEmpty()) R.string.share_my_position else R.string.precision_per_contact), fontSize = 16.sp, color = t.tx)
                }
            }
            Text(stringResource(R.string.share_note), fontSize = 12.sp, color = t.tx2)
        }
    }
}

@Composable
private fun shareStatus(model: AppModel): String {
    if (model.sos) return stringResource(R.string.sos_status)
    val soonest = model.shares.minOfOrNull { it.untilS } ?: return stringResource(R.string.off)
    val left = maxOf(0L, soonest.toLong() * 1000 - System.currentTimeMillis())
    val minutes = (left + 59_999) / 60_000
    val text = if (minutes >= 60) "${minutes / 60} h ${minutes % 60} min" else "$minutes min"
    return stringResource(R.string.active_ends_in, text)
}

@Composable
private fun ShareRow(model: AppModel, s: ShareInfo, edit: () -> Unit) {
    val t = lichen
    val chat = ChatId.of(s.chat)
    Row(
        Modifier.fillMaxWidth().clickable(onClick = edit).padding(horizontal = 16.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(model.title(chat), chat.hex.let { h -> h.chunked(2).map { it.toInt(16).toByte() }.toByteArray() }, 32.dp)
        Column(Modifier.weight(1f)) {
            Text(model.title(chat), fontSize = 16.sp, color = t.tx)
            Text(shareDetail(model, chat), fontSize = 13.sp, color = t.tx2)
        }
        Text(precisionName(s.precision), fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
    }
}

@Composable
private fun shareDetail(model: AppModel, chat: ChatId): String {
    if (chat.group) {
        val n = model.group(chat)?.members?.size ?: 0
        return pluralStringResource(R.plurals.group_members_flood_detail, n, n)
    }
    val hops = chat.contactKey?.let { model.contact(it) }?.hops ?: return stringResource(R.string.contact_path_unknown)
    if (hops.toInt() == 0) return stringResource(R.string.contact_lora_direct)
    return pluralStringResource(R.plurals.contact_hops, hops.toInt(), hops.toInt())
}

/** Who sees our position, at what precision, for how long. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareSheet(model: AppModel, dismiss: () -> Unit) {
    val t = lichen
    var hours by remember { mutableStateOf(1.0) }
    var error by remember { mutableStateOf<String?>(null) }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.precision_per_contact), fontSize = 22.sp, color = t.tx)
            SectionLabel(stringResource(R.string.duration))
            DurationPicker(hours) { hours = it }
            Text(stringResource(R.string.share_stops_note), fontSize = 12.sp, color = t.tx2)
            error?.let { Banner(BannerKind.SOS, stringResource(R.string.could_not_share), it) }
            if (model.contacts.isNotEmpty()) {
                SectionLabel(stringResource(R.string.contacts), Modifier.padding(top = 8.dp))
                CardList {
                    model.contacts.forEachIndexed { i, c ->
                        if (i > 0) CardDivider(inset = 60.dp)
                        PrecisionRow(model, ChatId.contact(c.key), c.name, c.key, hours) { error = it }
                    }
                }
            }
            if (model.groups.isNotEmpty()) {
                SectionLabel(stringResource(R.string.groups), Modifier.padding(top = 8.dp))
                CardList {
                    model.groups.forEachIndexed { i, g ->
                        if (i > 0) CardDivider(inset = 60.dp)
                        PrecisionRow(model, ChatId.group(g.gid), g.name, g.gid, hours) { error = it }
                    }
                }
                Text(stringResource(R.string.group_position_rate), fontSize = 12.sp, color = t.tx2)
            }
            if (model.contacts.isEmpty() && model.groups.isEmpty()) {
                Text(stringResource(R.string.no_contacts_yet), color = t.tx2)
            }
            Text(stringResource(R.string.rounded_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(top = 8.dp))
        }
    }
}

@Composable
private fun DurationPicker(hours: Double, pick: (Double) -> Unit) {
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        listOf(1.0 to "1 h", 8.0 to "8 h", 24.0 to "24 h").forEachIndexed { i, (h, label) ->
            SegmentedButton(hours == h, { pick(h) }, SegmentedButtonDefaults.itemShape(i, 3), colors = segmentedColors()) { Text(label) }
        }
    }
}

@Composable
private fun PrecisionRow(model: AppModel, chat: ChatId, name: String, key: ByteArray, hours: Double, onError: (String?) -> Unit) {
    val t = lichen
    val current = model.share(chat)?.precision
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(name, key, 32.dp)
        Text(name, fontSize = 16.sp, color = t.tx, modifier = Modifier.weight(1f))
        Box {
            TextButton(onClick = { menu = true }) {
                Text(current?.let { precisionName(it) } ?: stringResource(R.string.not_shared), fontWeight = FontWeight.SemiBold, color = if (current == null) t.tx2 else t.tx)
            }
            DropdownMenu(menu, { menu = false }, containerColor = t.s1) {
                DropdownMenuItem(text = { Text(stringResource(R.string.not_shared)) }, onClick = {
                    menu = false
                    onError(model.setShare(chat, null, hours))
                })
                for (p in listOf(SharePrecision.EXACT, SharePrecision.M100, SharePrecision.KM1, SharePrecision.KM10)) {
                    DropdownMenuItem(text = { Text(precisionName(p)) }, onClick = {
                        menu = false
                        onError(model.setShare(chat, p, hours))
                    })
                }
            }
        }
    }
}

/** Our position for one chat, from the composer's menu: the same shares as the map's panel. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatShareSheet(model: AppModel, chat: ChatId, dismiss: () -> Unit) {
    val t = lichen
    val current = model.share(chat)
    var precision by remember { mutableStateOf(current?.precision ?: SharePrecision.EXACT) }
    var hours by remember { mutableStateOf(1.0) }
    var error by remember { mutableStateOf<String?>(null) }
    val askLocation = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { model.locationReady() }
    LaunchedEffect(Unit) {
        if (!model.isDemo && model.locationDenied) {
            askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
        }
    }
    fun apply(p: SharePrecision?) {
        error = model.setShare(chat, p, hours)
        if (error == null) dismiss()
    }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.share_my_location), fontSize = 22.sp, color = t.tx)
            LocationAccessBanner(model) {
                askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
            }
            SectionLabel(stringResource(R.string.precision))
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                val all = listOf(SharePrecision.EXACT, SharePrecision.M100, SharePrecision.KM1, SharePrecision.KM10)
                all.forEachIndexed { i, p ->
                    SegmentedButton(precision == p, { precision = p }, SegmentedButtonDefaults.itemShape(i, all.size), colors = segmentedColors()) {
                        Text(precisionName(p), maxLines = 1)
                    }
                }
            }
            Text(stringResource(R.string.rounded_note), fontSize = 12.sp, color = t.tx2)
            SectionLabel(stringResource(R.string.duration), Modifier.padding(top = 8.dp))
            DurationPicker(hours) { hours = it }
            Text(stringResource(R.string.share_stops_note), fontSize = 12.sp, color = t.tx2)
            if (chat.group) Text(stringResource(R.string.group_position_rate), fontSize = 12.sp, color = t.tx2)
            error?.let { Banner(BannerKind.SOS, stringResource(R.string.could_not_share), it) }
            PrimaryButton(stringResource(if (current == null) R.string.share else R.string.update), Modifier.padding(top = 8.dp)) { apply(precision) }
            if (current != null) {
                SecondaryButton(stringResource(R.string.stop_sharing), background = t.sosBg, foreground = t.sosFg) { apply(null) }
            }
        }
    }
}

/** Why our position is missing or rough (audit M10): denied, approximate only, services off.
 * `ask` requests the permission again (Android offers the upgrade to precise). */
@Composable
fun LocationAccessBanner(model: AppModel, ask: () -> Unit) {
    if (model.isDemo) return
    val context = LocalContext.current
    when {
        model.locationDenied -> Banner(
            BannerKind.WARN, stringResource(R.string.location_off), stringResource(R.string.location_off_note),
            stringResource(R.string.allow) to ask,
        )
        model.locationServicesOff -> Banner(
            BannerKind.WARN, stringResource(R.string.location_services_off), stringResource(R.string.location_services_off_note),
            stringResource(R.string.open_settings) to {
                context.startActivity(android.content.Intent(android.provider.Settings.ACTION_LOCATION_SOURCE_SETTINGS))
            },
        )
        model.locationApproximate -> Banner(
            BannerKind.WARN, stringResource(R.string.location_approximate), stringResource(R.string.location_approximate_note),
            stringResource(R.string.allow_precise) to ask,
        )
    }
}

/** The areas kept for use without a network, one by one: what they cover, whether they are
 * complete, and what to do when they are not (audit M11). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun OfflineMapsSheet(offline: OfflineMaps, dismiss: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.offline_maps), fontSize = 22.sp, color = t.tx)
            CardList {
                offline.areas.forEachIndexed { i, a ->
                    if (i > 0) CardDivider()
                    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(a.name, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx, modifier = Modifier.weight(1f))
                            when (a.state) {
                                OfflineArea.State.COMPLETE -> Badge(stringResource(R.string.offline_complete), t.okBg, t.okFg)
                                OfflineArea.State.DOWNLOADING -> Badge(stringResource(R.string.offline_downloading), t.irohBg, t.irohFg)
                                OfflineArea.State.PARTIAL -> Badge(stringResource(R.string.offline_partial), t.warnBg, t.warnFg)
                                OfflineArea.State.FAILED -> Badge(stringResource(R.string.offline_failed), t.sosBg, t.sosFg)
                            }
                        }
                        val created = a.created?.let { android.text.format.DateFormat.getMediumDateFormat(context).format(java.util.Date(it)) }
                        Text(
                            listOfNotNull(a.style.ifEmpty { null }, Formatter.formatShortFileSize(context, a.bytes), created).joinToString(" · "),
                            fontSize = 13.sp, color = t.tx2,
                        )
                        if (a.state != OfflineArea.State.COMPLETE && a.total > 0) {
                            LinearProgressIndicator({ a.done.toFloat() / a.total }, Modifier.fillMaxWidth(), color = t.tx, trackColor = t.s2)
                            Text(stringResource(R.string.offline_resources, a.done, a.total), fontSize = 12.sp, color = t.tx2)
                        }
                        a.error?.let { Text(it, fontSize = 13.sp, color = t.sosFg) }
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            when (a.state) {
                                OfflineArea.State.DOWNLOADING -> TextButton({ offline.pause(a) }) { Text(stringResource(R.string.pause), color = t.tx) }
                                OfflineArea.State.PARTIAL -> TextButton({ offline.resume(a) }) { Text(stringResource(R.string.resume), color = t.tx) }
                                OfflineArea.State.FAILED -> TextButton({ offline.resume(a) }) { Text(stringResource(R.string.try_again), color = t.tx) }
                                OfflineArea.State.COMPLETE -> Unit
                            }
                            Box(Modifier.weight(1f))
                            TextButton({ offline.delete(a) }) { Text(stringResource(R.string.delete), color = t.sos) }
                        }
                    }
                }
            }
            Text(stringResource(R.string.offline_maps_note), fontSize = 12.sp, color = t.tx2)
        }
    }
}
