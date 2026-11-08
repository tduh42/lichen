package com.tduhamel.lichen

import android.content.Intent
import android.graphics.Color
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.LaunchedEffect
import androidx.core.content.IntentCompat
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.Notifications
import com.tduhamel.lichen.ui.LichenTheme
import com.tduhamel.lichen.ui.RootScreen
import com.tduhamel.lichen.ui.isDark

class MainActivity : ComponentActivity() {
    private val model: AppModel get() = (application as LichenApplication).model

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (BuildConfig.DEBUG) model.applyDebug(debugOptions())
        handle(intent)
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

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    override fun onStart() {
        super.onStart()
        model.inForeground = true
    }

    override fun onStop() {
        super.onStop()
        model.inForeground = false
    }

    /** A notification's conversation, or a radio plugged in over USB. */
    private fun handle(intent: Intent?) {
        intent?.getStringExtra(Notifications.EXTRA_CHAT)?.let(ChatId::fromRoute)?.let { model.openChat = it }
        if (intent?.action == UsbManager.ACTION_USB_DEVICE_ATTACHED) {
            IntentCompat.getParcelableExtra(intent, UsbManager.EXTRA_DEVICE, UsbDevice::class.java)?.let { model.connectUsb(it) }
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
