package com.tduhamel.lichen

import android.app.Application
import androidx.annotation.VisibleForTesting
import org.maplibre.android.MapLibre
import com.tduhamel.lichen.model.AppModel

/** Holds the app model for the whole process: the core outlives activities. */
open class LichenApplication : Application() {
    lateinit var model: AppModel
        // The UI tests build a new one, as when the process is started again.
        @VisibleForTesting internal set

    override fun onCreate() {
        super.onCreate()
        startMaps()
        model = AppModel(this)
    }

    /** MapLibre and its native library. The JVM UI tests (Robolectric) have no such library:
     * their application skips it, and draws no map. */
    protected open fun startMaps() {
        MapLibre.getInstance(this)
        mapsReady = true
    }

    companion object {
        /** MapLibre is set up, so maps can be drawn and kept offline: always on a phone, never in
         * the JVM UI tests (an empty map there, no offline areas). */
        var mapsReady = false
            private set
    }
}
