package com.tduhamel.lichen.model

import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ChannelCode
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.LinkKind
import com.tduhamel.lichen.core.NodeInfo
import com.tduhamel.lichen.core.PathHop
import com.tduhamel.lichen.core.SavedRadio
import com.tduhamel.lichen.core.ServerInfo
import com.tduhamel.lichen.core.TraceInfo

// Mesh tools (protocol.md §15.8, §16; ADR-0041 to ADR-0044): the nodes the radio knows, paths
// and traces, room servers and repeaters, saved radios, channels by QR code.

fun AppModel.node(key: ByteArray): NodeInfo? = nodes.firstOrNull { it.key.contentEquals(key) }

/** The radio's path to [key]: null when it floods, empty when it hears it directly. */
fun AppModel.nodePath(key: ByteArray): List<PathHop>? {
    meshToolsVersion
    return runCatching { core.nodePath(key) }.getOrNull()
}

fun AppModel.trace(key: ByteArray): TraceInfo? {
    meshToolsVersion
    return core.trace(key)
}

fun AppModel.server(key: ByteArray): ServerInfo? {
    meshToolsVersion
    return runCatching { core.server(key) }.getOrNull()
}

/** Runs a mesh request: an error message to show, or null. */
private fun AppModel.meshCall(f: () -> Unit): String? = try {
    f()
    meshToolsVersion++
    pump()
    null
} catch (e: LichenException) {
    e.display(app.resources)
}

fun AppModel.startTrace(key: ByteArray): String? = meshCall { core.startTrace(key, now()) }

fun AppModel.serverLogin(key: ByteArray, password: String, remember: Boolean): String? =
    meshCall { core.serverLogin(key, password, remember, now()) }

fun AppModel.serverLogout(key: ByteArray) {
    meshCall { core.serverLogout(key) }
}

fun AppModel.requestServerStats(key: ByteArray): String? = meshCall { core.requestServerStats(key, now()) }

fun AppModel.requestNeighbours(key: ByteArray): String? = meshCall { core.requestNeighbours(key, now()) }

fun AppModel.sendServerCommand(key: ByteArray, command: String): String? {
    val sentAt = (wallNow() / 1000).toUInt()
    return meshCall { core.sendServerCommand(key, command, sentAt, now()) }
}

/** "hub", "hub, crest", or the hash when no repeater has it. */
fun PathHop.display(): String = if (names.isEmpty()) hash.uppercase() else names.joinToString(", ")

// ----- saved radios (ADR-0042) -----

/** The radio answered: save it with how to reach it again. */
fun AppModel.radioConnected() {
    if (!isDemo) {
        val (address, kind) = when (linkKind) {
            "TCP" -> tcp.connectedTo to LinkKind.TCP
            "USB" -> usb.device?.deviceName to LinkKind.USB
            else -> ble.address to LinkKind.BLUETOOTH
        }
        if (address != null) runCatching { core.rememberRadio(address, kind, nodeName) }
    }
    savedRadios = core.savedRadios()
}

fun AppModel.refreshSavedRadios() {
    savedRadios = core.savedRadios()
}

/** Leaves the radio connected now for [radio]. */
fun AppModel.switchRadio(radio: SavedRadio) {
    if (isConnected) disconnect()
    when (radio.link) {
        LinkKind.BLUETOOTH -> pair(radio.address)
        LinkKind.TCP -> connectTcp(radio.address)
        LinkKind.USB -> usbDevices.firstOrNull { it.deviceName == radio.address }?.let { connectUsb(it) }
    }
}

/** Whether [radio] can be reached now (a USB radio must be plugged in). */
fun AppModel.canSwitchTo(radio: SavedRadio): Boolean =
    !radio.current && (radio.link != LinkKind.USB || usbDevices.any { it.deviceName == radio.address })

fun AppModel.forgetRadio(radio: SavedRadio) {
    if (radio.current) disconnect()
    runCatching { core.forgetRadio(radio.address) }
    savedRadios = core.savedRadios()
}

/** The radio's battery is low enough to warn (≤ 15 %). */
val AppModel.radioBatteryLow: Boolean get() = isConnected && radioSettings?.batteryLow == true

// ----- channels by QR code (protocol.md §15.3) -----

fun AppModel.channelQr(id: ByteArray): String? = runCatching { core.channelQr(id) }.getOrNull()

fun AppModel.parseChannelQr(text: String): ChannelCode? = runCatching { core.parseChannelQr(text) }.getOrNull()

/** "5 min ago", from a Unix time in seconds. */
fun AppModel.heard(unixS: ULong?): String {
    val res = app.resources
    if (unixS == null) return res.getString(R.string.never_heard)
    val ago = wallNow() / 1000 - unixS.toLong()
    return when {
        ago < 60 -> res.getString(R.string.just_now)
        ago < 3_600 -> res.getString(R.string.minutes_ago, ago / 60)
        ago < 86_400 -> res.getString(R.string.hours_ago, ago / 3_600)
        else -> res.getString(R.string.days_ago, ago / 86_400)
    }
}
