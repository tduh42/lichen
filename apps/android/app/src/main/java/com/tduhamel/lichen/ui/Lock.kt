package com.tduhamel.lichen.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.view.WindowCompat
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppLock
import com.tduhamel.lichen.model.AppModel

/** Brings up the app lock's system prompt (`MainActivity`). */
val LocalLockPrompt = staticCompositionLocalOf<(AppLock.Purpose) -> Unit> { {} }

/**
 * The app lock's cover (Settings › Privacy): the mark, an Unlock button, and a line when an SOS
 * came in meanwhile (its alert shows once unlocked). A full-screen dialog, so it sits above the
 * app's other dialogs; the same content in the app's window covers it until the dialog is up.
 * Back leaves lichen, still locked.
 */
@Composable
fun LockCover(model: AppModel) {
    val prompt = LocalLockPrompt.current
    val context = LocalContext.current
    // Under the dialog: no frame of the app before it is up, and no touch through.
    LockContent(model, prompt, Modifier.pointerInput(Unit) { awaitPointerEventScope { while (true) awaitPointerEvent() } })
    Dialog(
        onDismissRequest = { context.findActivity()?.moveTaskToBack(true) },
        properties = DialogProperties(
            dismissOnClickOutside = false,
            usePlatformDefaultWidth = false,
            decorFitsSystemWindows = false,
        ),
    ) {
        // The dialog's own window draws behind the system bars: their icons follow the theme.
        val view = LocalView.current
        val dark = lichen.dark
        SideEffect {
            (view.parent as? DialogWindowProvider)?.window?.let { w ->
                WindowCompat.getInsetsController(w, view).run {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
            }
        }
        LockContent(model, prompt)
    }
}

@Composable
private fun LockContent(model: AppModel, prompt: (AppLock.Purpose) -> Unit, modifier: Modifier = Modifier) {
    val t = lichen
    Column(
        modifier
            .fillMaxSize()
            .background(t.bg)
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = 24.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.weight(1f))
        Box(
            Modifier.size(120.dp).background(t.s1, RoundedCornerShape(27.dp)).border(1.dp, t.line, RoundedCornerShape(27.dp)),
            contentAlignment = Alignment.Center,
        ) { ChirpMark(t.lora, Modifier.size(78.dp)) }
        Text("lichen", fontSize = 48.sp, fontWeight = FontWeight.Bold, color = t.tx, modifier = Modifier.padding(top = 20.dp))
        Spacer(Modifier.weight(1f))
        if (model.sosAlert != null) {
            Row(
                Modifier.padding(bottom = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(Icons.Outlined.WarningAmber, null, tint = t.sos)
                Text(
                    stringResource(R.string.sos_while_locked),
                    fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.sos, textAlign = TextAlign.Center,
                )
            }
        }
        PrimaryButton(stringResource(R.string.unlock), enabled = model.lock.pending == null) { prompt(AppLock.Purpose.UNLOCK) }
    }
}

/** The activity behind a composable's context (a dialog's included). */
tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
