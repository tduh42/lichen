package com.tduhamel.lichen.ui

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The app's text in both languages, as the phone reads it: the typographic apostrophe ’,
 * never a straight one (B-and-16). */
@RunWith(AndroidJUnit4::class)
class StringsTest {
    /** Every string and plural form of the app (its own R: libraries' are not in it). */
    private fun texts(): Map<String, String> {
        val res = ApplicationProvider.getApplicationContext<Context>().resources
        val strings = R.string::class.java.fields.associate { it.name to res.getText(it.getInt(null)).toString() }
        val plurals = R.plurals::class.java.fields.flatMap { f ->
            listOf(0, 1, 2).map { n -> "${f.name}[$n]" to res.getQuantityText(f.getInt(null), n).toString() }
        }
        return strings + plurals
    }

    private fun straightApostrophes() = texts().filterValues { '\'' in it }.keys.sorted()

    /** Kilobytes as Android's own sizes write them in French, "ko", even next to one (B-and-18). */
    private fun capitalKo() = texts().filterValues { Regex("""[\dd] Ko\b""").containsMatchIn(it) }.keys.sorted() // "8 Ko", "%1$d Ko"

    @Test
    fun typographicApostrophe() = assertEquals(emptyList<String>(), straightApostrophes())

    @Test
    @Config(qualifiers = "+fr")
    fun typographicApostrophe_fr() = assertEquals(emptyList<String>(), straightApostrophes())

    @Test
    @Config(qualifiers = "+fr")
    fun kilobytesInLowerCase_fr() = assertEquals(emptyList<String>(), capitalKo())
}
