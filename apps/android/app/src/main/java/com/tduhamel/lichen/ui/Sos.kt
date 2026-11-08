package com.tduhamel.lichen.ui

import android.Manifest
import android.view.HapticFeedbackConstants
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.SosIncident
import com.tduhamel.lichen.model.search
import com.tduhamel.lichen.model.searchGroup
import com.tduhamel.lichen.model.startSearch
import com.tduhamel.lichen.model.unhex
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** The SOS screens' own background and cards (mockups `sos`, `sos-recu`). */
private val sosScreen @Composable get() = if (lichen.dark) Color(0xFF1A0B0B) else Color(0xFFFEF2F2)
private val sosCard @Composable get() = if (lichen.dark) Color(0xFF2A1212) else Color(0xFFFEE2E2)

/** Start or stop an SOS: hold for 3 s (mockup `sos`). */
@Composable
fun SosScreen(model: AppModel, close: () -> Unit) {
    val t = lichen
    Column(
        Modifier
            .fillMaxSize()
            .background(sosScreen)
            .windowInsetsPadding(WindowInsets.systemBars)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.distress_alert), fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.sosFg, modifier = Modifier.weight(1f))
            IconButton(onClick = close, colors = IconButtonDefaults.iconButtonColors(containerColor = t.sosBg, contentColor = t.sosFg)) {
                Icon(Icons.Filled.Close, stringResource(R.string.close))
            }
        }
        if (model.sos) {
            Box(Modifier.size(220.dp).background(t.sos.copy(alpha = 0.25f), CircleShape).padding(26.dp).background(t.sosBtn, CircleShape), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("SOS", fontSize = 44.sp, fontWeight = FontWeight.Black, color = Color.White)
                    Text(stringResource(R.string.on), fontSize = 15.sp, color = Color.White)
                }
            }
            PrimaryButton(stringResource(R.string.stop_sos)) { model.changeSos(false) }
            SosStatusBanner(model)
        } else {
            SosHoldButton { model.changeSos(true) }
        }
        SosCard(stringResource(R.string.what_is_sent), stringResource(R.string.what_is_sent_detail))
        SosCard(stringResource(R.string.to_whom), recipients(model))
        SosCard(stringResource(R.string.how), stringResource(R.string.how_detail))
        Text(stringResource(R.string.sos_note), fontSize = 12.sp, color = t.tx2, textAlign = TextAlign.Center)
    }
}

/** What the SOS really does now: never a delivery claim (nobody acknowledges a flood). */
@Composable
private fun SosStatusBanner(model: AppModel) {
    val st = model.sosStatus
    val askLocation = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { model.locationReady() }
    when {
        !model.isConnected -> Banner(BannerKind.WARN, stringResource(R.string.radio_disconnected), stringResource(R.string.sos_no_radio))
        st?.recipients == 0u -> Banner(BannerKind.WARN, stringResource(R.string.sos_nobody), stringResource(R.string.sos_nobody_note))
        st?.beaconOnly == true -> Banner(
            BannerKind.WARN,
            stringResource(if (model.locationDenied) R.string.location_off else R.string.waiting_gps),
            stringResource(R.string.sos_beacons_meanwhile),
            if (model.locationDenied) {
                stringResource(R.string.allow) to {
                    askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
                }
            } else {
                null
            },
        )
        st?.lastQueuedS != null -> Banner(
            BannerKind.OK,
            stringResource(R.string.sos_sent_to, ago(st.lastQueuedS!!), st.recipients.toInt()),
            stringResource(R.string.sos_sent_note),
        )
        else -> Banner(BannerKind.OK, stringResource(R.string.sos_starting), stringResource(R.string.sos_starting_note))
    }
}

@Composable
private fun recipients(model: AppModel): String {
    val names = model.groups.map { "${it.name} (${it.members.size})" } +
        model.shares.map { ChatId.of(it.chat) }.filter { !it.group }.map { model.title(it) }
    return if (names.isEmpty()) stringResource(R.string.nobody_yet) else names.joinToString(", ")
}

@Composable
private fun SosCard(title: String, text: String) {
    Column(
        Modifier.fillMaxWidth().background(sosCard, CardShape).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(title, fontSize = 13.sp, color = lichen.sosFg)
        Text(text, fontSize = 15.sp, color = lichen.tx)
    }
}

/** Hold for 3 seconds, with a ring filling up (mockup `sos`). */
@Composable
fun SosHoldButton(holdSeconds: Float = 3f, fire: () -> Unit) {
    val t = lichen
    val view = LocalView.current
    val scope = rememberCoroutineScope()
    var progress by remember { mutableFloatStateOf(0f) }
    var job by remember { mutableStateOf<Job?>(null) }
    val label = stringResource(R.string.send_sos)
    Box(
        Modifier
            .size(240.dp)
            .semantics {
                contentDescription = label
                onClick { fire(); true }
            }
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown()
                    job = scope.launch {
                        val start = System.nanoTime()
                        while (progress < 1f) {
                            progress = ((System.nanoTime() - start) / 1e9f / holdSeconds).coerceAtMost(1f)
                            delay(16)
                        }
                        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                        fire()
                    }
                    waitForUpOrCancellation()
                    job?.cancel()
                    if (progress < 1f) progress = 0f
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.fillMaxSize()) {
            val w = 8.dp.toPx()
            val inset = w / 2
            drawArc(t.sos.copy(alpha = 0.25f), 0f, 360f, false, Offset(inset, inset), Size(size.width - w, size.height - w), style = Stroke(w))
            drawArc(t.sos, -90f, 360f * progress, false, Offset(inset, inset), Size(size.width - w, size.height - w), style = Stroke(w, cap = StrokeCap.Round))
        }
        Box(Modifier.fillMaxSize().padding(26.dp).background(t.sosBtn, CircleShape), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text("SOS", fontSize = 52.sp, fontWeight = FontWeight.Black, color = Color.White)
                Text(stringResource(R.string.hold_3s), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = Color.White)
            }
        }
    }
}

/** Someone sent an SOS: full-screen alert (mockup `sos-recu`), live while it is up. */
@Composable
fun SosReceivedDialog(model: AppModel, from: String, search: () -> Unit, reply: () -> Unit, map: () -> Unit) {
    val t = lichen
    val sos = model.incidents[from] ?: return
    var error by remember { mutableStateOf<String?>(null) }
    var clock by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(5_000)
            clock = System.currentTimeMillis()
        }
    }
    // Our position, for the distance and bearing.
    DisposableEffect(Unit) {
        model.needsLocation("sosAlert", true)
        onDispose { model.needsLocation("sosAlert", false) }
    }
    val node = from.unhex()
    Dialog(onDismissRequest = { model.sosAlert = null }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .fillMaxSize()
                .background(sosScreen)
                .windowInsetsPadding(WindowInsets.systemBars)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Box(Modifier.padding(top = 24.dp).size(72.dp).background(t.sosBtn, CircleShape), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.WarningAmber, null, tint = Color.White, modifier = Modifier.size(32.dp))
            }
            @Suppress("UNUSED_EXPRESSION") clock
            Text(stringResource(R.string.sos_alert_ago, ago((sos.started / 1000).toULong())), fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.sosFg)
            Text(model.name(node), fontSize = 40.sp, fontWeight = FontWeight.Bold, color = t.tx, textAlign = TextAlign.Center)
            val via = sos.group?.let { gid -> model.group(ChatId.group(gid))?.name ?: stringResource(R.string.group) }
            Text(
                if (via != null) stringResource(R.string.received_group, via) else stringResource(R.string.received_direct),
                fontSize = 15.sp, color = t.tx2,
            )
            CardList {
                val p = sos.position
                if (p != null) {
                    CardRow(
                        stringResource(R.string.last_gps),
                        "${ago(p.atS)} · ±${p.radiusM.roundToInt()} m · ${"%.4f, %.4f".format(p.lat, p.lon)}",
                    )
                    CardDivider()
                    val d = model.distanceTo(p)
                    if (d != null) {
                        CardRow(stringResource(R.string.from_you)) {
                            Text("${distanceText(d.first)} · ${d.second.roundToInt()}°", style = figure(18.sp), color = t.tx)
                        }
                    } else {
                        CardRow(stringResource(R.string.from_you), stringResource(R.string.position_unknown)) {
                            Badge(stringResource(R.string.unknown), t.warnBg, t.warnFg)
                        }
                    }
                } else {
                    CardRow(stringResource(R.string.no_gps), stringResource(R.string.no_gps_note))
                }
                CardDivider()
                val heard = stringResource(R.string.heard_cap, ago((sos.lastHeard / 1000).toULong())) +
                    (sos.battery?.let { stringResource(R.string.battery_suffix, it.toInt()) } ?: "")
                CardRow(stringResource(R.string.beacon), heard) {
                    when (sos.state()) {
                        SosIncident.State.ACTIVE -> Badge(stringResource(R.string.active), t.loraBg, t.loraFg)
                        SosIncident.State.STALE -> Badge(stringResource(R.string.silent), t.warnBg, t.warnFg)
                        SosIncident.State.ENDED -> Badge(stringResource(R.string.ended), t.okBg, t.okFg)
                    }
                }
            }
            error?.let { Banner(BannerKind.WARN, it) }
            val searching = model.search(from) != null
            PrimaryButton(stringResource(if (searching) R.string.follow_search else R.string.start_search)) {
                if (!searching && model.searchGroup(from) != null) {
                    val e = model.startSearch(node)
                    if (e != null) {
                        error = e
                        return@PrimaryButton
                    }
                }
                model.searchTarget = from
                model.sosAlert = null
                search()
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton(stringResource(R.string.reply), Modifier.weight(1f), enabled = model.contact(node) != null) {
                    model.sosAlert = null
                    reply()
                }
                SecondaryButton(stringResource(R.string.show_on_map), Modifier.weight(1f), enabled = sos.position != null) {
                    model.sosAlert = null
                    map()
                }
            }
            TextButton(onClick = { model.sosAlert = null }) { Text(stringResource(R.string.dismiss), color = t.tx2) }
        }
    }
}

/** "850 m", "2.4 km". */
fun distanceText(m: Double): String = if (m < 1_000) "${m.roundToInt()} m" else "%.1f km".format(m / 1000)
