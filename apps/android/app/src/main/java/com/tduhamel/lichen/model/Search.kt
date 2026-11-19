package com.tduhamel.lichen.model

import android.content.Context
import android.graphics.Bitmap
import android.hardware.GeomagneticField
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.BatteryManager
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.PeerPosition
import com.tduhamel.lichen.core.SearchEstimate
import com.tduhamel.lichen.core.SearchMeasurement
import com.tduhamel.lichen.core.SearchState
import java.io.File
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.roundToInt
import kotlin.math.sin

/** An SOS heard from one person (protocol.md §11-12): its positions or, without GPS, its
 * beacons, until it ends. One alert per incident. */
data class SosIncident(
    val from: ByteArray,
    /** The group it came through (null: a direct share). */
    val group: ByteArray?,
    /** Its last position; null for an SOS without GPS (beacons only). */
    val position: PeerPosition?,
    val battery: UByte?,
    val started: Long,
    val lastHeard: Long,
    val ended: Boolean,
) {
    enum class State { ACTIVE, STALE, ENDED }

    /** Stale after five minutes without anything (beacons come every two). */
    fun state(now: Long = wallNow()): State = when {
        ended -> State.ENDED
        now - lastHeard > 5 * 60_000 -> State.STALE
        else -> State.ACTIVE
    }
}

// ----- SOS incidents -----

/** An SOS position or beacon from `from`: a new incident (alert, notification) or the current
 * one, kept up to date. */
fun AppModel.heardSos(from: ByteArray, group: ByteArray?, position: PeerPosition?, battery: UByte?) {
    val key = from.hex()
    val now = wallNow()
    val current = incidents[key]
    if (current != null && !current.ended) {
        incidents = incidents + (key to current.copy(
            lastHeard = now,
            position = position ?: current.position,
            group = group ?: current.group,
            battery = battery ?: current.battery,
        ))
        return
    }
    incidents = incidents + (key to SosIncident(from, group, position, battery, now, now, false))
    if (searchTarget?.let { incidents[it]?.state() } != SosIncident.State.ACTIVE) searchTarget = key
    // Debug extra `screen locate`: straight to the search, for screenshots.
    if (debugScreen == "locate") {
        startSearch(from)
        openSearch = key
        return
    }
    sosAlert = key
    Notifications.sos(app, key, name(from), position == null)
}

/** `from` left SOS (an end position, a normal one, or a beacon without the SOS bit). */
fun AppModel.endSos(from: ByteArray) {
    val key = from.hex()
    val i = incidents[key] ?: return
    if (i.ended) return
    incidents = incidents + (key to i.copy(ended = true))
    if (sosAlert == key) sosAlert = null
}

/** Incidents still going on, newest first. */
val AppModel.activeIncidents: List<SosIncident>
    get() = incidents.values.filter { it.state() != SosIncident.State.ENDED }.sortedByDescending { it.started }

// ----- searching (protocol.md §11) -----

/** The people the Locate tab can show (hex): incidents not ended, and every search we run. */
val AppModel.searchTargets: List<String>
    get() {
        val out = activeIncidents.map { it.from.hex() }.toMutableList()
        for (s in searches) if (s.node.hex() !in out) out.add(s.node.hex())
        searchTarget?.let { if (incidents[it] != null && it !in out) out.add(it) }
        return out
    }

fun AppModel.search(node: String): SearchState? = searches.firstOrNull { it.node.hex() == node }

/** The group to search `node` in: the incident's, else the first group with them. */
fun AppModel.searchGroup(node: String): ByteArray? {
    incidents[node]?.group?.let { g -> if (groups.any { it.gid.contentEquals(g) }) return g }
    return groups.firstOrNull { g -> g.members.any { it.key.hex() == node } }?.gid
}

/** Starts a search for `node`; an error to show, or null. */
fun AppModel.startSearch(node: ByteArray): String? {
    val gid = searchGroup(node.hex()) ?: return app.getString(R.string.search_needs_group)
    try {
        core.startSearch(gid, node, now())
    } catch (e: LichenException) {
        return e.display(app.resources)
    }
    searchTarget = node.hex()
    refreshSearches()
    pump()
    return null
}

fun AppModel.stopSearch(node: ByteArray) {
    runCatching { core.stopSearch(node) }
    refreshSearches()
}

private fun AppModel.searchCall(f: () -> Unit): String? = try {
    f()
    refreshSearches()
    pump()
    null
} catch (e: LichenException) {
    e.display(app.resources)
}

fun AppModel.ping(node: ByteArray): String? = searchCall { core.searchPing(node, now()) }

fun AppModel.addBearing(node: ByteArray, degrees: Double, yagi: Boolean): String? =
    searchCall { core.searchBearing(node, degrees, yagi, now()) }

fun AppModel.lowerPower(node: ByteArray, dbm: Byte): String? =
    searchCall { core.searchLowerPower(node, dbm, 10u, now()) }

fun AppModel.searchEstimate(node: ByteArray): SearchEstimate? = core.searchEstimate(node)

fun AppModel.measurements(node: ByteArray): List<SearchMeasurement> = core.searchMeasurements(node)

/** The search's measurements as a CSV file to share (field tests). */
fun AppModel.searchCsv(node: ByteArray): File? = runCatching {
    val dir = File(app.cacheDir, "search").also { it.mkdirs() }
    val safe = name(node).filter { it.isLetterOrDigit() }.ifEmpty { "target" }
    File(dir, "lichen-search-$safe.csv").also { it.writeText(core.searchCsv(node)) }
}.getOrNull()

fun AppModel.refreshSearches() {
    val t = now()
    val s = core.searches(t)
    if (s != searches) {
        searches = s
        updateLocator()
    }
    searchedBy = core.searchedBy(t)
}

fun AppModel.searchConsent(gid: ByteArray): Boolean = core.searchConsent(gid)

fun AppModel.setSearchConsent(gid: ByteArray, on: Boolean) {
    runCatching { core.setSearchConsent(gid, on, now()) }
    pump()
}

/** The phone's battery, for our beacons (at most once a minute). */
fun AppModel.reportBattery() {
    val t = wallNow()
    if (t - lastBatteryReport < 60_000) return
    lastBatteryReport = t
    val bm = app.getSystemService(Context.BATTERY_SERVICE) as? BatteryManager
    val level = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: -1
    core.setBattery(if (level in 0..100) level.toUByte() else null)
}

// ----- the estimate on the map -----

/** The heat grid as a bitmap (SOS red, transparent where nothing is likely). */
fun SearchEstimate.heatBitmap(): Bitmap? {
    val w = heatCols.toInt()
    val h = heatRows.toInt()
    if (w <= 0 || h <= 0 || heat.size != w * h) return null
    val px = IntArray(w * h) { i ->
        val a = ((heat[i].toInt() and 0xFF) / 255.0 * 0.85 * 255).roundToInt()
        (a shl 24) or (0xDC shl 16) or (0x26 shl 8) or 0x26
    }
    return Bitmap.createBitmap(px, w, h, Bitmap.Config.ARGB_8888)
}

/** Whether a point lies inside the 68 % ellipse. */
fun SearchEstimate.contains68(pLat: Double, pLon: Double): Boolean {
    val th = Math.toRadians(azimuthDeg)
    val east = (pLon - lon) * 111_320.0 * max(0.01, cos(Math.toRadians(lat)))
    val north = (pLat - lat) * 111_320.0
    val u = east * sin(th) + north * cos(th)
    val v = east * cos(th) - north * sin(th)
    val a = max(major68M, 1.0)
    val b = max(minor68M, 1.0)
    return (u / a) * (u / a) + (v / b) * (v / b) <= 1
}

/** The ellipse with semi-axes `major`, `minor` (metres) around the estimate, as (lat, lon). */
fun SearchEstimate.ellipse(major: Double, minor: Double): List<Pair<Double, Double>> {
    val th = Math.toRadians(azimuthDeg)
    val mPerLat = 111_320.0
    val mPerLon = 111_320.0 * max(0.01, cos(Math.toRadians(lat)))
    return (0..64).map { i ->
        val t = i / 64.0 * 2 * PI
        val x = major * cos(t)
        val y = minor * sin(t)
        val east = x * sin(th) + y * cos(th)
        val north = x * cos(th) - y * sin(th)
        (lat + north / mPerLat).coerceIn(-90.0, 90.0) to (lon + east / mPerLon)
    }
}

// ----- the compass -----

/** The phone's heading, degrees from true north (rotation-vector sensor), for search bearings. */
class Compass(context: Context, private val onHeading: (Double) -> Unit) : SensorEventListener {
    private val sensors = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
    private val rotation = sensors?.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR)
    private val matrix = FloatArray(9)
    private val angles = FloatArray(3)
    /** Magnetic declination where we are (true = magnetic + declination). */
    var declination = 0.0

    val available: Boolean get() = rotation != null

    fun start() {
        rotation?.let { sensors?.registerListener(this, it, SensorManager.SENSOR_DELAY_UI) }
    }

    fun stop() {
        sensors?.unregisterListener(this)
    }

    /** Declination at a position (once per fix is plenty). */
    fun at(lat: Double, lon: Double, altM: Double) {
        declination = GeomagneticField(lat.toFloat(), lon.toFloat(), altM.toFloat(), System.currentTimeMillis()).declination.toDouble()
    }

    override fun onSensorChanged(event: SensorEvent) {
        SensorManager.getRotationMatrixFromVector(matrix, event.values)
        SensorManager.getOrientation(matrix, angles)
        val magnetic = Math.toDegrees(angles[0].toDouble())
        onHeading(((magnetic + declination) % 360 + 360) % 360)
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
}
