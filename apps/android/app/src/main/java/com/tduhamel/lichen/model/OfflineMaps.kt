package com.tduhamel.lichen.model

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.tduhamel.lichen.R
import org.json.JSONObject
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.offline.OfflineManager
import org.maplibre.android.offline.OfflineRegion
import org.maplibre.android.offline.OfflineRegionError
import org.maplibre.android.offline.OfflineRegionStatus
import org.maplibre.android.offline.OfflineTilePyramidRegionDefinition
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

/** A map area kept for use without a network: what it covers, and whether it is complete. */
data class OfflineArea(
    val id: Long,
    val name: String,
    val style: String,
    /** Unix milliseconds, when known. */
    val created: Long?,
    val done: Long,
    val total: Long,
    val bytes: Long,
    val state: State,
    /** Why it failed. */
    val error: String? = null,
) {
    enum class State { COMPLETE, DOWNLOADING, PARTIAL, FAILED }
}

/** Map areas kept for use without a network, as on iOS, one by one: complete, partial, failed
 * or downloading, never just a count (audit M11). An area is the visible one, from two zoom
 * levels out to four in (16 at most). */
class OfflineMaps(private val context: Context) {
    var areas by mutableStateOf(listOf<OfflineArea>())
        private set

    /** An area could not even be created (no network, no storage). */
    var lastError by mutableStateOf<String?>(null)

    private val manager get() = OfflineManager.getInstance(context)
    private val regions = mutableMapOf<Long, OfflineRegion>()
    private val statuses = mutableMapOf<Long, OfflineRegionStatus>()
    private val errors = mutableMapOf<Long, String>()
    private val active = mutableSetOf<Long>()

    /** The download in progress, for the map's banner. */
    val progress: Pair<Long, Long>?
        get() = areas.firstOrNull { it.state == OfflineArea.State.DOWNLOADING }?.let { it.done to max(1L, it.total) }

    val bytes: Long get() = areas.sumOf { it.bytes }

    init {
        refresh()
    }

    fun download(bounds: LatLngBounds, zoom: Double, styleUrl: String, styleName: String) {
        if (progress != null) return
        val z = floor(zoom)
        val from = max(0.0, z - 2)
        val to = min(16.0, z + 4)
        val def = OfflineTilePyramidRegionDefinition(styleUrl, bounds, from, to, context.resources.displayMetrics.density)
        val c = bounds.center
        val name = "%.3f, %.3f · ".format(c.latitude, c.longitude) +
            context.getString(R.string.zoom_range, from.toInt(), to.toInt())
        val meta = JSONObject().put("name", name).put("style", styleName).put("created", System.currentTimeMillis())
        lastError = null
        manager.createOfflineRegion(def, meta.toString().toByteArray(), object : OfflineManager.CreateOfflineRegionCallback {
            override fun onCreate(offlineRegion: OfflineRegion) {
                watch(offlineRegion)
                start(offlineRegion)
                refresh()
            }

            override fun onError(error: String) {
                lastError = error
            }
        })
    }

    fun resume(area: OfflineArea) {
        errors.remove(area.id)
        regions[area.id]?.let { start(it) }
        publish()
    }

    fun pause(area: OfflineArea) {
        regions[area.id]?.setDownloadState(OfflineRegion.STATE_INACTIVE)
        active.remove(area.id)
        publish()
    }

    fun delete(area: OfflineArea) {
        val r = regions[area.id] ?: return
        r.setDownloadState(OfflineRegion.STATE_INACTIVE)
        r.delete(object : OfflineRegion.OfflineRegionDeleteCallback {
            override fun onDelete() {
                regions.remove(area.id)
                statuses.remove(area.id)
                errors.remove(area.id)
                active.remove(area.id)
                publish()
            }

            override fun onError(error: String) {
                lastError = error
            }
        })
    }

    private fun start(r: OfflineRegion) {
        active.add(r.id)
        r.setDownloadState(OfflineRegion.STATE_ACTIVE)
    }

    private fun watch(r: OfflineRegion) {
        regions[r.id] = r
        r.setObserver(object : OfflineRegion.OfflineRegionObserver {
            override fun onStatusChanged(status: OfflineRegionStatus) {
                statuses[r.id] = status
                if (status.isComplete) {
                    r.setDownloadState(OfflineRegion.STATE_INACTIVE)
                    active.remove(r.id)
                }
                publish()
            }

            override fun onError(error: OfflineRegionError) {
                errors[r.id] = error.message.ifEmpty { error.reason }
                r.setDownloadState(OfflineRegion.STATE_INACTIVE)
                active.remove(r.id)
                publish()
            }

            override fun mapboxTileCountLimitExceeded(limit: Long) {
                errors[r.id] = context.getString(R.string.offline_too_large)
                r.setDownloadState(OfflineRegion.STATE_INACTIVE)
                active.remove(r.id)
                publish()
            }
        })
    }

    private fun refresh() {
        manager.listOfflineRegions(object : OfflineManager.ListOfflineRegionsCallback {
            override fun onList(offlineRegions: Array<OfflineRegion>?) {
                for (r in offlineRegions.orEmpty()) {
                    if (r.id !in regions) watch(r)
                    r.getStatus(object : OfflineRegion.OfflineRegionStatusCallback {
                        override fun onStatus(status: OfflineRegionStatus?) {
                            status?.let { statuses[r.id] = it }
                            publish()
                        }

                        override fun onError(error: String?) = Unit
                    })
                }
                publish()
            }

            override fun onError(error: String) {
                lastError = error
            }
        })
    }

    private fun publish() {
        areas = regions.values.map { r ->
            val meta = runCatching { JSONObject(String(r.metadata)) }.getOrNull()
            val st = statuses[r.id]
            val err = errors[r.id]
            val state = when {
                err != null -> OfflineArea.State.FAILED
                st?.isComplete == true -> OfflineArea.State.COMPLETE
                r.id in active -> OfflineArea.State.DOWNLOADING
                else -> OfflineArea.State.PARTIAL
            }
            OfflineArea(
                id = r.id,
                name = meta?.optString("name")?.ifEmpty { null } ?: context.getString(R.string.map_area),
                style = meta?.optString("style").orEmpty(),
                created = meta?.optLong("created")?.takeIf { it > 0 },
                done = st?.completedResourceCount ?: 0,
                total = st?.requiredResourceCount ?: 0,
                bytes = st?.completedResourceSize ?: 0,
                state = state,
                error = err,
            )
        }.sortedByDescending { it.created ?: 0 }
    }
}
