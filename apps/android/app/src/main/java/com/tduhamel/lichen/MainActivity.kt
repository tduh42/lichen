package com.tduhamel.lichen

import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.LaunchedEffect
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.ui.LichenTheme
import com.tduhamel.lichen.ui.RootScreen
import com.tduhamel.lichen.ui.isDark

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val model = (application as LichenApplication).model
        if (BuildConfig.DEBUG) model.applyDebug(debugOptions())
        setContent {
            val dark = isDark(model.appearance)
            LaunchedEffect(dark) {
                val bars = if (dark) {
                    SystemBarStyle.dark(Color.TRANSPARENT)
                } else {
                    SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
                }
                enableEdgeToEdge(bars, bars)
            }
            LichenTheme(model.appearance) { RootScreen(model) }
        }
    }

    /**
     * Debug extras for screenshots, as the iOS launch arguments:
     * `adb shell am start -n com.tduhamel.lichen/.MainActivity --ez demo true --es open Camille`.
     */
    private fun debugOptions(): AppModel.DebugOptions {
        val e = intent?.extras ?: return AppModel.DebugOptions()
        return AppModel.DebugOptions(
            demo = e.getBoolean("demo"),
            open = e.getString("open"),
            say = e.getString("say"),
            tab = e.getString("tab"),
            screen = e.getString("screen"),
        )
    }
}
