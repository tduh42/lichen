package com.tduhamel.lichen.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import com.tduhamel.lichen.model.Appearance

/**
 * Design tokens (docs/design/tokens.json): dark and light. Orange means LoRa and the brand,
 * nothing else; primary actions use `actBg` / `actFg`.
 */
@Immutable
data class LichenColors(
    val bg: Color,
    val s1: Color,
    val s2: Color,
    val line: Color,
    val tx: Color,
    val tx2: Color,
    val actBg: Color,
    val actFg: Color,
    val lora: Color,
    val loraBg: Color,
    val loraFg: Color,
    val iroh: Color,
    val irohBg: Color,
    val irohFg: Color,
    val tor: Color,
    val torBg: Color,
    val torFg: Color,
    val sos: Color,
    val sosBg: Color,
    val sosFg: Color,
    val sosBtn: Color,
    val ok: Color,
    val okBg: Color,
    val okFg: Color,
    val warn: Color,
    val warnBg: Color,
    val warnFg: Color,
    val dark: Boolean,
) {
    /** Avatar colours, picked from the key so a contact keeps its colour. */
    fun avatar(key: ByteArray): Pair<Color, Color> {
        val palette = listOf(loraBg to loraFg, irohBg to irohFg, torBg to torFg, okBg to okFg, warnBg to warnFg, s2 to tx)
        return palette[(key.firstOrNull()?.toInt() ?: 0).and(0xFF) % palette.size]
    }

    /** A solid colour for a key (map markers). */
    fun marker(key: ByteArray): Color {
        val solid = listOf(lora, iroh, tor, ok, warn, tx2)
        return solid[(key.firstOrNull()?.toInt() ?: 0).and(0xFF) % solid.size]
    }
}

private fun c(hex: Long) = Color(0xFF000000 or hex)

val DarkColors = LichenColors(
    bg = c(0x0E0E0D), s1 = c(0x1A1A18), s2 = c(0x262623), line = c(0x34342F),
    tx = c(0xF4F4F0), tx2 = c(0xABABA3), actBg = c(0xF4F4F0), actFg = c(0x141413),
    lora = c(0xFB923C), loraBg = c(0x3B2414), loraFg = c(0xFDBA74),
    iroh = c(0x60A5FA), irohBg = c(0x16264A), irohFg = c(0x93C5FD),
    tor = c(0xA78BFA), torBg = c(0x2A1B4D), torFg = c(0xC4B5FD),
    sos = c(0xEF4444), sosBg = c(0x3F1414), sosFg = c(0xFCA5A5), sosBtn = c(0xDC2626),
    ok = c(0x4ADE80), okBg = c(0x12301C), okFg = c(0x86EFAC),
    warn = c(0xFACC15), warnBg = c(0x332B0A), warnFg = c(0xFDE047),
    dark = true,
)

val LightColors = LichenColors(
    bg = c(0xF7F7F5), s1 = c(0xFFFFFF), s2 = c(0xEFEFEB), line = c(0xE2E2DC),
    tx = c(0x141413), tx2 = c(0x55554F), actBg = c(0x141413), actFg = c(0xFFFFFF),
    lora = c(0xC2410C), loraBg = c(0xFFEDD5), loraFg = c(0x9A3412),
    iroh = c(0x1D4ED8), irohBg = c(0xDBEAFE), irohFg = c(0x1E40AF),
    tor = c(0x6D28D9), torBg = c(0xEDE9FE), torFg = c(0x5B21B6),
    sos = c(0xB91C1C), sosBg = c(0xFEE2E2), sosFg = c(0x991B1B), sosBtn = c(0xB91C1C),
    ok = c(0x15803D), okBg = c(0xDCFCE7), okFg = c(0x166534),
    warn = c(0xA16207), warnBg = c(0xFEF9C3), warnFg = c(0x854D0E),
    dark = false,
)

val LocalLichen = staticCompositionLocalOf { DarkColors }

/** The current tokens. */
val lichen: LichenColors @Composable get() = LocalLichen.current

@Composable
fun isDark(appearance: Appearance): Boolean = when (appearance) {
    Appearance.SYSTEM -> isSystemInDarkTheme()
    Appearance.DARK -> true
    Appearance.LIGHT -> false
}

/** The tokens, also mapped onto Material 3's roles so its components follow the design. */
@Composable
fun LichenTheme(appearance: Appearance, content: @Composable () -> Unit) {
    val t = if (isDark(appearance)) DarkColors else LightColors
    val base = if (t.dark) darkColorScheme() else lightColorScheme()
    val scheme = base.copy(
        primary = t.actBg, onPrimary = t.actFg,
        secondary = t.lora, onSecondary = t.actFg,
        background = t.bg, onBackground = t.tx,
        surface = t.bg, onSurface = t.tx,
        surfaceVariant = t.s2, onSurfaceVariant = t.tx2,
        surfaceContainerLowest = t.bg, surfaceContainerLow = t.s1, surfaceContainer = t.s1,
        surfaceContainerHigh = t.s1, surfaceContainerHighest = t.s2,
        secondaryContainer = t.s2, onSecondaryContainer = t.tx,
        outline = t.line, outlineVariant = t.line,
        error = t.sos, onError = t.actFg, errorContainer = t.sosBg, onErrorContainer = t.sosFg,
    )
    CompositionLocalProvider(LocalLichen provides t) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}

/** Big figures (budget, fingerprints, safety numbers): bold, tabular. */
fun figure(size: TextUnit) = TextStyle(fontSize = size, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
