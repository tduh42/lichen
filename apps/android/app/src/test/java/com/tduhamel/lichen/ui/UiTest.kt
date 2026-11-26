package com.tduhamel.lichen.ui

import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.onLast
import android.content.Context
import android.graphics.Bitmap
import android.content.Intent
import android.os.Looper
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasAnySibling
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import com.tduhamel.lichen.LichenApplication
import com.tduhamel.lichen.MainActivity
import com.tduhamel.lichen.core.ContactInfo
import com.tduhamel.lichen.model.AppModel
import org.junit.After
import org.junit.Rule
import org.robolectric.Shadows.shadowOf
import java.io.File
import java.io.FileOutputStream
import java.time.Duration
import kotlin.math.abs

/**
 * A Compose UI test of the whole app on the JVM (Robolectric): [MainActivity] with the debug
 * extras of CLAUDE.md, the host build of the core through JNA, and the demo network in simulated
 * time. The clock moves only when the test calls [advance]; the demo network then runs in step,
 * on the main thread, so a five-minute scenario takes seconds and goes the same way every time.
 *
 * The screen is a phone's (robolectric.properties); a test adds its language or theme with
 * `@Config(qualifiers = "+fr")`, `"+night"`.
 */
abstract class UiTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null

    val context: Context get() = ApplicationProvider.getApplicationContext()
    val app: LichenApplication get() = context as LichenApplication
    val model: AppModel get() = app.model

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

    /** The soft keyboard up, [heightDp] tall, or down (0), as the system tells the window (its
     * IME insets): Robolectric shows none by itself. The composer then rises above it and the
     * screens see it up, as on a phone. */
    fun keyboard(heightDp: Int) {
        scenario!!.onActivity { a ->
            val px = (heightDp * a.resources.displayMetrics.density).toInt()
            val insets = androidx.core.view.WindowInsetsCompat.Builder()
                .setInsets(androidx.core.view.WindowInsetsCompat.Type.ime(), androidx.core.graphics.Insets.of(0, 0, 0, px))
                .setVisible(androidx.core.view.WindowInsetsCompat.Type.ime(), px > 0)
                .build()
            androidx.core.view.ViewCompat.dispatchApplyWindowInsets(a.window.decorView, insets)
        }
        compose.waitForIdle()
    }

    /** The process is killed and started again: the activity goes, a new model reads what
     * was saved (identity, database, preferences), and the app starts with these extras. */
    fun restart(vararg extras: Pair<String, Any>) {
        scenario?.close()
        scenario = null
        compose.waitForIdle()
        // The old process's demo network stops with it.
        app.model.stopDemo()
        app.model = AppModel(app)
        launch(*extras)
    }

    /** Another phone: this one's data gone (database, files, preferences, keys), the app
     * installed afresh and started with these extras. */
    fun newPhone(vararg extras: Pair<String, Any>) {
        scenario?.close()
        scenario = null
        compose.waitForIdle()
        app.model.stopDemo()
        java.io.File(context.filesDir, "lichen").deleteRecursively()
        for (p in listOf("lichen", "lichen.identity")) context.deleteSharedPreferences(p)
        FakeAndroidKeyStore.clear()
        app.model = AppModel(app)
        launch(*extras)
    }

    /** The app leaves the screen (another app, the home screen); it goes on behind. */
    fun leave() {
        scenario!!.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
    }

    /** Back on screen. */
    fun comeBack() {
        scenario!!.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        compose.waitForIdle()
    }

    /** Away for [seconds], then back. */
    fun awayFor(seconds: Int) {
        leave()
        advance(seconds)
        comeBack()
    }

    /** The app opened by an intent from outside (a notification's): its screen comes back with
     * it, the process (the model, the demo) the same. */
    fun launchWith(intent: Intent) {
        scenario?.close()
        scenario = ActivityScenario.launch(intent)
        compose.waitForIdle()
    }

    /** Answers the activity the app just started for a result (the system's file picker…). */
    fun answerNextActivity(data: Intent?, result: Int = android.app.Activity.RESULT_OK) {
        scenario!!.onActivity { a ->
            val shadow = shadowOf(a)
            val started = shadow.nextStartedActivityForResult ?: error("no activity started for a result")
            shadow.receiveResult(started.intent, result, data)
        }
        compose.waitForIdle()
    }

    /** The text the clipboard holds now. */
    fun clipboardText(): String? =
        context.getSystemService(android.content.ClipboardManager::class.java).primaryClip?.getItemAt(0)?.text?.toString()

    /** What the QR code in the node labelled [label] says, read off the screen as a camera
     * would: once the screen is idle, and captured again a few frames later if a capture can't
     * be read (F-ON7). */
    fun readQr(label: String): String {
        var failure: Throwable? = null
        var image: Bitmap? = null
        repeat(4) { attempt ->
            if (attempt > 0) compose.mainClock.advanceTimeBy(250)
            compose.waitForIdle()
            image = desc(label).captureToImage().asAndroidBitmap()
            runCatching { return readQr(image!!) }.onFailure { failure = it }
        }
        // What the last capture held, to tell a blank one from a cut or covered code; kept as a
        // file for a look.
        val last = image!!
        val px = IntArray(last.width * last.height)
        last.getPixels(px, 0, last.width, 0, 0, last.width, last.height)
        val dark = px.count { (it and 0xFF) < 128 } * 100 / maxOf(px.size, 1)
        val file = File("build/qr-failures").apply { mkdirs() }.let { File(it, "${label.hashCode()}-${System.currentTimeMillis()}.png") }
        FileOutputStream(file).use { last.compress(Bitmap.CompressFormat.PNG, 100, it) }
        throw AssertionError("no QR code read in \"$label\" after 4 captures: ${last.width}×${last.height}, $dark % dark, kept as $file", failure)
    }

    /** What a QR code in [image] says (zxing): with a white margin around it, as a camera sees
     * one on a page, and trying harder with both of zxing's binarizers. A code whose modules
     * fall between pixels can defeat a single plain read (F-ON7: a card is random each run). */
    fun readQr(image: Bitmap): String {
        val m = maxOf(image.width, image.height) / 8
        val w = image.width + 2 * m
        val h = image.height + 2 * m
        val px = IntArray(w * h) { 0xFFFFFFFF.toInt() }
        image.getPixels(px, m * w + m, w, 0, 0, image.width, image.height)
        val source = com.google.zxing.RGBLuminanceSource(w, h, px)
        val hints = mapOf(com.google.zxing.DecodeHintType.TRY_HARDER to true)
        var failure: Exception? = null
        for (binarizer in listOf<(com.google.zxing.LuminanceSource) -> com.google.zxing.Binarizer>(
            { com.google.zxing.common.HybridBinarizer(it) },
            { com.google.zxing.common.GlobalHistogramBinarizer(it) },
        )) {
            try {
                return com.google.zxing.qrcode.QRCodeReader().decode(com.google.zxing.BinaryBitmap(binarizer(source)), hints).text
            } catch (e: com.google.zxing.ReaderException) {
                failure = e
            }
        }
        throw failure ?: com.google.zxing.NotFoundException.getNotFoundInstance()
    }

    /** The system's back: closes the sheet or dialog on top, else goes back a screen. */
    fun back() {
        androidx.test.espresso.Espresso.pressBack()
        compose.waitForIdle()
    }

    /** Every text on screen, in every window. */
    fun shown(): List<String> =
        compose.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Text), useUnmergedTree = true)
            .fetchSemanticsNodes().flatMap { n -> n.config[SemanticsProperties.Text].map { it.text } }

    /** Lets [seconds] of simulated time pass, in the demo's own 200 ms steps. */
    fun advance(seconds: Double) {
        val looper = shadowOf(Looper.getMainLooper())
        repeat((seconds * 5).toInt()) { looper.idleFor(Duration.ofMillis(200)) }
        compose.waitForIdle()
    }

    fun advance(seconds: Int) = advance(seconds.toDouble())

    /** Advances simulated time until [done], at most [seconds]; fails with [what] otherwise, and
     * what [state] says then. */
    fun advanceUntil(what: String, seconds: Int = 120, state: () -> Any? = { null }, done: () -> Boolean) {
        val looper = shadowOf(Looper.getMainLooper())
        repeat(seconds * 5) {
            if (done()) {
                compose.waitForIdle()
                return
            }
            looper.idleFor(Duration.ofMillis(200))
        }
        compose.waitForIdle()
        check(done()) { "not within $seconds s: $what" + (state()?.let { " ($it)" } ?: "") }
    }

    /** Waits for [done] in real time (a link with threads of its own), the clocks moving on. */
    fun waitReal(what: String, ms: Long = 15_000, done: () -> Boolean) {
        val end = System.nanoTime() + ms * 1_000_000
        val looper = shadowOf(Looper.getMainLooper())
        while (System.nanoTime() < end) {
            looper.idleFor(Duration.ofMillis(50))
            if (done()) {
                compose.waitForIdle()
                return
            }
            Thread.sleep(20)
        }
        compose.waitForIdle()
        check(done()) { "not within $ms ms: $what" }
    }

    /** The demo contact of that name, once the radio's contact list is read. */
    fun contact(name: String): ContactInfo {
        advanceUntil("contact $name") { model.contacts.any { it.name == name } }
        return model.contacts.first { it.name == name }
    }

    fun str(@StringRes id: Int, vararg args: Any): String = context.getString(id, *args)

    fun plural(@PluralsRes id: Int, n: Int, vararg args: Any): String = context.resources.getQuantityString(id, n, *args)

    fun text(t: String, substring: Boolean = false): SemanticsNodeInteraction =
        compose.onNodeWithText(t, substring = substring, useUnmergedTree = true)

    fun text(@StringRes id: Int, vararg args: Any): SemanticsNodeInteraction = text(str(id, *args))

    fun desc(d: String): SemanticsNodeInteraction = compose.onNodeWithContentDescription(d, useUnmergedTree = true)

    fun desc(@StringRes id: Int): SemanticsNodeInteraction = desc(str(id))

    fun has(t: String, substring: Boolean = false): Boolean =
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    fun has(@StringRes id: Int, vararg args: Any): Boolean = has(str(id, *args))

    fun hasDesc(d: String): Boolean =
        compose.onAllNodesWithContentDescription(d, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    /** Taps the text, scrolling the first scrollable list to it first when needed. */
    fun tap(t: String, substring: Boolean = false) {
        if (!has(t, substring)) scrollTo(hasText(t, substring = substring))
        val node = compose.onAllNodesWithText(t, substring = substring).onFirst()
        // In view first: a field's keyboard can push a button out of a scrolling column.
        runCatching { node.performScrollTo() }
        node.performClick()
        compose.waitForIdle()
    }

    fun tap(@StringRes id: Int, vararg args: Any) = tap(str(id, *args))

    fun tapDesc(@StringRes id: Int) = tapDesc(str(id))

    fun tapDesc(d: String) {
        compose.onAllNodesWithContentDescription(d).onFirst().performClick()
        compose.waitForIdle()
    }

    /** The text is on screen: scrolled to first when a list holds it out of view. */
    fun see(t: String, substring: Boolean = false) {
        runCatching { scrollTo(hasText(t, substring = substring)) }
        compose.onAllNodesWithText(t, substring = substring, useUnmergedTree = true).onFirst().assertIsDisplayed()
    }

    fun see(@StringRes id: Int, vararg args: Any) = see(str(id, *args))

    /** Taps the button of that text in the dialog or the menu on screen (not the one behind). */
    fun tapInDialog(t: String) {
        compose.onAllNodes(hasText(t) and (hasAnyAncestor(isDialog()) or hasAnyAncestor(isPopup()))).onFirst().performClick()
        compose.waitForIdle()
    }

    fun tapInDialog(@StringRes id: Int, vararg args: Any) = tapInDialog(str(id, *args))

    /** Flips the switch of the row with that title: the switch nearest to it, in view. */
    fun toggle(@StringRes title: Int) {
        val t = str(title)
        if (!has(t)) scrollTo(hasText(t))
        val label = compose.onAllNodesWithText(t, useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot
        val switches = compose.onAllNodes(isToggleable(), useUnmergedTree = true)
        val nodes = switches.fetchSemanticsNodes()
        val i = nodes.indices.minBy { abs(nodes[it].boundsInRoot.center.y - label.center.y) }
        // In view first, when a list holds it.
        switches[i].also { runCatching { it.performScrollTo() } }.performClick()
        compose.waitForIdle()
    }

    /** Where a test failed: how many lists, and why each would not scroll. */
    private fun scrollFailure(matcher: SemanticsMatcher, why: List<String>): Nothing =
        error("no list holds ${matcher.description}: " + why.joinToString(" | "))

    /** How many lines the text takes on screen (a word cut in two shows as one line more). */
    fun lines(t: String): Int {
        val node = compose.onAllNodesWithText(t, useUnmergedTree = true).onFirst().fetchSemanticsNode()
        val layouts = mutableListOf<TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
        return layouts.first().lineCount
    }

    /** Wherever the text shows, whole: not wider than its room, no ellipsis; on one line when
     * [oneLine]. False when it does not show. */
    fun whole(t: String, oneLine: Boolean = false): Boolean {
        val nodes = compose.onAllNodesWithText(t, useUnmergedTree = true).fetchSemanticsNodes()
        return nodes.isNotEmpty() && nodes.all { n ->
            val layouts = mutableListOf<TextLayoutResult>()
            n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.invoke(layouts)
            // Every character laid out, no ellipsis; on one line, no wider than its room.
            // (Robolectric's `didOverflowWidth` is set for text well inside its room.)
            layouts.all { l ->
                val laidOut = l.getLineEnd(l.lineCount - 1) >= l.layoutInput.text.length
                val unclipped = l.layoutInput.softWrap || l.multiParagraph.intrinsics.maxIntrinsicWidth <= l.size.width + 1f
                laidOut && unclipped && (0 until l.lineCount).none { l.isLineEllipsized(it) } && (!oneLine || l.lineCount == 1)
            }
        }
    }

    /** Saves what each window shows (the screen, a dialog, a menu) as
     * `local/shots/robo-<name>-<n>.png` in the worktree: to look at a screen as the test saw it. */
    fun snap(name: String) {
        compose.waitForIdle()
        val dir = File("../../../local/shots").apply { mkdirs() }
        compose.onAllNodes(isRoot()).fetchSemanticsNodes().indices.forEach { i ->
            val image = compose.onAllNodes(isRoot())[i].captureToImage().asAndroidBitmap()
            FileOutputStream(File(dir, "robo-$name-$i.png")).use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }

    /** Scrolls the list that holds the node to it: the top-most window's lists first (a sheet,
     * a dialog), then the screen's. */
    fun scrollTo(matcher: SemanticsMatcher) {
        val lists = compose.onAllNodes(hasScrollAction())
        val n = lists.fetchSemanticsNodes().size
        val why = mutableListOf<String>()
        for (i in n - 1 downTo 0) {
            val r = runCatching { lists[i].performScrollToNode(matcher) }
            if (r.isSuccess) {
                compose.waitForIdle()
                return
            }
            why += "list $i: " + r.exceptionOrNull()?.message?.lineSequence()?.firstOrNull()
        }
        scrollFailure(matcher, why)
    }

    /** A node's login card's Log in, the last of the page's: the readings card's, above it, only
     * takes the page there (§16.7). */
    fun tapLogIn() {
        compose.onAllNodesWithText(str(com.tduhamel.lichen.R.string.log_in)).onLast().also { runCatching { it.performScrollTo() } }.performClick()
        compose.waitForIdle()
    }

    /** One of the four tabs, or of the Map's two modes (Positions, Locate): a tab of that
     * name, not a title that says the same. */
    fun tab(@StringRes id: Int) {
        compose.onAllNodes(hasText(str(id)) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).onFirst().performClick()
        compose.waitForIdle()
    }
}
