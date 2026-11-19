package com.tduhamel.lichen.ui

import com.sun.jna.NativeLibrary
import com.tduhamel.lichen.LichenApplication
import com.tduhamel.lichen.model.AppModel
import kotlinx.coroutines.Dispatchers

/**
 * The app on the JVM (Robolectric), set up before its model is built: an empty Android Keystore,
 * the host build of the core, the demo network on the main thread. MapLibre's native library is
 * not there, so no map.
 */
class TestApplication : LichenApplication() {
    override fun onCreate() {
        FakeAndroidKeyStore.install()
        AppModel.demoDispatcher = Dispatchers.Main.immediate
        super.onCreate()
    }

    override fun startMaps() = Unit
}
