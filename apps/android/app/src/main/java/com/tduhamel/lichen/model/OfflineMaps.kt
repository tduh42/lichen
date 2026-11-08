package com.tduhamel.lichen.model

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.offline.OfflineManager
import org.maplibre.android.offline.OfflineRegion
import org.maplibre.android.offline.OfflineRegionError
import org.maplibre.android.offline.OfflineRegionStatus
import org.maplibre.android.offline.OfflineTilePyramidRegionDefinition
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

/** Map areas kept for use without a network, as on iOS: the visible area, from two zoom
 * levels out to four in (16 at most). */
class OfflineMaps(private val context: Context) {
    /** Resources done and to do while downloading. */
    var progress by mutableStateOf<Pair<Long, Long>?>(null)
        private set
    var areas by mutableStateOf(0)
        private set
    var bytes by mutableStateOf(0L)
        private set

    private val manager get() = OfflineManager.getInstance(context)

    init {
        refresh()
    }

    fun download(bounds: LatLngBounds, zoom: Double, styleUrl: String) {
        if (progress != null) return
        val z = floor(zoom)
        val def = OfflineTilePyramidRegionDefinition(
            styleUrl, bounds, max(0.0, z - 2), min(16.0, z + 4), context.resources.displayMetrics.density,
        )
        progress = 0L to 1L
        manager.createOfflineRegion(def, ByteArray(0), object : OfflineManager.CreateOfflineRegionCallback {
            override fun onCreate(offlineRegion: OfflineRegion) {
                offlineRegion.setObserver(object : OfflineRegion.OfflineRegionObserver {
                    override fun onStatusChanged(status: OfflineRegionStatus) {
                        progress = status.completedResourceCount to max(1L, status.requiredResourceCount)
                        if (status.isComplete) {
                            offlineRegion.setDownloadState(OfflineRegion.STATE_INACTIVE)
                            progress = null
                            refresh()
                        }
                    }

                    override fun onError(error: OfflineRegionError) = Unit

                    override fun mapboxTileCountLimitExceeded(limit: Long) {
                        offlineRegion.setDownloadState(OfflineRegion.STATE_INACTIVE)
                        progress = null
                        refresh()
                    }
                })
                offlineRegion.setDownloadState(OfflineRegion.STATE_ACTIVE)
            }

            override fun onError(error: String) {
                progress = null
            }
        })
    }

    fun deleteAll() {
        manager.listOfflineRegions(object : OfflineManager.ListOfflineRegionsCallback {
            override fun onList(offlineRegions: Array<OfflineRegion>?) {
                val regions = offlineRegions.orEmpty()
                var left = regions.size
                if (left == 0) refresh()
                for (r in regions) {
                    r.delete(object : OfflineRegion.OfflineRegionDeleteCallback {
                        override fun onDelete() {
                            if (--left == 0) refresh()
                        }

                        override fun onError(error: String) {
                            if (--left == 0) refresh()
                        }
                    })
                }
            }

            override fun onError(error: String) = Unit
        })
    }

    private fun refresh() {
        manager.listOfflineRegions(object : OfflineManager.ListOfflineRegionsCallback {
            override fun onList(offlineRegions: Array<OfflineRegion>?) {
                val regions = offlineRegions.orEmpty()
                areas = regions.size
                bytes = 0L
                for (r in regions) {
                    r.getStatus(object : OfflineRegion.OfflineRegionStatusCallback {
                        override fun onStatus(status: OfflineRegionStatus?) {
                            bytes += status?.completedResourceSize ?: 0L
                        }

                        override fun onError(error: String?) = Unit
                    })
                }
            }

            override fun onError(error: String) = Unit
        })
    }
}
