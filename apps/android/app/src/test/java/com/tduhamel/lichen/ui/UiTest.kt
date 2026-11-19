package com.tduhamel.lichen.ui

import android.content.Context
import android.content.Intent
import android.os.Looper
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import com.tduhamel.lichen.LichenApplication
import com.tduhamel.lichen.MainActivity
import com.tduhamel.lichen.model.AppModel
import org.junit.After
import org.junit.Rule
import org.robolectric.Shadows.shadowOf
import java.time.Duration

/**
 * A Compose UI test of the whole app on the JVM (Robolectric): [MainActivity] with the debug
 * extras of CLAUDE.md, the host build of the core through JNA, and the demo network in simulated
 * time. The clock moves only when the test calls [advance]; the demo network then runs in step,
 * on the main thread, so a five-minute scenario takes seconds and goes the same way every time.
 *
 * Language and theme come from Robolectric's qualifiers: `@Config(qualifiers = "fr")`,
 * `@Config(qualifiers = "night")`.
 */
abstract class UiTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null

    val context: Context get() = ApplicationProvider.getApplicationContext()
    val model: AppModel get() = (context as LichenApplication).model

    @After
    fun close() {
        scenario?.close()
        model.stopDemo()
    }

    /** Starts the app with these debug extras (`"demo" to true`, `"open" to "Camille"`…). */
    fun launch(vararg extras: Pair<String, Any>) {
        val intent = Intent(context, MainActivity::class.java)
        for ((k, v) in extras) {
            when (v) {
                is Boolean -> intent.putExtra(k, v)
                is Int -> intent.putExtra(k, v)
                is String -> intent.putExtra(k, v)
                else -> error("extra $k: ${v::class}")
            }
        }
        scenario = ActivityScenario.launch(intent)
        compose.waitForIdle()
    }

    /** Lets [seconds] of simulated time pass, in the demo's own 200 ms steps. */
    fun advance(seconds: Double) {
        val looper = shadowOf(Looper.getMainLooper())
        repeat((seconds * 5).toInt()) { looper.idleFor(Duration.ofMillis(200)) }
        compose.waitForIdle()
    }

    fun advance(seconds: Int) = advance(seconds.toDouble())

    fun str(@StringRes id: Int, vararg args: Any): String = context.getString(id, *args)

    fun plural(@PluralsRes id: Int, n: Int, vararg args: Any): String = context.resources.getQuantityString(id, n, *args)
}
