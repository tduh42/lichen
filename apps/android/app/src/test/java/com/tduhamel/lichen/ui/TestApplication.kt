package com.tduhamel.lichen.ui

import android.os.SystemClock
import com.tduhamel.lichen.LichenApplication
import com.tduhamel.lichen.model.AppModel
import kotlinx.coroutines.Dispatchers

/**
 * The app on the JVM (Robolectric), set up before its model is built: an empty Android Keystore,
 * the host build of the core, the demo network on the main thread, a wall clock that follows the
 * simulated time. MapLibre's native library is not there, so no map.
 */
class TestApplication : LichenApplication() {
    override fun onCreate() {
        FakeAndroidKeyStore.install()
        AppModel.demoDispatcher = Dispatchers.Main.immediate
        // The wall clock moves with the simulated one (SystemClock, which the test advances).
        val wall0 = System.currentTimeMillis() - SystemClock.elapsedRealtime()
        AppModel.wallClock = { wall0 + SystemClock.elapsedRealtime() }
        // The same randomness in every run: the demo network is seeded too.
        AppModel.coreSeed = ByteArray(32) { 9 }
        super.onCreate()
    }

    override fun startMaps() = Unit
}
