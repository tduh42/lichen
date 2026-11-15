package com.tduhamel.lichen

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
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
import com.tduhamel.lichen.model.isHex
import com.tduhamel.lichen.ui.LichenTheme
import com.tduhamel.lichen.ui.RootScreen
import com.tduhamel.lichen.ui.isDark

class MainActivity : ComponentActivity() {
    private val model: AppModel get() = (application as LichenApplication).model

    /** Notifications for messages and SOS: asked once, after the welcome screens. */
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

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
            // Notifications for messages and SOS: asked once, when the welcome screens are done
            // (not at every recreation, not before the user knows the app).
            LaunchedEffect(model.onboarded) {
                val prefs = getSharedPreferences("lichen", MODE_PRIVATE)
                if (model.onboarded && Build.VERSION.SDK_INT >= 33 && !prefs.getBoolean(ASKED_NOTIFICATIONS, false) &&
                    ContextCompat.checkSelfPermission(this@MainActivity, Manifest.permission.POST_NOTIFICATIONS) !=
                    PackageManager.PERMISSION_GRANTED
                ) {
                    prefs.edit().putBoolean(ASKED_NOTIFICATIONS, true).apply()
                    askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
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
        // An SOS notification opens its search.
        intent?.getStringExtra(Notifications.EXTRA_SOS)?.takeIf { it.isHex(32) }?.let { model.openSearch = it.lowercase() }
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
            sayPlain = e.getString("sayPlain"),
            tab = e.getString("tab"),
            screen = e.getString("screen"),
            tcp = e.getString("tcp"),
            media = e.getBoolean("media"),
            share = e.getString("share"),
            sosDemo = e.getBoolean("sosdemo"),
            sosDemo2 = e.getBoolean("sosdemo2"),
            fill = e.getInt("fill"),
            identityProblem = e.getString("identityProblem"),
            offline = e.getInt("offline"),
            online = e.getInt("online"),
            expiry = e.getInt("expiry"),
            inviteDemo = e.getBoolean("inviteDemo"),
            joinNow = e.getBoolean("joinNow"),
            // Screenshots and tests skip the welcome screens unless asked for them.
            onboarding = when {
                e.containsKey("onboarding") -> e.getBoolean("onboarding")
                e.containsKey("demo") || e.containsKey("tcp") -> false
                else -> null
            },
        )
    }

    private companion object {
        const val ASKED_NOTIFICATIONS = "lichen.askedNotifications"
    }
}
