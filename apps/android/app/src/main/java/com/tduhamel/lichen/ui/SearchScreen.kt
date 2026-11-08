package com.tduhamel.lichen.ui

import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Navigation
import androidx.compose.material.icons.outlined.GpsFixed
import androidx.compose.material.icons.outlined.QuestionMark
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.SearchEstimate
import com.tduhamel.lichen.core.SearchMeasurement
import com.tduhamel.lichen.core.SearchState
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.SosIncident
import com.tduhamel.lichen.model.activeIncidents
import com.tduhamel.lichen.model.addBearing
import com.tduhamel.lichen.model.contains68
import com.tduhamel.lichen.model.ellipse
import com.tduhamel.lichen.model.heatBitmap
import com.tduhamel.lichen.model.initials
import com.tduhamel.lichen.model.lowerPower
import com.tduhamel.lichen.model.measurements
import com.tduhamel.lichen.model.ping
import com.tduhamel.lichen.model.refreshSearches
import com.tduhamel.lichen.model.search
import com.tduhamel.lichen.model.searchCsv
import com.tduhamel.lichen.model.searchEstimate
import com.tduhamel.lichen.model.searchGroup
import com.tduhamel.lichen.model.searchTargets
import com.tduhamel.lichen.model.startSearch
import com.tduhamel.lichen.model.stopSearch
import com.tduhamel.lichen.model.unhex
import kotlinx.coroutines.delay
import kotlin.math.roundToInt

/** "Search" (mockup `recherche`): follow an SOS, and search for a group member by radio
 * (protocol.md §11): where they probably are (heat map, 68 % / 95 % ellipses), which way and how
 * far, the receptions everyone shared, and the actions (ping, bearing, lower power). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchScreen(model: AppModel) {
    val t = lichen
    val context = LocalContext.current
    var error by remember { mutableStateOf<String?>(null) }
    var bearing by remember { mutableStateOf(false) }
    var recenter by remember { mutableIntStateOf(0) }
    // Ages and the estimate refresh on their own.
    var clock by remember { mutableLongStateOf(System.currentTimeMillis()) }
    DisposableEffect(Unit) {
        model.needsLocation("search", true)
        model.useHeading(true)
        model.refreshSearches()
        onDispose {
            model.needsLocation("search", false)
            model.useHeading(false)
        }
    }
    LaunchedEffect(Unit) {
        while (true) {
            delay(5_000)
            clock = System.currentTimeMillis()
        }
    }
    val targets = model.searchTargets
    val target = model.searchTarget?.takeIf { it in targets } ?: targets.firstOrNull()
    LaunchedEffect(target) { recenter++ }
    Scaffold(
        containerColor = t.bg,
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        target?.let { stringResource(R.string.search_name, model.name(it.unhex())) } ?: stringResource(R.string.tab_search),
                        fontSize = 22.sp,
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx),
            )
        },
    ) { padding ->
        if (target == null) {
            Column(
                Modifier.fillMaxSize().padding(padding).padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
            ) {
                Icon(Icons.Outlined.GpsFixed, null, tint = t.tx2, modifier = Modifier.size(44.dp))
                Text(stringResource(R.string.no_search), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                Text(stringResource(R.string.no_search_note), fontSize = 14.sp, color = t.tx2, textAlign = TextAlign.Center)
            }
            return@Scaffold
        }
        val node = target.unhex()
        val search = model.search(target)
        @Suppress("UNUSED_EXPRESSION") clock
        val estimate = if (search == null) null else model.searchEstimate(node)
        val incident = model.incidents[target]
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (targets.size > 1) item { TargetPicker(model, targets, target) }
            item { StatusBanner(model, incident, search) }
            item {
                SearchMap(model, target, estimate, incident, search, recenter)
            }
            item { Pointer(model, estimate, incident, search) }
            if (search == null) {
                item {
                    StartCard(model) { error = model.startSearch(node) }
                }
            } else {
                item {
                    Actions(
                        model, search,
                        ping = { error = model.ping(node) },
                        bearing = { bearing = true },
                        lower = { dbm -> error = model.lowerPower(node, dbm) },
                        export = {
                            model.searchCsv(node)?.let { f ->
                                val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", f)
                                val send = Intent(Intent.ACTION_SEND).setType("text/csv")
                                    .putExtra(Intent.EXTRA_STREAM, uri)
                                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                context.startActivity(Intent.createChooser(send, null))
                            }
                        },
                        stop = { model.stopSearch(node) },
                    )
                }
                item { Readings(model, node, estimate) }
            }
        }
    }
    if (bearing && target != null) {
        BearingSheet(model, target.unhex(), { bearing = false }) { error = it }
    }
    error?.let {
        AlertDialog(
            onDismissRequest = { error = null },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { error = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun TargetPicker(model: AppModel, targets: List<String>, selected: String) {
    val t = lichen
    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (k in targets) {
            val on = k == selected
            Row(
                Modifier
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(50))
                    .background(if (on) t.actBg else t.s2)
                    .clickable { model.searchTarget = k }
                    .padding(horizontal = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                model.incidents[k]?.state()?.takeIf { it != SosIncident.State.ENDED }?.let {
                    Box(Modifier.size(8.dp).background(if (it == SosIncident.State.ACTIVE) t.sos else t.warn, CircleShape))
                }
                Text(model.name(k.unhex()), fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = if (on) t.actFg else t.tx)
            }
        }
    }
}

@Composable
private fun StatusBanner(model: AppModel, incident: SosIncident?, search: SearchState?) {
    val battery = (incident?.battery ?: search?.battery)?.let { stringResource(R.string.battery_suffix, it.toInt()) } ?: ""
    if (incident != null) {
        val heard = ago((incident.lastHeard / 1000).toULong())
        when (incident.state()) {
            SosIncident.State.ACTIVE -> Banner(
                BannerKind.SOS, stringResource(R.string.sos_heard, heard, battery),
                stringResource(if (incident.position == null) R.string.sos_no_gps_note else R.string.sos_gps_note),
            )
            SosIncident.State.STALE -> Banner(BannerKind.WARN, stringResource(R.string.sos_silent, heard), stringResource(R.string.sos_silent_note))
            SosIncident.State.ENDED -> Banner(BannerKind.OK, stringResource(R.string.sos_ended), stringResource(R.string.sos_ended_note))
        }
    } else if (search != null) {
        val heard = search.lastHeardS?.let { stringResource(R.string.heard_ago, ago(it)) } ?: stringResource(R.string.not_heard_yet)
        Banner(BannerKind.LORA, stringResource(R.string.searching_status, heard, battery), stringResource(R.string.searching_note))
    }
}

@Composable
private fun SearchMap(model: AppModel, target: String, estimate: SearchEstimate?, incident: SosIncident?, search: SearchState?, recenter: Int) {
    val t = lichen
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences("lichen", android.content.Context.MODE_PRIVATE) }
    val style = remember { runCatching { MapStyle.valueOf(prefs.getString("map.style", null) ?: "") }.getOrDefault(MapStyle.AUTO) }
    val name = model.name(target.unhex())
    val items = buildList {
        val p = incident?.position
        val f = search?.lastFix
        if (p != null) {
            add(MapItem("target", MapItem.Kind.SOS, p.lat, p.lon, p.radiusM, initials(name), ago(p.atS), t.sosBtn.toArgb(), t.dark))
        } else if (f != null) {
            add(MapItem("target", MapItem.Kind.SOS, f.lat, f.lon, f.radiusM, initials(name), ago(f.atS), t.sosBtn.toArgb(), t.dark))
        }
        model.myFix?.let { me ->
            add(MapItem("me", MapItem.Kind.ME, me.lat, me.lon, me.accuracyM, "", "", t.iroh.toArgb(), t.dark))
        }
    }
    val overlay = estimate?.let { e ->
        MapOverlay(
            model.searchVersion * 31 + target.hashCode(), e.heatBitmap(), e.heatNorth, e.heatSouth, e.heatWest, e.heatEast,
            e.ellipse(e.major68M, e.minor68M), e.ellipse(e.major95M, e.minor95M),
        )
    }
    var reframe by remember { mutableIntStateOf(0) }
    LaunchedEffect(estimate != null) { reframe++ }
    val label = estimate?.let { stringResource(R.string.search_map_label, name, distanceText(it.major68M)) } ?: stringResource(R.string.tab_map)
    Box(Modifier.fillMaxWidth().height(300.dp).clip(CardShape).semantics { contentDescription = label }) {
        if (items.isNotEmpty() || overlay != null) {
            LichenMap(
                items, style.resolved(t.dark), recenter + reframe, Modifier.fillMaxSize(),
                overlay = overlay, margins = intArrayOf(30, 30, 30, 30),
            ) { _, _ -> }
        }
    }
}

/** Which way and how far: to the estimate, else to their last position. Inside the likely zone
 * (a ring around us, two places), there is no direction to give. */
@Composable
private fun Pointer(model: AppModel, estimate: SearchEstimate?, incident: SosIncident?, search: SearchState?) {
    val t = lichen
    val goal: Triple<Double, Double, Double>? = estimate?.let { Triple(it.lat, it.lon, it.major68M) }
        ?: incident?.position?.let { Triple(it.lat, it.lon, it.radiusM) }
        ?: search?.lastFix?.let { Triple(it.lat, it.lon, it.radiusM) }
    val d = goal?.let { model.distanceTo(it.first, it.second) }
    // Inside the 68 % ellipse (a ring around us, two places), there is no direction to give.
    val me = model.myFix
    val unknownWay = estimate != null && me != null && estimate.contains68(me.lat, me.lon)
    val relative = d?.let { dd -> model.heading?.let { ((dd.second - it) % 360 + 360) % 360 } }
    Row(
        Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Box(Modifier.size(64.dp).background(t.s2, CircleShape), contentAlignment = Alignment.Center) {
            if (unknownWay || goal == null) {
                Icon(Icons.Outlined.QuestionMark, null, tint = t.sos, modifier = Modifier.size(30.dp))
            } else {
                Icon(Icons.Filled.Navigation, null, tint = t.sos, modifier = Modifier.size(30.dp).rotate((relative ?: d?.second ?: 0.0).toFloat()))
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            when {
                unknownWay && goal != null -> {
                    Text(stringResource(R.string.direction_unknown), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.direction_unknown_note, distanceText(goal.third)), fontSize = 13.sp, color = t.tx2)
                }
                d != null && goal != null -> {
                    Text("${d.second.roundToInt()}° · ${distanceText(d.first)}", style = figure(28.sp), color = t.tx)
                    Text(
                        stringResource(if (relative != null) R.string.arrow_follows else R.string.bearing_from_north, distanceText(goal.third)),
                        fontSize = 13.sp, color = t.tx2,
                    )
                }
                goal == null -> {
                    Text(stringResource(R.string.target_unknown), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.target_unknown_note), fontSize = 13.sp, color = t.tx2)
                }
                else -> {
                    Text(stringResource(R.string.waiting_position), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.waiting_position_note), fontSize = 13.sp, color = t.tx2)
                }
            }
        }
    }
}

@Composable
private fun StartCard(model: AppModel, start: () -> Unit) {
    val t = lichen
    val target = model.searchTarget
    val possible = target != null && model.searchGroup(target) != null
    Column(Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(stringResource(R.string.search_by_radio), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
        Text(stringResource(R.string.search_by_radio_note), fontSize = 14.sp, color = t.tx2)
        PrimaryButton(stringResource(R.string.start_search), Modifier.fillMaxWidth(), enabled = possible, onClick = start)
        if (!possible) Text(stringResource(R.string.search_needs_group), fontSize = 13.sp, color = t.tx2)
    }
}

@Composable
private fun Actions(
    model: AppModel,
    search: SearchState,
    ping: () -> Unit,
    bearing: () -> Unit,
    lower: (Byte) -> Unit,
    export: () -> Unit,
    stop: () -> Unit,
) {
    val t = lichen
    val wait = ((search.pingInMs.toLong() + 999) / 1000).toInt()
    var power by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SecondaryButton(
            if (wait > 0) stringResource(R.string.ping_in, wait) else stringResource(R.string.ping),
            Modifier.fillMaxWidth(), enabled = wait == 0 && model.isConnected, onClick = ping,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            SecondaryButton(stringResource(R.string.add_bearing), Modifier.weight(1f), enabled = model.myFix != null, onClick = bearing)
            Box(Modifier.weight(1f)) {
                SecondaryButton(stringResource(R.string.lower_power), enabled = model.isConnected) { power = true }
                DropdownMenu(power, { power = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.power_10)) }, { power = false; lower(10) })
                    DropdownMenuItem({ Text(stringResource(R.string.power_2)) }, { power = false; lower(2) })
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(export, Modifier.weight(1f).heightIn(min = 48.dp)) { Text(stringResource(R.string.export), color = t.tx, fontWeight = FontWeight.SemiBold) }
            TextButton(stop, Modifier.weight(1f).heightIn(min = 48.dp)) { Text(stringResource(R.string.stop_search), color = t.sos, fontWeight = FontWeight.SemiBold) }
        }
        search.txDbm?.let { Text(stringResource(R.string.their_tx, it.toInt()), fontSize = 12.sp, color = t.tx2) }
    }
}

@Composable
private fun Readings(model: AppModel, node: ByteArray, estimate: SearchEstimate?) {
    val t = lichen
    val ms = model.measurements(node)
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        SectionLabel(
            when {
                estimate == null -> stringResource(R.string.no_measure)
                estimate.calibrated -> stringResource(R.string.measures_calibrated, ms.size)
                else -> stringResource(R.string.measures_count, ms.size)
            },
        )
        if (ms.isNotEmpty()) {
            CardList {
                ms.take(20).forEachIndexed { i, m ->
                    if (i > 0) CardDivider()
                    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(if (m.mine) stringResource(R.string.you) else model.name(m.by), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                            Text(detail(m), fontSize = 13.sp, color = t.tx2)
                        }
                        Text(m.rssi?.let { "$it dBm" } ?: m.bearingDeg?.let { "$it°" } ?: "—", style = figure(17.sp), color = t.tx)
                    }
                }
            }
        }
    }
}

@Composable
private fun detail(m: SearchMeasurement): String {
    val parts = mutableListOf(ago(m.atS))
    if (m.rssi != null) parts += "SNR %.0f dB".format(m.snrDb)
    if (m.estimated) parts += stringResource(if (m.saturated) R.string.estimated_saturated else R.string.estimated)
    m.bearingDeg?.let { parts += stringResource(if (m.bearingYagi) R.string.bearing_antenna else R.string.bearing_body, it.toInt()) }
    return parts.joinToString(" · ")
}

/** A bearing: face where the signal seems strongest (the last readings help), and keep it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BearingSheet(model: AppModel, target: ByteArray, dismiss: () -> Unit, onError: (String) -> Unit) {
    val t = lichen
    var yagi by remember { mutableStateOf(false) }
    var manual by remember { mutableFloatStateOf(0f) }
    var error by remember { mutableStateOf<String?>(null) }
    val direction = model.heading ?: manual.toDouble()
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(stringResource(R.string.add_bearing), fontSize = 22.sp, color = t.tx)
            Text(stringResource(R.string.bearing_intro), fontSize = 14.sp, color = t.tx2)
            Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(Icons.Filled.Navigation, null, tint = t.sos, modifier = Modifier.size(44.dp))
                Text("${direction.roundToInt()}°", style = figure(40.sp), color = t.tx)
                Text(
                    stringResource(if (model.heading == null) R.string.no_compass else R.string.from_compass),
                    fontSize = 13.sp, color = t.tx2,
                )
            }
            if (model.heading == null) {
                Slider(manual, { manual = it }, valueRange = 0f..359f, steps = 358)
            }
            model.measurements(target).firstOrNull { it.mine && it.rssi != null }?.let { m ->
                Text(stringResource(R.string.last_reading, m.rssi!!.toInt(), ago(m.atS)), fontSize = 14.sp, color = t.tx)
            }
            CardList {
                CardRow(stringResource(R.string.directional_antenna), stringResource(if (yagi) R.string.trusted_15 else R.string.trusted_40)) {
                    LichenSwitch(yagi, { yagi = it })
                }
            }
            error?.let { Banner(BannerKind.SOS, it) }
            Spacer(Modifier.height(4.dp))
            PrimaryButton(stringResource(R.string.use_direction), Modifier.fillMaxWidth()) {
                val e = model.addBearing(target, direction, yagi)
                if (e == null) dismiss() else error = e
            }
        }
    }
}
