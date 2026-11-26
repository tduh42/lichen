package com.tduhamel.lichen.ui

import androidx.compose.ui.layout.Layout
import androidx.compose.ui.unit.Density
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.ui.semantics.Role
import com.tduhamel.lichen.core.SignalRating
import com.tduhamel.lichen.model.dbText
import androidx.compose.ui.text.TextStyle
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.CompositionLocalProvider
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.material3.IconButton
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.foundation.clickable
import android.content.Intent
import android.content.ClipData
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
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
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

/** Two or three choices side by side, each a tab (48 dp tall), the chosen one on `actBg`. */
@Composable
fun SegmentedTabs(labels: List<String>, selected: Int, choose: (Int) -> Unit) {
    val t = lichen
    Row(Modifier.background(t.s2, CircleShape).selectableGroup()) {
        labels.forEachIndexed { i, label ->
            val on = i == selected
            Box(
                Modifier
                    .heightIn(min = 48.dp)
                    .selectable(on, role = Role.Tab) { choose(i) }
                    .padding(4.dp)
                    .background(if (on) t.actBg else Color.Transparent, CircleShape)
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = if (on) t.actFg else t.tx, maxLines = 1)
            }
        }
    }
}

/** A reading's colour (ADR-0049): `ok` Good, `warn` Fair, `sos` Bad; `line` without one. */
@Composable
fun signalColor(r: SignalRating): Color = when (r) {
    SignalRating.GOOD -> lichen.ok
    SignalRating.FAIR -> lichen.warn
    SignalRating.BAD -> lichen.sos
    SignalRating.NONE -> lichen.line
}

/** "Signal good, 7.5 dB", as TalkBack reads a meter. */
@Composable
fun signalSaid(r: SignalRating, snr: Float?): String? {
    if (r == SignalRating.NONE || snr == null) return null
    return stringResource(
        when (r) {
            SignalRating.GOOD -> R.string.signal_good
            SignalRating.FAIR -> R.string.signal_fair
            else -> R.string.signal_bad
        },
        dbText(snr),
    )
}

/** A node's signal (ADR-0049): three bars, Good 3 in `ok`, Fair 2 in `warn`, Bad 1 in `sos`, the
 * rest in `line`, over the SNR in tabular digits. Nothing without a reading. [large] for a
 * node's page, small for a row. */
@Composable
fun SignalMeter(rating: SignalRating, snrDb: Float?, modifier: Modifier = Modifier, large: Boolean = false) {
    val said = signalSaid(rating, snrDb) ?: return
    val lit = when (rating) {
        SignalRating.GOOD -> 3
        SignalRating.FAIR -> 2
        else -> 1
    }
    val on = signalColor(rating)
    val off = lichen.line
    val unit = if (large) 7.dp else 4.6.dp
    Column(
        modifier.clearAndSetSemantics { contentDescription = said },
        horizontalAlignment = Alignment.End,
        verticalArrangement = Arrangement.spacedBy(if (large) 4.dp else 3.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(if (large) 3.dp else 2.dp), verticalAlignment = Alignment.Bottom) {
            for (i in 1..3) {
                Box(
                    Modifier
                        .width(if (large) 6.dp else 4.dp)
                        .height(unit * (i + 0.3f))
                        .background(if (i <= lit) on else off, RoundedCornerShape(1.5.dp)),
                )
            }
        }
        Text(
            dbText(snrDb!!),
            fontSize = if (large) 15.sp else 13.sp,
            fontWeight = FontWeight.SemiBold,
            color = lichen.tx,
            style = TextStyle(fontFeatureSettings = "tnum"),
            maxLines = 1,
        )
    }
}

/** "1 hop", "2 hops", "path unknown": how far a node is through repeaters. */
@Composable
fun HopPill(hops: UByte?, modifier: Modifier = Modifier) {
    val t = lichen
    val text = when (hops) {
        null -> stringResource(R.string.path_unknown)
        0.toUByte() -> stringResource(R.string.hops_direct)
        else -> pluralStringResource(R.plurals.hops_n, hops.toInt(), hops.toInt())
    }
    Text(
        text,
        fontSize = 12.sp,
        fontWeight = FontWeight.SemiBold,
        color = t.tx2,
        maxLines = 1,
        modifier = modifier.background(t.s2, RoundedCornerShape(8.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
    )
}

/** "lichen", with a lock: a node whose identity is known (a lichen user). */
@Composable
fun LichenBadge() {
    val t = lichen
    Row(
        Modifier.background(t.s2, RoundedCornerShape(6.dp)).padding(horizontal = 6.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Icon(Icons.Outlined.Lock, null, tint = t.tx2, modifier = Modifier.size(10.dp))
        Text(stringResource(R.string.app_name), fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = t.tx2, maxLines = 1)
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
        // Sized by the circle, not the font scale: larger text would not fit it.
        Text(initials(name), color = fg, fontSize = with(LocalDensity.current) { (size * 0.32f).toSp() }, fontWeight = FontWeight.SemiBold, maxLines = 1)
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
fun Banner(
    kind: BannerKind,
    title: String,
    message: String? = null,
    action: Pair<String, () -> Unit>? = null,
    /** A second choice: both then sit under the text ("Compare", "Later"). */
    second: Pair<String, () -> Unit>? = null,
) {
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
            if (action != null && second != null) {
                Row(Modifier.align(Alignment.End), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    TextButton(onClick = second.second) { Text(second.first, color = t.tx) }
                    TextButton(onClick = action.second) { Text(action.first, fontWeight = FontWeight.SemiBold, color = t.tx) }
                }
            }
        }
        if (action != null && second == null) {
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
fun CardRow(
    title: String,
    subtitle: String? = null,
    /** Off when the row sits in something clickable, which then takes its words (a merged row
     * inside would leave the button without a label). */
    merge: Boolean = true,
    /** The row's icon on its tile ([IconTile]), before the words. */
    tile: (@Composable () -> Unit)? = null,
    trailing: @Composable () -> Unit = {},
) {
    Row(
        Modifier
            .fillMaxWidth()
            // One node for screen readers: the title, the subtitle and a switch's state together.
            .then(if (merge) Modifier.semantics(mergeDescendants = true) {} else Modifier)
            .heightIn(min = 48.dp)
            .padding(horizontal = 16.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        tile?.invoke()
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = lichen.tx)
            if (subtitle != null) Text(subtitle, fontSize = 13.sp, color = lichen.tx2)
        }
        CompositionLocalProvider(LocalRowTitle provides title, content = trailing)
    }
}

/** The title of the row a control sits in: a switch says it to TalkBack, which reads the switch
 * apart from its row. */
val LocalRowTitle = staticCompositionLocalOf<String?> { null }

/** A row's icon on a small tinted tile (30 dp, as in the radio-screens mockups): LoRa's on
 * `loraBg`, iroh's on `irohBg`, Tor's on `torBg`, the others neutral (`s2` with `tx`). */
@Composable
fun IconTile(icon: Painter, background: Color = lichen.s2, tint: Color = lichen.tx, size: Dp = 30.dp) {
    Box(Modifier.size(size).background(background, RoundedCornerShape(size * 0.27f)), contentAlignment = Alignment.Center) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(size * 0.57f))
    }
}

@Composable
fun IconTile(icon: ImageVector, background: Color = lichen.s2, tint: Color = lichen.tx, size: Dp = 30.dp) =
    IconTile(rememberVectorPainter(icon), background, tint, size)

/** The inset of a divider between rows that have a tile: under their text. */
val TileInset = 58.dp

/** A node's MeshCore public key, in groups of 8 hex digits (F-1). Tapped (or its Copy), its 64
 * digits go to the clipboard and the row says "Copied"; with [share], Copy and Share buttons (Share
 * sends the digits to another app). */
@Composable
fun MeshCoreKeyRow(key: ByteArray, share: Boolean = false, tile: (@Composable () -> Unit)? = null) {
    val t = lichen
    val context = LocalContext.current
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    val hex = key.joinToString("") { "%02x".format(it) }
    var copied by remember(hex) { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(2_000)
            copied = false
        }
    }
    fun copy() {
        scope.launch { clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("lichen", hex))) }
        copied = true
    }
    Box(Modifier.clickable(onClickLabel = stringResource(R.string.copy)) { copy() }) {
        CardRow(
            stringResource(R.string.meshcore_public_key),
            if (copied) stringResource(R.string.copied) else hex.chunked(8).joinToString(" "),
            merge = false,
            tile = tile,
        ) {
            if (share) {
                IconButton(onClick = { copy() }) { Icon(Icons.Outlined.ContentCopy, stringResource(R.string.copy), tint = t.tx2) }
                IconButton(onClick = {
                    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, hex)
                    context.startActivity(Intent.createChooser(send, null))
                }) { Icon(Icons.Outlined.Share, stringResource(R.string.share), tint = t.tx2) }
            }
        }
    }
}

/** A section label above a card ("Radio", "Members"). */
@Composable
fun SectionLabel(text: String, modifier: Modifier = Modifier) {
    // On more lines with larger text rather than cut.
    Text(
        text,
        fontSize = 13.sp,
        color = lichen.tx2,
        modifier = modifier.fillMaxWidth().padding(horizontal = 4.dp),
    )
}

/** Text in [content] grows with the system's font size up to [max] times, no further: short
 * labels in a bar of fixed width (the tabs, the Map's modes), which would be cut or broken in
 * the middle of a word beyond it. */
@Composable
fun CappedFontScale(max: Float, content: @Composable () -> Unit) {
    val d = LocalDensity.current
    if (d.fontScale <= max) content() else CompositionLocalProvider(LocalDensity provides Density(d.density, max), content = content)
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

/** A media card's width in a conversation: `base` at the default text size, growing with the
 * system's font size like the text around it, but never wider than the bubble's room. */
@Composable
fun mediaWidth(base: Dp = 250.dp): Dp {
    val density = LocalDensity.current
    val room = with(density) { LocalWindowInfo.current.containerSize.width.toDp() } - 92.dp
    return minOf(base * density.fontScale.coerceAtLeast(1f), room.coerceAtLeast(base))
}

/** A title at the start and its value at the end of one line; the value under the title when
 * the line can't hold both. */
@Composable
fun TitleAndValue(title: @Composable () -> Unit, value: @Composable () -> Unit, modifier: Modifier = Modifier) {
    Layout(contents = listOf(title, value), modifier = modifier) { (titles, values), c ->
        val gap = 12.dp.roundToPx()
        val loose = c.copy(minWidth = 0, minHeight = 0)
        val tm = titles.first()
        val vm = values.first()
        if (tm.maxIntrinsicWidth(c.maxHeight) + gap + vm.maxIntrinsicWidth(c.maxHeight) <= c.maxWidth) {
            val tp = tm.measure(loose)
            val vp = vm.measure(loose)
            val h = maxOf(tp.height, vp.height)
            layout(c.maxWidth, h) {
                tp.place(0, (h - tp.height) / 2)
                vp.place(c.maxWidth - vp.width, (h - vp.height) / 2)
            }
        } else {
            val tp = tm.measure(loose)
            val vp = vm.measure(loose)
            val space = 2.dp.roundToPx()
            layout(c.maxWidth, tp.height + space + vp.height) {
                tp.place(0, 0)
                vp.place(0, tp.height + space)
            }
        }
    }
}

/** A text and the button that goes with it, side by side, the button at the end; the button
 * under the text when beside it the text would keep less than about half the width (larger
 * text, French), which a column of a word or two per line reads badly. */
@Composable
fun SideOrUnder(modifier: Modifier = Modifier, text: @Composable () -> Unit, action: @Composable () -> Unit) {
    Layout(contents = listOf(text, action), modifier = modifier) { (texts, actions), c ->
        val gap = 12.dp.roundToPx()
        val loose = c.copy(minWidth = 0, minHeight = 0)
        val button = actions.firstOrNull()?.measure(loose)
        val room = c.maxWidth - (button?.width ?: 0) - gap
        if (button == null || room >= c.maxWidth * 0.55f) {
            val words = texts.first().measure(loose.copy(maxWidth = if (button == null) c.maxWidth else room))
            val h = maxOf(words.height, button?.height ?: 0)
            layout(c.maxWidth, h) {
                words.place(0, (h - words.height) / 2)
                button?.place(c.maxWidth - button.width, (h - button.height) / 2)
            }
        } else {
            val words = texts.first().measure(loose)
            val between = 8.dp.roundToPx()
            layout(c.maxWidth, words.height + between + button.height) {
                words.place(0, 0)
                button.place(0, words.height + between)
            }
        }
    }
}
