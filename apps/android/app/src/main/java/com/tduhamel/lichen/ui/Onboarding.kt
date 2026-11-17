package com.tduhamel.lichen.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel

private enum class Step { WELCOME, PAIRING, IDENTITY, CONTACT }

/** First launch: welcome, pairing, identity, a first contact (mockups `accueil`, `appairage`,
 * `identite`, `contact`), as on iOS. */
@Composable
fun Onboarding(model: AppModel) {
    // Kept through rotation and activity recreation.
    var step by rememberSaveable { mutableStateOf(if (model.debugScreen == "pairing") Step.PAIRING else Step.WELCOME) }
    BackHandler(step != Step.WELCOME) {
        step = when (step) {
            Step.CONTACT -> Step.IDENTITY
            else -> Step.WELCOME
        }
    }
    when (step) {
        Step.WELCOME -> Welcome(model, pair = { step = Step.PAIRING }, later = { step = Step.IDENTITY })
        Step.PAIRING -> PairingScreen(model, back = { step = Step.WELCOME }, done = { step = Step.IDENTITY })
        Step.IDENTITY -> IdentityScreen(model, back = { step = Step.WELCOME }, next = { step = Step.CONTACT })
        Step.CONTACT -> ContactScreen(model, back = { step = Step.IDENTITY }, next = { model.markOnboarded(true) })
    }
}

@Composable
private fun Welcome(model: AppModel, pair: () -> Unit, later: () -> Unit) {
    val t = lichen
    var restoring by remember { mutableStateOf(false) }
    Column(
        Modifier
            .fillMaxSize()
            .background(t.bg)
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = 24.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.weight(0.6f))
        Box(
            Modifier.size(120.dp).background(t.s1, RoundedCornerShape(27.dp)).border(1.dp, t.line, RoundedCornerShape(27.dp)),
            contentAlignment = Alignment.Center,
        ) { ChirpMark(t.lora, Modifier.size(78.dp)) }
        Text("lichen", fontSize = 48.sp, fontWeight = FontWeight.Bold, color = t.tx, modifier = Modifier.padding(top = 20.dp))
        Text(stringResource(R.string.tagline), fontSize = 17.sp, color = t.tx2, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 12.dp))
        Spacer(Modifier.weight(1f))
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Point(stringResource(R.string.no_cell_towers), stringResource(R.string.no_cell_towers_note))
            Point(stringResource(R.string.end_to_end), stringResource(R.string.end_to_end_note))
            Point(stringResource(R.string.find_your_people), stringResource(R.string.find_your_people_note))
        }
        Spacer(Modifier.weight(1f))
        PrimaryButton(stringResource(R.string.connect_my_radio), onClick = pair)
        TextButton(onClick = later, modifier = Modifier.fillMaxWidth().padding(top = 4.dp)) {
            Text(stringResource(R.string.later), fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
        }
        // A phone that replaces another (ADR-0037): everything comes back from the file.
        TextButton(onClick = { restoring = true }, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.restore_backup), fontSize = 14.sp, color = t.tx2)
        }
    }
    if (restoring) RestoreBackupSheet(model, { restoring = false }) { model.markOnboarded(true) }
}

@Composable
private fun Point(title: String, detail: String) {
    val t = lichen
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Box(Modifier.padding(top = 7.dp).size(8.dp).background(t.lora, CircleShape))
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            Text(detail, fontSize = 14.sp, color = t.tx2)
        }
    }
}
