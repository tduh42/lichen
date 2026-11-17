package com.tduhamel.lichen.model

import android.content.res.Resources
import android.icu.text.MeasureFormat
import android.icu.util.Measure
import android.icu.util.MeasureUnit
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
    is LichenException.InvalidSetting -> res.getString(R.string.err_invalid_setting)
    is LichenException.NotAnExport -> res.getString(R.string.err_not_an_export)
    is LichenException.IdentityPending -> res.getString(R.string.err_identity_pending)
    is LichenException.NotABackup -> res.getString(R.string.err_not_a_backup)
    is LichenException.NewerBackup -> res.getString(R.string.err_newer_backup)
    is LichenException.WrongBackupWords -> res.getString(R.string.err_wrong_backup_words)
    is LichenException.BackupDamaged -> res.getString(R.string.err_backup_damaged)
    is LichenException.NoPath -> res.getString(R.string.err_no_path)
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

/** One line for a message: its text, or what it is. `me`: our node, for notices ("You"). */
fun Message.display(res: Resources, me: ByteArray? = null): String = when (this) {
    is Message.Notice -> noticeText(res, me) { _, name -> name }
    is Message.Text -> text
    is Message.Voice -> res.getString(R.string.voice_message_length, clock(durationMs.toDouble() / 1000))
    is Message.Image -> res.getString(R.string.photo)
    is Message.Announce -> res.getString(if (kind.toInt() == 3) R.string.file_named else R.string.photo_named, name)
    is Message.File -> res.getString(R.string.file_named, name)
    is Message.Sticker -> res.getString(R.string.sticker_named, emoji)
    is Message.Gif -> if (title.isEmpty()) res.getString(R.string.gif) else res.getString(R.string.gif_named, title)
    is Message.Deleted -> res.getString(R.string.message_deleted)
    is Message.Other ->
        if (contentType.toInt() == 0x03) {
            res.getString(R.string.position)
        } else {
            res.getString(R.string.unsupported_content, contentType.toInt(), body.size)
        }
}

/**
 * A membership notice as a sentence (ADR-0021): "You" for ourselves (`me`, our node), the
 * others named by `name(node, name then)`.
 */
fun Message.Notice.noticeText(res: Resources, me: ByteArray?, name: (ByteArray, String) -> String): String {
    val mine = me != null && actor.contentEquals(me)
    val who = name(actor, actorName)
    val names = subjects.map { name(it.node, it.name) }.let { list ->
        when (list.size) {
            0 -> ""
            1 -> list[0]
            else -> res.getString(R.string.names_and, list.dropLast(1).joinToString(", "), list.last())
        }
    }
    return when (kind.toInt()) {
        1 -> if (mine) res.getString(R.string.notice_you_created) else res.getString(R.string.notice_created, who)
        2 -> res.getString(R.string.notice_added_you, who)
        3 -> res.getString(R.string.notice_joined, names)
        4 -> res.getString(R.string.notice_left, names)
        5 -> if (mine) res.getString(R.string.notice_you_removed, names) else res.getString(R.string.notice_removed, who, names)
        6 -> res.getString(R.string.notice_gone, names)
        7 -> res.getString(R.string.notice_removed_you, who)
        8 -> if (mine) res.getString(R.string.notice_you_closed) else res.getString(R.string.notice_closed, who)
        9 -> if (mine) res.getString(R.string.notice_you_renamed, text) else res.getString(R.string.notice_renamed, who, text)
        10 -> res.getString(R.string.notice_you_left)
        11 -> {
            val seconds = text.toUIntOrNull() ?: 0u
            when {
                seconds == 0u && mine -> res.getString(R.string.notice_you_timer_off)
                seconds == 0u -> res.getString(R.string.notice_timer_off, who)
                mine -> res.getString(R.string.notice_you_timer, timerLabel(res, seconds))
                else -> res.getString(R.string.notice_timer, who, timerLabel(res, seconds))
            }
        }
        // The name shown before (SR-30): a verified contact keeps it, the line says what changed.
        12 -> res.getString(R.string.notice_node_renamed, actorName, text)
        // A handover (§10.8): the former admin acts, the successor is the subject.
        40 -> when {
            mine -> res.getString(R.string.notice_you_made_admin, names)
            me != null && subjects.firstOrNull()?.node?.contentEquals(me) == true -> res.getString(R.string.notice_made_you_admin, who)
            else -> res.getString(R.string.notice_made_admin, who, names)
        }
        else -> res.getString(R.string.notice_changed)
    }
}

/** A disappearing-message timer in words: "5 minutes", "1 week" (§6.5). */
fun timerLabel(res: Resources, seconds: UInt): String {
    val s = seconds.toLong()
    val (n, unit) = when {
        s >= 604_800 && s % 604_800 == 0L -> s / 604_800 to MeasureUnit.WEEK
        s >= 86_400 && s % 86_400 == 0L -> s / 86_400 to MeasureUnit.DAY
        s >= 3_600 && s % 3_600 == 0L -> s / 3_600 to MeasureUnit.HOUR
        else -> (s + 59) / 60 to MeasureUnit.MINUTE
    }
    return MeasureFormat.getInstance(res.configuration.locales[0], MeasureFormat.FormatWidth.WIDE).format(Measure(n, unit))
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

/** When the sender wrote it (Unix ms), when the message carries it. */
val StoredMessage.writtenAtMs: Long?
    get() {
        val sent = when (val m = message) {
            is Message.Text -> m.sentAt
            is Message.Voice -> m.sentAt
            else -> return null
        }.toLong() * 1000
        return sent.takeIf { it > 0 }
    }

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
