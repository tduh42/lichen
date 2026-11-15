package com.tduhamel.lichen

import android.Manifest
import android.app.KeyguardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import android.graphics.Color
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Bundle
import android.os.SystemClock
import android.view.WindowManager
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.core.content.IntentCompat
import androidx.fragment.app.FragmentActivity
import com.tduhamel.lichen.model.AppLock
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.Notifications
import com.tduhamel.lichen.model.isHex
import com.tduhamel.lichen.ui.LichenTheme
import com.tduhamel.lichen.ui.LocalLockPrompt
import com.tduhamel.lichen.ui.LockCover
import com.tduhamel.lichen.ui.RootScreen
import com.tduhamel.lichen.ui.isDark

/** A `FragmentActivity`: androidx.biometric's prompt (the app lock) needs one. */
class MainActivity : FragmentActivity() {
    private val model: AppModel get() = (application as LichenApplication).model

    /** Notifications for messages and SOS: asked once, after the welcome screens. */
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    /** The app lock's system prompt. Made in `onCreate`, so a prompt that is up when the
     * activity is recreated reports to the new one. */
    private lateinit var biometric: BiometricPrompt
    private val lockPrompt: (AppLock.Purpose) -> Unit = { prompt(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (BuildConfig.DEBUG) model.applyDebug(debugOptions())
        handle(intent)
        biometric = BiometricPrompt(
            this, ContextCompat.getMainExecutor(this),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    model.lock.finished(true)
                }

                // A finger or face not recognised is not an error: the prompt stays up.
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    model.lock.finished(false, again = errorCode == BiometricPrompt.ERROR_CANCELED)
                }
            },
        )
        hideFromRecents(model.lock.enabled)
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
            // The app lock on: no recents thumbnail.
            LaunchedEffect(model.lock.enabled) { hideFromRecents(model.lock.enabled) }
            CompositionLocalProvider(LocalLockPrompt provides lockPrompt) {
                LichenTheme(model.appearance) {
                    Box(Modifier.fillMaxSize()) {
                        RootScreen(model)
                        if (model.lock.locked) LockCover(model)
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    override fun onStart() {
        super.onStart()
        model.inForeground = true
        model.lock.foreground(SystemClock.elapsedRealtime())
    }

    override fun onResume() {
        super.onResume()
        // Locked: the prompt comes up by itself, once.
        if (model.lock.shouldPrompt()) prompt(AppLock.Purpose.UNLOCK)
    }

    override fun onStop() {
        super.onStop()
        model.inForeground = false
        // Not for a rotation or a theme change, nor for our prompt (the screen lock's own screen
        // can stop the activity). Elapsed real time counts deep sleep too.
        if (!isChangingConfigurations && model.lock.pending == null) model.lock.background(SystemClock.elapsedRealtime())
    }

    /**
     * The app lock's system prompt: a strong biometric (fingerprint, face) or the screen lock.
     * Turning the lock on needs a method that works, and one success first.
     */
    private fun prompt(purpose: AppLock.Purpose) {
        val lock = model.lock
        if (lock.pending != null) return
        when (purpose) {
            AppLock.Purpose.UNLOCK -> if (getSystemService(KeyguardManager::class.java)?.isDeviceSecure == false) {
                lock.noScreenLock()
                return
            }
            AppLock.Purpose.ENABLE -> if (BiometricManager.from(this).canAuthenticate(AUTHENTICATORS) != BiometricManager.BIOMETRIC_SUCCESS) {
                lock.problem = true
                return
            }
        }
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(getString(if (purpose == AppLock.Purpose.UNLOCK) R.string.unlock_lichen else R.string.turn_on_app_lock))
            .setAllowedAuthenticators(AUTHENTICATORS)
            .build()
        lock.started(purpose)
        runCatching { biometric.authenticate(info) }.onFailure { lock.finished(false) }
    }

    /** No thumbnail in recents while the app lock is on (FLAG_SECURE before Android 13). */
    private fun hideFromRecents(on: Boolean) {
        if (Build.VERSION.SDK_INT >= 33) {
            setRecentsScreenshotEnabled(!on)
        } else if (on) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
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
            timer = e.getInt("timer"),
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
        const val AUTHENTICATORS = BIOMETRIC_STRONG or DEVICE_CREDENTIAL
    }
}
