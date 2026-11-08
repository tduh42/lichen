package com.tduhamel.lichen.ui

import android.view.HapticFeedbackConstants
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
import androidx.compose.runtime.LaunchedEffect
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
import com.tduhamel.lichen.core.PeerPosition
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
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
        } else {
            SosHoldButton { model.changeSos(true) }
        }
        SosCard(stringResource(R.string.what_is_sent), stringResource(R.string.what_is_sent_detail))
        SosCard(stringResource(R.string.to_whom), recipients(model))
        SosCard(stringResource(R.string.how), stringResource(R.string.how_detail))
        Text(stringResource(R.string.sos_note), fontSize = 12.sp, color = t.tx2, textAlign = TextAlign.Center)
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

/** Someone sent an SOS: full-screen alert (mockup `sos-recu`). */
@Composable
fun SosReceivedDialog(model: AppModel, sos: PeerPosition, search: () -> Unit, reply: () -> Unit, map: () -> Unit) {
    val t = lichen
    // Our position, for the distance and bearing.
    LaunchedEffect(Unit) { model.mapVisible = true }
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
            Text(stringResource(R.string.sos_alert_ago, ago(sos.atS)), fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.sosFg)
            Text(model.name(sos.from), fontSize = 40.sp, fontWeight = FontWeight.Bold, color = t.tx, textAlign = TextAlign.Center)
            val via = sos.group?.let { gid -> model.group(ChatId.group(gid))?.name ?: stringResource(R.string.group) }
            Text(
                if (via != null) stringResource(R.string.received_group, via) else stringResource(R.string.received_direct),
                fontSize = 15.sp, color = t.tx2,
            )
            CardList {
                CardRow(
                    stringResource(R.string.last_gps),
                    "${ago(sos.atS)} · ±${sos.radiusM.roundToInt()} m · ${"%.4f, %.4f".format(sos.lat, sos.lon)}",
                )
                CardDivider()
                val d = model.distanceTo(sos)
                if (d != null) {
                    CardRow(stringResource(R.string.from_you)) {
                        Text("${distanceText(d.first)} · ${d.second.roundToInt()}°", style = figure(18.sp), color = t.tx)
                    }
                } else {
                    CardRow(stringResource(R.string.from_you), stringResource(R.string.position_unknown)) {
                        Badge(stringResource(R.string.unknown), t.warnBg, t.warnFg)
                    }
                }
                CardDivider()
                CardRow(stringResource(R.string.beacon), stringResource(R.string.beacon_note)) {
                    Badge(stringResource(R.string.active), t.loraBg, t.loraFg)
                }
            }
            PrimaryButton(stringResource(R.string.start_search)) {
                model.sosAlert = null
                search()
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton(stringResource(R.string.reply), Modifier.weight(1f), enabled = model.contact(sos.from) != null) {
                    model.sosAlert = null
                    reply()
                }
                SecondaryButton(stringResource(R.string.show_on_map), Modifier.weight(1f)) {
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
