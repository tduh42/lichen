package com.tduhamel.lichen.ui

import android.content.Context
import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.widget.ImageView
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import com.airbnb.lottie.LottieComposition
import com.airbnb.lottie.LottieCompositionFactory
import com.airbnb.lottie.compose.LottieAnimation
import com.airbnb.lottie.compose.animateLottieCompositionAsState
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.model.AppModel
import org.json.JSONArray
import java.nio.ByteBuffer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.zip.Inflater
import kotlin.math.roundToInt

/**
 * The animated stickers the app ships (protocol.md §6.3, ADR-0025): Noto Animated Emoji as
 * Lottie animations, stored in raw DEFLATE, found by their emoji.
 */
object Stickers {
    class Entry(val emoji: String, val file: String, val group: String)

    @Volatile private var entries: List<Entry>? = null
    private val cache = java.util.concurrent.ConcurrentHashMap<String, LottieComposition>()

    /** Already parsed: shown at once. */
    fun cached(emoji: String): LottieComposition? = cache[key(emoji)]

    @Synchronized
    fun all(context: Context): List<Entry> = entries ?: runCatching {
        val json = context.assets.open("noto/manifest.json").use { it.readBytes().decodeToString() }
        val a = JSONArray(json)
        List(a.length()) { i ->
            val o = a.getJSONObject(i)
            Entry(o.getString("emoji"), o.getString("file"), o.getString("group"))
        }
    }.getOrDefault(emptyList()).also { entries = it }

    /** Without the variation selector: "❤" and "❤️" are the same sticker. */
    private fun key(emoji: String) = emoji.replace("️", "")

    /** Parses the animation (tens of milliseconds): off the main thread. */
    fun composition(context: Context, emoji: String): LottieComposition? {
        val k = key(emoji)
        cache[k]?.let { return it }
        val entry = all(context).firstOrNull { key(it.emoji) == k } ?: return null
        return runCatching {
            val packed = context.assets.open("noto/${entry.file}").use { it.readBytes() }
            val json = inflate(packed).decodeToString()
            LottieCompositionFactory.fromJsonStringSync(json, "sticker-$k").value
        }.getOrNull()?.also { cache[k] = it }
    }

    /** Raw DEFLATE (RFC 1951), as the files are stored. */
    private fun inflate(data: ByteArray): ByteArray {
        val inflater = Inflater(true)
        inflater.setInput(data)
        val out = java.io.ByteArrayOutputStream(data.size * 8)
        val buf = ByteArray(64 * 1024)
        while (!inflater.finished()) {
            val n = inflater.inflate(buf)
            if (n == 0 && (inflater.needsInput() || inflater.needsDictionary())) break
            out.write(buf, 0, n)
        }
        inflater.end()
        return out.toByteArray()
    }
}

/** The animation of `emoji`, parsed off the main thread; null until then (or without it). */
@Composable
private fun rememberSticker(emoji: String): LottieComposition? {
    val context = LocalContext.current.applicationContext
    val state = produceState(Stickers.cached(emoji), emoji) {
        if (value == null) value = withContext(Dispatchers.Default) { Stickers.composition(context, emoji) }
    }
    return state.value
}

/** A sticker: its animation played three times, again on a tap; the plain emoji when this
 * phone does not have it (a newer set, §6.3). */
@Composable
fun StickerView(emoji: String, size: Dp = 132.dp) {
    val composition = rememberSticker(emoji)
    var replay by remember { mutableIntStateOf(0) }
    val label = stringResource(R.string.sticker_named, emoji)
    Box(
        Modifier.size(size).clickable { replay++ }.semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        if (composition != null) {
            // A tap starts it again.
            key(replay) {
                val progress by animateLottieCompositionAsState(composition, iterations = 3)
                LottieAnimation(composition, { progress }, Modifier.fillMaxSize())
            }
        } else {
            Text(emoji, fontSize = (size.value * 0.6f).sp)
        }
    }
}

/** A still sticker for the picker (dozens at once would cost too much animating). */
@Composable
fun StickerThumb(emoji: String, size: Dp = 56.dp) {
    val composition = rememberSticker(emoji)
    Box(Modifier.size(size), contentAlignment = Alignment.Center) {
        if (composition != null) {
            LottieAnimation(composition, { 0.4f }, Modifier.fillMaxSize())
        } else {
            Text(emoji, fontSize = (size.value * 0.6f).sp)
        }
    }
}

/** The sticker picker: tap one to send it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StickerSheet(send: (String) -> Unit, done: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    val groups = remember { Stickers.all(context).groupBy { it.group } }
    ModalBottomSheet(onDismissRequest = done, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Text(stringResource(R.string.stickers), fontSize = 22.sp, color = t.tx, modifier = Modifier.padding(horizontal = 16.dp))
        LazyVerticalGrid(
            GridCells.Fixed(5),
            Modifier.fillMaxWidth().padding(horizontal = 12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            for ((group, entries) in groups) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    SectionLabel(stringResource(groupTitle(group)), Modifier.padding(top = 10.dp))
                }
                items(entries, key = { it.file }) { e ->
                    val label = stringResource(R.string.sticker_named, e.emoji)
                    Box(
                        Modifier.height(64.dp).clickable {
                            send(e.emoji)
                            done()
                        }.semantics { contentDescription = label },
                        contentAlignment = Alignment.Center,
                    ) { StickerThumb(e.emoji) }
                }
            }
            item(span = { GridItemSpan(maxLineSpan) }) {
                Text(stringResource(R.string.sticker_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(vertical = 12.dp))
            }
        }
    }
}

private fun groupTitle(group: String) = when (group) {
    "faces" -> R.string.sticker_faces
    "hands" -> R.string.sticker_hands
    "hearts" -> R.string.sticker_hearts
    "celebrations" -> R.string.sticker_celebrations
    "outdoors" -> R.string.sticker_outdoors
    "animals" -> R.string.sticker_animals
    else -> R.string.sticker_signs
}

/**
 * An animated GIF or WebP, looping (decoded by the platform, API 28+): once, in the background,
 * within bounds (review of 2026-11-15, SR-27). A first frame of more than 16,384 pixels a side
 * or 64 million in all is refused, and the animation is scaled to 480 pixels at most.
 */
@Composable
fun AnimatedImage(data: ByteArray, modifier: Modifier = Modifier) {
    val drawable = rememberInBackground(data) {
        ImageDecoder.decodeDrawable(ImageDecoder.createSource(ByteBuffer.wrap(data))) { d, info, _ ->
            val (w, h) = info.size.width to info.size.height
            require(w in 1..16_384 && h in 1..16_384 && w.toLong() * h <= 64_000_000L)
            val scale = minOf(1.0, 480.0 / maxOf(w, h))
            d.setTargetSize(maxOf(1, (w * scale).roundToInt()), maxOf(1, (h * scale).roundToInt()))
        }
    }
    AndroidView(
        factory = { ctx -> ImageView(ctx).apply { scaleType = ImageView.ScaleType.CENTER_CROP } },
        modifier = modifier,
        update = { v ->
            if (v.drawable !== drawable) {
                v.setImageDrawable(drawable)
                (drawable as? AnimatedImageDrawable)?.start()
            }
        },
    )
}

/** The pixel size of an image's first frame: the shape a GIF message carries (§6.3). */
fun imageSize(data: ByteArray): Pair<UShort, UShort>? = runCatching {
    var size: Pair<UShort, UShort>? = null
    ImageDecoder.decodeDrawable(ImageDecoder.createSource(ByteBuffer.wrap(data))) { _, info, _ ->
        size = info.size.width.coerceIn(0, 65_535).toUShort() to info.size.height.coerceIn(0, 65_535).toUShort()
    }
    size
}.getOrNull()

/** A GIF in the conversation (§6.3): loaded from GIPHY by its id, on a tap unless GIFs load
 * automatically (or through Tor); its title until then, and its source. */
@Composable
fun GifCard(model: AppModel, m: StoredMessage, gif: Message.Gif) {
    val t = lichen
    var data by remember(gif.id) { mutableStateOf(model.cachedGif(gif.id)) }
    var loading by remember(gif.id) { mutableStateOf(false) }
    var failed by remember(gif.id) { mutableStateOf(false) }
    var asked by remember(gif.id) { mutableIntStateOf(0) }
    LaunchedEffect(gif.id, asked) {
        if (gif.provider.toInt() != 1 || data != null) return@LaunchedEffect
        if (asked == 0 && !model.gifsLoadThemselves) return@LaunchedEffect
        loading = true
        failed = false
        data = model.loadGif(gif.id)
        failed = data == null
        loading = false
    }
    val label = if (gif.title.isEmpty()) stringResource(R.string.gif) else stringResource(R.string.gif_named, gif.title)
    Column(horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
        Box(
            Modifier.width(220.dp).height(gifHeight(gif.width.toInt(), gif.height.toInt()))
                .clip(RoundedCornerShape(16.dp)).background(t.s2)
                .clickable(enabled = data == null && !loading && gif.provider.toInt() == 1) { asked++ }
                .semantics { contentDescription = label },
            contentAlignment = Alignment.Center,
        ) {
            val shown = data
            when {
                shown != null -> AnimatedImage(shown, Modifier.fillMaxSize())
                loading -> CircularProgressIndicator(Modifier.size(22.dp), color = t.tx2, strokeWidth = 2.dp)
                else -> Column(Modifier.padding(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(gif.title.ifEmpty { "GIF" }, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx, textAlign = TextAlign.Center, maxLines = 2)
                    Text(
                        stringResource(
                            when {
                                gif.provider.toInt() != 1 -> R.string.gif_unknown_source
                                failed -> R.string.gif_failed
                                else -> R.string.gif_tap_to_load
                            },
                        ),
                        fontSize = 12.sp, color = t.tx2, textAlign = TextAlign.Center,
                    )
                }
            }
        }
        Text("GIPHY", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = t.tx2, modifier = Modifier.padding(top = 2.dp))
    }
}

/** The card's height for a GIF of `w` × `h`, 220 dp wide; square when unknown. */
private fun gifHeight(w: Int, h: Int): Dp =
    if (w <= 0 || h <= 0) 220.dp else (220f * h / w).coerceIn(90f, 300f).dp
