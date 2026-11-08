package com.tduhamel.lichen.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.SettingsInputAntenna
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.initials

/** Cards and lists (Android: radius 16). */
val CardShape = RoundedCornerShape(16.dp)

/** A coloured badge ("LoRa · 2 hops"). */
@Composable
fun Badge(text: String, background: Color, foreground: Color) {
    Text(
        text,
        fontSize = 11.sp,
        fontWeight = FontWeight.SemiBold,
        color = foreground,
        maxLines = 1,
        modifier = Modifier
            .background(background, RoundedCornerShape(8.dp))
            .padding(horizontal = 8.dp, vertical = 2.dp),
    )
}

enum class Transport { LORA, IROH, TOR, NEUTRAL }

/** The transport, always written (design principle 3). */
@Composable
fun TransportBadge(kind: Transport, label: String) {
    val t = lichen
    when (kind) {
        Transport.LORA -> Badge(label, t.loraBg, t.loraFg)
        Transport.IROH -> Badge(label, t.irohBg, t.irohFg)
        Transport.TOR -> Badge(label, t.torBg, t.torFg)
        Transport.NEUTRAL -> Badge(label, t.s2, t.tx2)
    }
}

/** Initials in a coloured circle. */
@Composable
fun Avatar(name: String, key: ByteArray, size: Dp = 44.dp) {
    val (bg, fg) = lichen.avatar(key)
    Box(
        Modifier
            .size(size)
            .background(bg, CircleShape)
            .clearAndSetSemantics {},
        contentAlignment = Alignment.Center,
    ) {
        Text(initials(name), color = fg, fontSize = (size.value * 0.32f).sp, fontWeight = FontWeight.SemiBold)
    }
}

/** A thin rounded progress bar. */
@Composable
fun Bar(fraction: Float, modifier: Modifier = Modifier, color: Color = lichen.lora) {
    Box(
        modifier
            .fillMaxWidth()
            .height(6.dp)
            .clip(CircleShape)
            .background(lichen.s2),
    ) {
        Box(
            Modifier
                .fillMaxWidth(fraction.coerceIn(0f, 1f))
                .height(6.dp)
                .background(color, CircleShape),
        )
    }
}

/** Airtime left this hour: figure and bar. */
@Composable
fun AirtimeMeter(usedMs: ULong, limitMs: ULong, large: Boolean = false) {
    val t = lichen
    val left = limitMs - minOf(usedMs, limitMs)
    Column(verticalArrangement = Arrangement.spacedBy(if (large) 10.dp else 8.dp)) {
        if (large) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text("${left / 1000u} s", style = figure(40.sp), color = t.tx)
                Spacer(Modifier.weight(1f))
                Text(stringResource(R.string.of_seconds_per_hour, (limitMs / 1000u).toLong()), fontSize = 13.sp, color = t.tx2)
            }
        } else {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.airtime_left), fontSize = 13.sp, color = t.tx2, modifier = Modifier.weight(1f))
                Text("${left / 1000u} s", style = figure(20.sp), color = t.tx)
                Text(" / ${limitMs / 1000u} s", fontSize = 13.sp, color = t.tx2)
            }
        }
        Bar(left.toFloat() / maxOf(limitMs, 1uL).toFloat(), color = t.lora)
    }
}

/** Fragments sent of a datagram. */
@Composable
fun FragmentProgress(sent: Int, total: Int, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Bar(sent.toFloat() / maxOf(total, 1).toFloat())
        Text(
            stringResource(R.string.sending_fragments, minOf(sent, total), total),
            fontSize = 12.sp,
            color = lichen.tx2,
        )
    }
}

enum class BannerKind { SOS, WARN, LORA, IROH, OK }

/** Banners of the degraded states (mockup `etats`). */
@Composable
fun Banner(kind: BannerKind, title: String, message: String? = null, action: Pair<String, () -> Unit>? = null) {
    val t = lichen
    val (accent, background) = when (kind) {
        BannerKind.SOS -> t.sosFg to t.sosBg
        BannerKind.WARN -> t.warnFg to t.warnBg
        BannerKind.LORA -> t.loraFg to t.loraBg
        BannerKind.IROH -> t.irohFg to t.irohBg
        BannerKind.OK -> t.okFg to t.okBg
    }
    val icon = when (kind) {
        BannerKind.SOS, BannerKind.WARN -> Icons.Outlined.WarningAmber
        BannerKind.LORA -> Icons.Outlined.SettingsInputAntenna
        BannerKind.IROH -> Icons.Outlined.Info
        BannerKind.OK -> Icons.Outlined.Check
    }
    Row(
        Modifier
            .fillMaxWidth()
            .background(background, CardShape)
            .padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = accent, modifier = Modifier.size(18.dp).align(Alignment.Top))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = accent)
            if (message != null) Text(message, fontSize = 13.sp, color = t.tx)
        }
        if (action != null) {
            TextButton(onClick = action.second) {
                Text(action.first, fontWeight = FontWeight.SemiBold, color = t.tx)
            }
        }
    }
}

/** Rows in a card; put a [CardDivider] between them. */
@Composable
fun CardList(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier
            .fillMaxWidth()
            .clip(CardShape)
            .background(lichen.s1),
        content = content,
    )
}

/** The line between two rows of a card. */
@Composable
fun CardDivider(inset: Dp = 16.dp) {
    Box(
        Modifier
            .padding(start = inset)
            .fillMaxWidth()
            .height(1.dp)
            .background(lichen.line),
    )
}

/** A row inside a card: title, subtitle, trailing content. */
@Composable
fun CardRow(title: String, subtitle: String? = null, trailing: @Composable () -> Unit = {}) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .padding(horizontal = 16.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = lichen.tx)
            if (subtitle != null) Text(subtitle, fontSize = 13.sp, color = lichen.tx2)
        }
        trailing()
    }
}

/** A section label above a card ("Radio", "Members"). */
@Composable
fun SectionLabel(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        fontSize = 13.sp,
        color = lichen.tx2,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier.fillMaxWidth().padding(horizontal = 4.dp),
    )
}

/** Primary button: `actBg` / `actFg`, pill, 48 high (Android). */
@Composable
fun PrimaryButton(text: String, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    Button(
        onClick,
        modifier.fillMaxWidth().heightIn(min = 48.dp),
        enabled = enabled,
        colors = ButtonDefaults.buttonColors(containerColor = lichen.actBg, contentColor = lichen.actFg),
    ) { Text(text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold) }
}

/** Secondary button: `s2`, or a semantic tint (`sos` for destructive). */
@Composable
fun SecondaryButton(
    text: String,
    modifier: Modifier = Modifier,
    background: Color = lichen.s2,
    foreground: Color = lichen.tx,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Button(
        onClick,
        modifier.fillMaxWidth().heightIn(min = 48.dp),
        enabled = enabled,
        colors = ButtonDefaults.buttonColors(containerColor = background, contentColor = foreground),
    ) { Text(text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold) }
}

/** The "chirp" mark (docs/design/assets/chirp-mark.svg), drawn in `color`. */
@Composable
fun ChirpMark(color: Color, modifier: Modifier = Modifier) {
    Canvas(modifier) {
        val u = size.minDimension / 64f
        val path = Path().apply {
            moveTo(10f * u, 50f * u)
            lineTo(24f * u, 14f * u)
            lineTo(24f * u, 50f * u)
            lineTo(38f * u, 14f * u)
            lineTo(38f * u, 50f * u)
            lineTo(48f * u, 28.3f * u)
        }
        drawPath(path, color, style = Stroke(width = 5.5f * u, cap = StrokeCap.Round, join = StrokeJoin.Round))
        drawCircle(color, radius = 4f * u, center = Offset(53.5f * u, 16.4f * u))
    }
}

/** A gap of `h`. */
@Composable
fun Gap(h: Dp) = Spacer(Modifier.height(h).width(h))
