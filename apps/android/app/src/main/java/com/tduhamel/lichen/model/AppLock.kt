package com.tduhamel.lichen.model

import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.edit

/**
 * The app lock (Settings › Privacy): fingerprint, face or the screen lock before lichen shows
 * anything, on a cold start and when it comes back after [delay] seconds in the background.
 * App only: the core, the radio link, the outbox and notifications go on behind the cover.
 * While the lock is on, the recents thumbnail is blank and message notifications say only
 * "New message"; SOS notifications keep their content. `MainActivity` runs the prompt.
 */
class AppLock(private val prefs: SharedPreferences) {
    /** What the prompt is for. */
    enum class Purpose { UNLOCK, ENABLE }

    /** The lock is on; turned on only after one successful authentication. */
    var enabled by mutableStateOf(prefs.getBoolean(ENABLED, false))
        private set

    /** Seconds in the background before lichen locks. */
    var delay by mutableIntStateOf(prefs.getInt(DELAY, 0))
        private set

    /** The cover is up until the owner authenticates. A cold start (a new process) begins locked. */
    var locked by mutableStateOf(enabled)
        private set

    /** A prompt of ours is up, and what for. */
    var pending by mutableStateOf<Purpose?>(null)
        private set

    /** The lock could not be turned on: the phone has no screen lock. */
    var problem by mutableStateOf(false)

    /** The prompt came up by itself for this lock (once). */
    var prompted = false
        private set

    private var backgroundSince: Long? = null

    fun chooseDelay(seconds: Int) {
        delay = seconds
        prefs.edit { putInt(DELAY, seconds) }
    }

    /** Off at once: lichen is open already. */
    fun disable() {
        store(false)
        locked = false
    }

    /** The activity left the screen (not for a configuration change, nor for our prompt). */
    fun background(nowMs: Long) {
        if (backgroundSince == null) backgroundSince = nowMs
        // Back on screen still locked, the prompt comes up again.
        if (locked) prompted = false
    }

    /** The activity is back: locks if it stayed away at least [delay]. */
    fun foreground(nowMs: Long) {
        val since = backgroundSince ?: return
        backgroundSince = null
        if (enabled && !locked && nowMs - since >= delay * 1_000L) {
            locked = true
            prompted = false
        }
    }

    /** The prompt should come up by itself now: once per lock. */
    fun shouldPrompt(): Boolean {
        if (!locked || prompted || pending != null) return false
        prompted = true
        return true
    }

    fun started(purpose: Purpose) {
        pending = purpose
    }

    /** The prompt's outcome. [again]: it was cancelled by the system (lichen left the screen),
     * so it comes up by itself on the way back. */
    fun finished(success: Boolean, again: Boolean = false) {
        val purpose = pending
        pending = null
        if (again) prompted = false
        if (!success) return
        when (purpose) {
            Purpose.UNLOCK -> locked = false
            Purpose.ENABLE -> {
                backgroundSince = null
                store(true)
            }
            null -> {}
        }
    }

    /** The phone has no screen lock any more: nothing can unlock lichen, and anyone holding the
     * phone could open it anyway. The lock goes rather than locking the owner out. */
    fun noScreenLock() {
        pending = null
        disable()
    }

    private fun store(on: Boolean) {
        enabled = on
        prefs.edit { putBoolean(ENABLED, on) }
    }

    companion object {
        private const val ENABLED = "lichen.lock"
        private const val DELAY = "lichen.lockAfter"

        /** What "Lock after" offers, in seconds: immediately, 1 minute, 15 minutes, 1 hour. */
        val DELAYS = listOf(0, 60, 900, 3_600)
    }
}
