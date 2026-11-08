package com.tduhamel.lichen.model

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Looper
import androidx.core.content.ContextCompat
import com.tduhamel.lichen.core.LocationFix

/**
 * The phone's location, as the iOS app's `Locator`: on while something needs our position (a
 * share, SOS, the map or the search on screen). Android's own location service, no Play
 * services; in the background, the radio service keeps it alive while sharing.
 */
class Locator(private val context: Context, private val onFix: (LocationFix) -> Unit) {
    private val manager = context.getSystemService(LocationManager::class.java)
    private var running = false

    private val listener = LocationListener { onFix(it.fix()) }

    fun permitted(): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

    /** Starts the updates (every second, every metre: the core decides what is worth sending). */
    @SuppressLint("MissingPermission")
    fun start() {
        val m = manager ?: return
        if (running || !permitted()) return
        val provider = when {
            m.hasProvider(LocationManager.FUSED_PROVIDER) -> LocationManager.FUSED_PROVIDER
            m.hasProvider(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
            else -> return
        }
        m.getLastKnownLocation(provider)?.let { onFix(it.fix()) }
        m.requestLocationUpdates(provider, 1_000L, 1f, listener, Looper.getMainLooper())
        running = true
    }

    fun stop() {
        if (!running) return
        manager?.removeUpdates(listener)
        running = false
    }
}

private fun Location.fix() = LocationFix(
    lat = latitude,
    lon = longitude,
    altM = if (hasAltitude() && hasVerticalAccuracy()) altitude else null,
    accuracyM = if (hasAccuracy()) maxOf(accuracy.toDouble(), 1.0) else 50.0,
    headingDeg = if (hasBearing()) bearing.toDouble() else null,
    speedMps = if (hasSpeed()) speed.toDouble() else null,
    unixS = (time / 1000).toULong(),
)
