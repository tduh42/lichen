package com.tduhamel.lichen.model

import android.content.res.Resources
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.BridgeException
import com.tduhamel.lichen.core.FailReason
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.Message
import com.tduhamel.lichen.core.MessageStatus
import com.tduhamel.lichen.core.StoredMessage
import kotlin.math.ceil
import kotlin.math.roundToInt

/** What the user reads for an error of the core. */
fun LichenException.display(res: Resources): String = when (this) {
    is LichenException.UnknownContact -> res.getString(R.string.err_unknown_contact)
    is LichenException.UnknownGroup -> res.getString(R.string.err_unknown_group)
    is LichenException.Empty -> res.getString(R.string.err_empty)
    is LichenException.TooLarge -> res.getString(R.string.err_too_large)
    is LichenException.NoIdentity -> res.getString(R.string.err_no_identity)
    is LichenException.IdentityUnknown -> res.getString(R.string.err_identity_unknown)
    is LichenException.BadCode -> res.getString(R.string.err_bad_code)
    is LichenException.NotAllowed -> res.getString(R.string.err_not_allowed)
    is LichenException.InvalidKey -> res.getString(R.string.err_invalid_key)
    is LichenException.OverBudget -> {
        val minutes = maxOf(1, ceil(waitS.toDouble() / 60).toInt())
        res.getString(R.string.err_over_budget, minutes)
    }
    is LichenException.MediaInGroup -> res.getString(R.string.err_media_in_group)
    is LichenException.BadImage -> res.getString(R.string.err_bad_image)
    is LichenException.NotAPreview -> res.getString(R.string.err_not_a_preview)
    is LichenException.NotSearching -> res.getString(R.string.err_not_searching)
    is LichenException.TooSoon -> res.getString(R.string.err_too_soon, waitS.toLong())
    is LichenException.NoPosition -> res.getString(R.string.err_no_position)
    is LichenException.Storage -> res.getString(R.string.err_storage)
    is LichenException.NotCancellable -> res.getString(R.string.err_not_cancellable)
    is LichenException.NotReferable -> res.getString(R.string.err_not_referable)
    is LichenException.NoClock -> res.getString(R.string.err_no_clock)
    is LichenException.Expired -> res.getString(R.string.err_invite_expired)
}

fun BridgeException.display(res: Resources): String = when (this) {
    is BridgeException.Unreachable -> res.getString(R.string.bridge_unreachable)
    is BridgeException.NotFound -> res.getString(R.string.bridge_not_found)
    is BridgeException.TooLarge -> res.getString(R.string.bridge_too_large)
    is BridgeException.Mismatch -> res.getString(R.string.bridge_mismatch)
    is BridgeException.Timeout -> res.getString(R.string.bridge_timeout)
    is BridgeException.Failed -> reason
}

fun FailReason.display(res: Resources): String = res.getString(
    when (this) {
        FailReason.NO_PATH -> R.string.fail_no_path
        FailReason.UNREACHABLE -> R.string.fail_unreachable
        FailReason.TIMEOUT -> R.string.fail_timeout
        FailReason.REFUSED -> R.string.fail_refused
        FailReason.TOO_LARGE -> R.string.fail_too_large
        FailReason.INTERRUPTED -> R.string.fail_interrupted
        FailReason.IDENTITY_CHANGED -> R.string.fail_identity_changed
        FailReason.EXPIRED -> R.string.fail_expired
    },
)

/** One line for a message: its text, or what it is. */
fun Message.display(res: Resources): String = when (this) {
    is Message.Text -> text
    is Message.Voice -> res.getString(R.string.voice_message_length, clock(durationMs.toDouble() / 1000))
    is Message.Image -> res.getString(R.string.photo)
    is Message.Announce -> res.getString(R.string.photo_named, name)
    is Message.Other ->
        if (contentType.toInt() == 0x03) {
            res.getString(R.string.position)
        } else {
            res.getString(R.string.unsupported_content, contentType.toInt(), body.size)
        }
}

/** "LoRa", "LoRa · direct", "LoRa · 2 hops". */
fun hopsBadge(res: Resources, hops: UByte?): String = when {
    hops == null -> res.getString(R.string.lora)
    hops.toInt() == 0 -> res.getString(R.string.lora_direct)
    else -> res.getQuantityString(R.plurals.lora_hops, hops.toInt(), hops.toInt())
}

/** "3 fragments", "1 fragment". */
fun fragmentsText(res: Resources, n: Int): String = res.getQuantityString(R.plurals.fragments, n, n)

/** "0:06". */
fun clock(seconds: Double): String {
    val s = seconds.roundToInt()
    return "%d:%02d".format(s / 60, s % 60)
}

/** Seconds with one decimal, in the user's locale ("0.4", "0,4"). */
fun secondsText(ms: ULong): String = "%.1f".format(ms.toDouble() / 1000)

/** "62.5 kHz", "62,5 kHz" in French. */
fun bandwidth(hz: UInt): String {
    val khz = hz.toDouble() / 1000
    return (if (khz % 1.0 == 0.0) "%.0f" else "%.1f").format(khz) + " kHz"
}

/** "CA" for "Camille", "RV" for "Rando Vercors". */
fun initials(name: String): String {
    val words = name.split(" ").filter { it.isNotEmpty() }
    val letters = when {
        words.isEmpty() -> ""
        words.size == 1 -> words[0].take(2)
        else -> words.take(2).joinToString("") { it.take(1) }
    }
    return letters.ifEmpty { "?" }.uppercase()
}

/** Accepted and waiting (for the radio, a path, a session…), §6.1. */
val StoredMessage.isWaiting: Boolean get() = status is MessageStatus.Waiting

/** When the sender wrote it (Unix ms), if well before it arrived: a message that waited (§6.1). */
val StoredMessage.writtenEarlierMs: Long?
    get() {
        if (outgoing) return null
        val sent = when (val m = message) {
            is Message.Text -> m.sentAt
            is Message.Voice -> m.sentAt
            else -> return null
        }.toLong() * 1000
        return if (sent > 0 && atMs - sent > 600_000) sent else null
    }
