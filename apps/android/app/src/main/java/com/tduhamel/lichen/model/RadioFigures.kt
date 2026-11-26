package com.tduhamel.lichen.model

import android.content.res.Resources
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.RadioSettings
import java.text.NumberFormat
import kotlin.math.abs

/** The radio's own figures (protocol §16.4, `GET_STATS`) shown in Radio settings' "Since it
 * started", in their order there: only those the radio reported, never a 0 standing for one it
 * did not. */
enum class RadioFigure(val key: String) {
    UPTIME("uptime"),
    RECEIVED("packets_received"),
    SENT("packets_sent"),
    ERRORS("receive_errors"),
    AIRTIME_SENT("airtime_sent"),
    AIRTIME_RECEIVED("airtime_received"),
    LAST_PACKET("last_packet"),
}

/** The figures this radio reported. */
fun RadioSettings.figures(): List<RadioFigure> = buildList {
    if (uptimeS != null) add(RadioFigure.UPTIME)
    if (packetsReceived != null) add(RadioFigure.RECEIVED)
    if (packetsSent != null) add(RadioFigure.SENT)
    if (receiveErrors != null) add(RadioFigure.ERRORS)
    if (txAirtimeS != null) add(RadioFigure.AIRTIME_SENT)
    if (rxAirtimeS != null) add(RadioFigure.AIRTIME_RECEIVED)
    if (lastRssiDbm != null || lastSnrDb != null) add(RadioFigure.LAST_PACKET)
}

/** What a figure's row says after its title, in the current language: "3 d 4 h", "1,204",
 * "12 · 1 % of received", "4 min 12 s · 0.4 % of the time", "−92 dBm · 6.25 dB". */
fun RadioSettings.figureText(res: Resources, f: RadioFigure): String = when (f) {
    RadioFigure.UPTIME -> uptime(res, uptimeS?.toLong() ?: 0)
    RadioFigure.RECEIVED -> count(packetsReceived)
    RadioFigure.SENT -> count(packetsSent)
    RadioFigure.ERRORS -> {
        val errors = receiveErrors?.toLong() ?: 0
        shareOf(errors, packetsReceived?.toLong() ?: 0)?.let { res.getString(R.string.fig_errors_share, count(receiveErrors), it) }
            ?: count(receiveErrors)
    }
    RadioFigure.AIRTIME_SENT -> airtimeShare(res, txAirtimeS)
    RadioFigure.AIRTIME_RECEIVED -> airtimeShare(res, rxAirtimeS)
    RadioFigure.LAST_PACKET -> listOfNotNull(lastRssiDbm?.let { "${minus(it.toFloat())}${abs(it.toInt())} dBm" }, lastSnrDb?.let(::snrText))
        .joinToString(" · ")
}

private fun RadioSettings.airtimeShare(res: Resources, s: UInt?): String {
    val air = s?.toLong() ?: 0
    val text = airtime(res, air)
    return shareOf(air, uptimeS?.toLong() ?: 0)?.let { res.getString(R.string.fig_airtime_share, text, it) } ?: text
}

/** The noise floor as the header says it: "−118 dBm". */
fun dbmText(dbm: Short): String = "${minus(dbm.toFloat())}${abs(dbm.toInt())} dBm"

/** "1,204" ("1 204" in French). */
private fun count(n: UInt?): String = NumberFormat.getIntegerInstance().format(n?.toLong() ?: 0)

/** "6.25 dB", "−3.5 dB": as the radio gives it, up to two decimals. */
private fun snrText(db: Float): String =
    minus(db) + NumberFormat.getNumberInstance().apply { maximumFractionDigits = 2 }.format(abs(db)) + " dB"

private fun minus(v: Float) = if (v < 0) "−" else ""

/** A share in percent: one decimal under 1 % ("0.4"), none above ("12"); none of nothing. */
fun shareOf(part: Long, whole: Long): String? {
    if (whole <= 0) return null
    val p = part.toDouble() / whole * 100
    return if (p > 0 && p < 1) "%.1f".format(p) else "%.0f".format(p)
}

/** For how long the radio has been on: "3 d 4 h", "2 h 5 min", "42 min" (under a minute "42 s"). */
private fun uptime(res: Resources, s: Long): String =
    if (s < 60) res.getString(R.string.duration_seconds, s.toInt()) else duration(res, s)

/** Airtime: "42 s", "4 min 12 s", then as [duration]. */
private fun airtime(res: Resources, s: Long): String = when {
    s < 60 -> res.getString(R.string.duration_seconds, s.toInt())
    s < 3_600 -> res.getString(R.string.duration_minutes_seconds, (s / 60).toInt(), (s % 60).toInt())
    else -> duration(res, s)
}

/** "3 d 4 h", "2 h 5 min", "42 min": a repeater's or the radio's time up. */
fun duration(res: Resources, s: Long): String = when {
    s >= 86_400 -> res.getString(R.string.duration_days, s / 86_400, s % 86_400 / 3_600)
    s >= 3_600 -> res.getString(R.string.duration_hours, s / 3_600, s % 3_600 / 60)
    else -> res.getString(R.string.duration_minutes, s / 60)
}

/** Asks the radio for its figures again (at most every 60 s, the core decides); Radio settings
 * calls it while it is on screen, nothing else does. */
fun AppModel.refreshRadioStats() {
    if (runCatching { core.refreshRadioStats(now()) }.getOrDefault(false)) pump()
}
