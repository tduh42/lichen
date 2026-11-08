package com.tduhamel.lichen

import android.app.Application
import org.maplibre.android.MapLibre
import com.tduhamel.lichen.model.AppModel

/** Holds the app model for the whole process: the core outlives activities. */
class LichenApplication : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        MapLibre.getInstance(this)
        model = AppModel(this)
    }
}
