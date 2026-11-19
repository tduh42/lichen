package com.tduhamel.lichen

import android.app.Application
import org.maplibre.android.MapLibre
import com.tduhamel.lichen.model.AppModel

/** Holds the app model for the whole process: the core outlives activities. */
open class LichenApplication : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        startMaps()
        model = AppModel(this)
    }

    /** MapLibre and its native library. The JVM UI tests (Robolectric) have no such library:
     * their application skips it, and draws no map. */
    protected open fun startMaps() {
        MapLibre.getInstance(this)
    }
}
