package com.tduhamel.lichen.model

import android.annotation.SuppressLint
import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.StoredMessage
import com.tduhamel.lichen.core.decodeVoice
import java.nio.ByteBuffer
import kotlin.math.max
import kotlin.math.roundToInt

/** Records a voice message: 8 kHz mono 16-bit, what Codec2 takes, 15 seconds at most. */
class VoiceRecorder {
    var recording by mutableStateOf(false)
        private set
    var elapsed by mutableStateOf(0.0)
        private set

    private val main = Handler(Looper.getMainLooper())
    private var record: AudioRecord? = null
    private var thread: Thread? = null
    private var samples = ShortArray(MAX_SAMPLES)
    @Volatile private var count = 0

    /** Starts recording (the RECORD_AUDIO permission is granted); false when the microphone
     * cannot be opened. */
    @SuppressLint("MissingPermission")
    fun start(): Boolean {
        if (recording) return true
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val r = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT, max(min, RATE),
        )
        if (r.state != AudioRecord.STATE_INITIALIZED) {
            r.release()
            return false
        }
        record = r
        samples = ShortArray(MAX_SAMPLES)
        count = 0
        elapsed = 0.0
        recording = true
        r.startRecording()
        if (r.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            r.release()
            record = null
            recording = false
            return false
        }
        thread = Thread({
            val buf = ShortArray(800)
            while (recording && count < MAX_SAMPLES) {
                val n = r.read(buf, 0, minOf(buf.size, MAX_SAMPLES - count))
                if (n > 0) {
                    buf.copyInto(samples, count, 0, n)
                    count += n
                    val t = count.toDouble() / RATE
                    main.post { elapsed = t }
                }
            }
        }, "lichen-voice").apply { start() }
        return true
    }

    /** Stops; the samples, or null when cancelled or empty. */
    fun stop(keep: Boolean): ShortArray? {
        if (!recording) return null
        recording = false
        thread?.join(500)
        thread = null
        record?.let {
            runCatching { it.stop() }
            it.release()
        }
        record = null
        return if (keep && count > 0) samples.copyOf(count) else null
    }

    companion object {
        const val RATE = 8_000
        const val MAX_SECONDS = 15
        private const val MAX_SAMPLES = RATE * MAX_SECONDS
    }
}

/** Plays voice messages: Codec2 frames decoded by the core. */
class VoicePlayer {
    /** The message playing, if any. */
    var playing by mutableStateOf<Long?>(null)
        private set

    private var track: AudioTrack? = null

    fun toggle(m: StoredMessage) {
        if (playing == m.id) {
            stop()
            return
        }
        stop()
        val v = m.message as? Message.Voice ?: return
        val pcm = decodeVoice(v.mode, v.frames, v.data)?.toShortArray() ?: return
        if (pcm.isEmpty()) return
        val t = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(VoiceRecorder.RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STATIC)
            .setBufferSizeInBytes(pcm.size * 2)
            .build()
        t.write(pcm, 0, pcm.size)
        t.notificationMarkerPosition = pcm.size
        t.setPlaybackPositionUpdateListener(object : AudioTrack.OnPlaybackPositionUpdateListener {
            override fun onMarkerReached(track: AudioTrack) {
                if (playing == m.id) stop()
            }

            override fun onPeriodicNotification(track: AudioTrack) = Unit
        })
        t.play()
        track = t
        playing = m.id
    }

    fun stop() {
        track?.let {
            runCatching { it.stop() }
            it.release()
        }
        track = null
        playing = null
    }
}

/** A picked photo: its bytes as they are (offered over the bridge), and RGBA8 pixels at most
 * `maxSide` on the longer side, for the core's AVIF thumbnail. */
class PickedPhoto(val original: ByteArray, val rgba: ByteArray, val width: Int, val height: Int)

fun pickPhoto(resolver: ContentResolver, uri: Uri, maxSide: Int = 640): PickedPhoto? = runCatching {
    val original = resolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(ByteBuffer.wrap(original))) { d, info, _ ->
        val scale = minOf(1.0, maxSide.toDouble() / max(info.size.width, info.size.height))
        d.setTargetSize(
            max(1, (info.size.width * scale).roundToInt()),
            max(1, (info.size.height * scale).roundToInt()),
        )
        d.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
    }.let { if (it.config == Bitmap.Config.ARGB_8888) it else it.copy(Bitmap.Config.ARGB_8888, false) }
    // ARGB_8888 is laid out R, G, B, A in memory, premultiplied: what the core takes.
    val buf = ByteBuffer.allocate(bitmap.byteCount)
    bitmap.copyPixelsToBuffer(buf)
    PickedPhoto(original, buf.array(), bitmap.width, bitmap.height)
}.getOrNull()

/** Decodes an image (AVIF thumbnails from Android 12, JPEG, PNG, HEIC…), or null. */
fun decodeImage(bytes: ByteArray): Bitmap? =
    runCatching { ImageDecoder.decodeBitmap(ImageDecoder.createSource(ByteBuffer.wrap(bytes))) }.getOrNull()

/** A file name from the image's format, as the iOS app names them. */
fun photoName(data: ByteArray, nowMs: Long = System.currentTimeMillis()): String {
    fun at(i: Int) = data.getOrNull(i)?.toInt()?.and(0xFF) ?: -1
    val brand = if (data.size >= 12) String(data, 4, 8, Charsets.US_ASCII) else ""
    val ext = when {
        at(0) == 0xFF && at(1) == 0xD8 -> "jpg"
        at(0) == 0x89 && at(1) == 0x50 && at(2) == 0x4E && at(3) == 0x47 -> "png"
        brand.startsWith("ftyphei") -> "heic"
        brand == "ftypavif" -> "avif"
        else -> "jpg"
    }
    val stamp = java.text.SimpleDateFormat("yyyy-MM-dd'T'HHmmss", java.util.Locale.ROOT)
        .apply { timeZone = java.util.TimeZone.getTimeZone("UTC") }
        .format(java.util.Date(nowMs))
    return "photo-$stamp.$ext"
}
