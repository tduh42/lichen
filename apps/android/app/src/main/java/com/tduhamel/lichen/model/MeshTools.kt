package com.tduhamel.lichen.model

import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ChannelCode
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.LinkKind
import com.tduhamel.lichen.core.NodeInfo
import com.tduhamel.lichen.core.NodeReading
import com.tduhamel.lichen.core.ReadingsState
import com.tduhamel.lichen.core.NodeRoute
import com.tduhamel.lichen.core.SignalRating
import com.tduhamel.lichen.core.SignalReading
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

/** The route to [key] (protocol §16.3): our radio, the repeaters of its path, the node; each
 * leg's last SNR towards us, or none when nothing measured it. Read again when the nodes or a
 * trace change. */
fun AppModel.nodeRoute(key: ByteArray): NodeRoute? {
    meshToolsVersion
    nodes
    return runCatching { core.nodeRoute(key) }.getOrNull()
}

/** What our radio heard of [key] directly, the last 7 days (ADR-0049), oldest first. */
fun AppModel.nodeSignalHistory(key: ByteArray): List<SignalReading> {
    meshToolsVersion
    nodes
    return runCatching { core.nodeSignalHistory(key) }.getOrDefault(emptyList()).sortedBy { it.atS }
}

/** Where [key] shows on the map: its advertised position, else the farthest stop of its route
 * that advertises one; null when none does. */
fun AppModel.nodePlace(key: ByteArray): Pair<Double, Double>? {
    node(key)?.let { n -> if (n.lat != null && n.lon != null) return n.lat!! to n.lon!! }
    return nodeRoute(key)?.stops?.drop(1)?.lastOrNull { it.lat != null && it.lon != null }?.let { it.lat!! to it.lon!! }
}

/** How many stops of [key]'s route the map can place: ours at the phone, the others where they
 * advertise. Fewer than two: no route to draw. */
fun AppModel.routePlaces(key: ByteArray): Int {
    val route = nodeRoute(key) ?: return 0
    return (if (myFix != null) 1 else 0) + route.stops.drop(1).count { it.lat != null && it.lon != null }
}

/** The Map tab, in Positions mode, with [key]'s route (a node's page's Show on the map): the
 * camera takes the route in, or centres on the node when there is none to draw. */
fun AppModel.showNodeOnMap(key: ByteArray) {
    val (lat, lon) = nodePlace(key) ?: return
    mapRouteTo = key.hex()
    if (routePlaces(key) < 2) mapFocus = MapPin(lat, lon, (mapFocus?.version ?: 0) + (mapPin?.version ?: 0) + 1)
    tab = AppModel.Tab.MAP
    mapMode = AppModel.MapMode.POSITIONS
    openMap = true
}

/** Heard directly in the last 24 h (ADR-0049): the Nodes tab's "In reach". */
fun NodeInfo.inReach(nowS: Long): Boolean =
    signal != SignalRating.NONE && signalAtS?.let { nowS - it.toLong() <= 86_400 } == true

/** Heard at all, directly or not, in the last [seconds]. */
fun NodeInfo.heardWithin(nowS: Long, seconds: Long): Boolean = lastHeardS?.let { nowS - it.toLong() <= seconds } == true

private fun SignalRating.rank(): Int = when (this) {
    SignalRating.GOOD -> 0
    SignalRating.FAIR -> 1
    SignalRating.BAD -> 2
    SignalRating.NONE -> 3
}

/** The Nodes tab's two sections: in reach, by rating then SNR; through repeaters, by hops then
 * the last heard, the nodes with no known path last. */
fun nodeSections(nodes: List<NodeInfo>, nowS: Long): Pair<List<NodeInfo>, List<NodeInfo>> {
    val (reach, through) = nodes.partition { it.inReach(nowS) }
    return reach.sortedWith(compareBy<NodeInfo> { it.signal.rank() }.thenByDescending { it.snrDb ?: Float.NEGATIVE_INFINITY }) to
        through.sortedWith(
            compareBy<NodeInfo> { it.hops == null }.thenBy { it.hops?.toInt() ?: 0 }.thenByDescending { it.lastHeardS?.toLong() ?: 0L },
        )
}

/** "1.2 km NE", "300 m S" from the phone, when both positions are known. */
fun AppModel.distanceBearing(lat: Double?, lon: Double?): String? {
    if (lat == null || lon == null) return null
    val (d, b) = distanceTo(lat, lon) ?: return null
    val points = app.resources.getString(R.string.compass_points).split(',')
    val point = points[((b + 22.5) / 45).toInt().mod(8)]
    // "850 m", "2.4 km", in the phone's language (as the SOS screen writes it).
    val far = if (d < 1_000) "${kotlin.math.round(d).toInt()} m" else "%.1f km".format(d / 1000)
    return "$far $point"
}

/** An SNR as people read it: "7.5 dB", "−3.0 dB" (a true minus), the decimal of the language. */
fun dbText(snr: Float): String = (if (snr <= -0.05f) "−" else "") + "%.1f dB".format(kotlin.math.abs(snr))

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

// ----- a node's readings (§16.7, ADR-0050) -----

/** What [key] answered this radio over the last 7 days, oldest first. */
fun AppModel.nodeReadings(key: ByteArray): List<NodeReading> {
    meshToolsVersion
    nodes
    return runCatching { core.nodeReadings(key) }.getOrDefault(emptyList())
}

/** Where asking [key] for its readings stands (`NodeUpdated` follows each change). */
fun AppModel.nodeReadingsState(key: ByteArray): ReadingsState {
    meshToolsVersion
    return runCatching { core.nodeReadingsState(key) }.getOrDefault(ReadingsState.IDLE)
}

/** Asks [key] for its readings, by hand only (§16.7: nothing is asked by itself). Why the
 * radio would not is kept for the page until the next request ([AppModel.readingsRefusals]). */
fun AppModel.askReadings(key: ByteArray) {
    val k = key.hex()
    readingsRefusals = readingsRefusals - k
    try {
        core.requestNodeReadings(key, now())
        meshToolsVersion++
        pump()
    } catch (e: LichenException) {
        readingsRefusals = readingsRefusals + (k to e)
    }
}

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
