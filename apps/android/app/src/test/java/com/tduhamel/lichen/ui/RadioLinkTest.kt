package com.tduhamel.lichen.ui

import android.content.res.Configuration
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.tduhamel.lichen.R
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Area 2 of docs/testing.md: the radio link and the radio's settings. */
@RunWith(AndroidJUnit4::class)
class RadioLinkTest : UiTest() {
    /** RL-1: the demo radio connects; its contacts and channels arrive. */
    @Test
    fun RL_1_demoRadioConnects() = demoRadioConnects()

    @Test
    @Config(qualifiers = "fr")
    fun RL_1_demoRadioConnects_fr() {
        assertEquals("Nouveau message", str(R.string.new_message))
        demoRadioConnects()
    }

    @Test
    @Config(qualifiers = "fr-night")
    fun RL_1_demoRadioConnects_frDark() {
        val night = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        assertEquals(Configuration.UI_MODE_NIGHT_YES, night)
        demoRadioConnects()
    }

    private fun demoRadioConnects() {
        launch("demo" to true)
        compose.onNodeWithText(str(R.string.radio_connected, "ThinkNode M5")).assertIsDisplayed()
        // The radio's channel in Messages, once the radio's channels are read (about 3 s), then
        // what Lucie posts on it at 12 s.
        advance(5)
        compose.onNodeWithText("Public").assertIsDisplayed()
        advance(10)
        compose.onNodeWithText("Fog on the Pas de la Ville", substring = true).assertIsDisplayed()
        // Its contacts: the people one can write to.
        compose.onNodeWithContentDescription(str(R.string.new_message)).performClick()
        compose.onNodeWithText("Camille").assertIsDisplayed()
        compose.onNodeWithText("Julien").assertIsDisplayed()
    }
}
