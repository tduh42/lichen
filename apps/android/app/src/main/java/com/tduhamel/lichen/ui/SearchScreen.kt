package com.tduhamel.lichen.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Navigation
import androidx.compose.material.icons.outlined.GpsFixed
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.PeerPosition
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.hex
import kotlinx.coroutines.delay
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.roundToInt
import kotlin.math.sin

/** "Search": follow an SOS — bearing and distance from our GPS to theirs (mockup `recherche`).
 * Radio bearings, pings and the particle filter come with search mode (L8). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchScreen(model: AppModel) {
    val t = lichen
    val target = model.incomingSos ?: model.sosLog.lastOrNull()
    DisposableEffect(Unit) {
        // Our position, to measure the distance.
        model.mapVisible = true
        onDispose { model.mapVisible = false }
    }
    LaunchedEffect(Unit) {
        while (true) {
            delay(5_000)
            model.refreshPositions()
        }
    }
    Scaffold(
        containerColor = t.bg,
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(
                            target?.let { stringResource(R.string.search_name, model.name(it.from)) } ?: stringResource(R.string.tab_search),
                            fontSize = 22.sp,
                        )
                        if (target != null) {
                            Text(pluralStringResource(R.plurals.positions_received, model.sosLog.size, model.sosLog.size), fontSize = 12.sp, color = t.tx2)
                        }
                    }
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
                Text(stringResource(R.string.later_search), fontSize = 12.sp, color = t.tx2, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 4.dp))
            }
            return@Scaffold
        }
        val d = model.distanceTo(target)
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item {
                Banner(
                    BannerKind.SOS,
                    stringResource(R.string.sos_received_ago, ago(target.atS)),
                    stringResource(R.string.gps_at, target.radiusM.roundToInt(), "%.4f, %.4f".format(target.lat, target.lon)),
                )
            }
            item {
                val label = stringResource(R.string.radar_label)
                Box(Modifier.fillMaxWidth().height(230.dp).clip(CardShape).background(t.s1).semantics { contentDescription = label }) {
                    Radar(target, d)
                }
            }
            item { BearingCard(target, d) }
            if (model.sosLog.isNotEmpty()) {
                item {
                    CardList {
                        model.sosLog.reversed().take(8).forEachIndexed { i, p ->
                            if (i > 0) CardDivider()
                            val value = model.lastSnr[p.from.hex()]?.let { "SNR %.0f dB".format(it) }
                                ?: model.distanceTo(p)?.let { distanceText(it.first) } ?: "—"
                            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                                Column(Modifier.weight(1f)) {
                                    Text(model.name(p.from), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                                    Text("±${p.radiusM.roundToInt()} m · ${ago(p.atS)}", fontSize = 13.sp, color = t.tx2)
                                }
                                Text(value, style = figure(17.sp), color = t.tx)
                            }
                        }
                    }
                }
            }
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (label in listOf(R.string.ping, R.string.add_bearing, R.string.lower_power)) {
                        SecondaryButton(stringResource(label), Modifier.weight(1f), enabled = false) {}
                    }
                }
            }
            item { Text(stringResource(R.string.search_later_note), fontSize = 12.sp, color = t.tx2) }
        }
    }
}

@Composable
private fun BearingCard(target: PeerPosition, d: Pair<Double, Double>?) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Box(Modifier.size(64.dp).background(t.s2, CircleShape), contentAlignment = Alignment.Center) {
            Icon(Icons.Filled.Navigation, null, tint = t.lora, modifier = Modifier.size(30.dp).rotate((d?.second ?: 0.0).toFloat()))
        }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (d != null) {
                Text("${d.second.roundToInt()}° · ${distanceText(d.first)}", style = figure(28.sp), color = t.tx)
                Text(stringResource(R.string.bearing_note, target.radiusM.roundToInt()), fontSize = 13.sp, color = t.tx2)
            } else {
                Text(stringResource(R.string.waiting_position), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                Text(stringResource(R.string.waiting_position_note), fontSize = 13.sp, color = t.tx2)
            }
        }
    }
}

/** A schematic plan: the SOS position and its uncertainty in the middle, us on the side. */
@Composable
private fun Radar(target: PeerPosition, distance: Pair<Double, Double>?) {
    val t = lichen
    Canvas(Modifier.fillMaxSize()) {
        val c = Offset(size.width / 2, size.height / 2)
        val maxR = minOf(size.width, size.height) / 2 - 18.dp.toPx()
        for (f in listOf(0.33f, 0.66f, 1f)) drawCircle(t.line, maxR * f, c, style = Stroke(1.dp.toPx()))
        // Scale: we sit on the outer ring; the uncertainty is to scale.
        val span = max(max(distance?.first ?: (target.radiusM * 4), target.radiusM * 2), 50.0)
        val ur = max(10.dp.toPx(), (maxR * target.radiusM / span).toFloat())
        drawCircle(t.lora.copy(alpha = 0.25f), ur, c)
        drawCircle(t.sos, 6.dp.toPx(), c)
        if (distance != null) {
            // We are at the opposite of their bearing from us.
            val a = Math.toRadians(distance.second + 180)
            val p = Offset(c.x + maxR * sin(a).toFloat(), c.y - maxR * cos(a).toFloat())
            drawLine(t.iroh.copy(alpha = 0.7f), p, c, 1.5.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(14f, 10f)))
            drawCircle(t.iroh, 7.dp.toPx(), p)
        }
    }
}
