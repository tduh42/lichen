package com.tduhamel.lichen.model

import android.os.Build
import com.tduhamel.lichen.BuildConfig
import com.tduhamel.lichen.core.ReportApp
import com.tduhamel.lichen.core.ReportLine

/**
 * The app's recent log, for problem reports (ADR-0045): fixed words and numbers, never a name,
 * text, key or position (the core masks them anyway). Kept in memory, the last [MAX] lines.
 */
class AppLog(private val now: () -> ULong) {
    private val lines = ArrayDeque<ReportLine>()

    @Synchronized
    fun note(text: String) {
        lines.addLast(ReportLine(now(), text))
        while (lines.size > MAX) lines.removeFirst()
    }

    @Synchronized
    fun lines(): List<ReportLine> = lines.toList()

    companion object {
        const val MAX = 200
    }
}

/** The state in a word for the log, without the radio's name. */
val AppModel.RadioState.logWord: String
    get() = when (this) {
        AppModel.RadioState.Off -> "off"
        AppModel.RadioState.Idle -> "idle"
        AppModel.RadioState.Scanning -> "scanning"
        is AppModel.RadioState.Connecting -> "connecting"
        is AppModel.RadioState.Connected -> "connected"
    }

/**
 * The problem report: versions, the radio, settings, and the core's and the app's recent log,
 * without message content (ADR-0045).
 */
fun AppModel.problemReport(): String {
    val link = when {
        isDemo -> "demo"
        else -> when (linkKind) {
            "TCP" -> "TCP"
            "USB" -> "USB"
            else -> "Bluetooth"
        }
    }
    val settings = listOf(
        "iroh: " + if (!irohEnabled) "off" else if (relayOnly) "on, relays only" else "on",
        "Tor: ${if (torEnabled) "on" else "off"} (${torStatus.name.lowercase()})",
        "GIFs: " + if (gifAutoLoad) "load automatically" else "tap to load",
        "voice mode: $voiceMode",
        "radio details: " + if (radioDetails) "shown" else "behind a tap",
        "app lock: " + if (lock.enabled) "on" else "off",
        "language: ${java.util.Locale.getDefault().toLanguageTag()}",
    )
    val personal = buildList {
        add(myName)
        add(nodeName)
        addAll(found.map { it.name })
        tcp.target?.let { add(it.substringBeforeLast(':')) }
        usbDevices.forEach { d -> runCatching { d.productName }.getOrNull()?.let { add(it) } }
    }
    val app = ReportApp(
        appVersion = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
        platform = "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
        device = "${Build.MANUFACTURER} ${Build.MODEL}",
        link = link,
        settings = settings,
        log = log.lines(),
        privateWords = personal,
    )
    return core.problemReport(app, now())
}
