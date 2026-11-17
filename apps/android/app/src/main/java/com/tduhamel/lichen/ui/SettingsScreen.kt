package com.tduhamel.lichen.ui

import android.app.LocaleManager
import android.os.Build
import android.os.LocaleList
import androidx.annotation.RequiresApi
import androidx.compose.foundation.background
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.SettingsInputAntenna
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.BuildConfig
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppLock
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.Appearance
import com.tduhamel.lichen.model.bandwidth

@Composable
fun LichenSwitch(checked: Boolean, onChange: (Boolean) -> Unit, enabled: Boolean = true) {
    val t = lichen
    Switch(
        checked, onChange, enabled = enabled,
        colors = SwitchDefaults.colors(
            checkedTrackColor = t.ok, checkedThumbColor = t.actFg,
            uncheckedTrackColor = t.s2, uncheckedThumbColor = t.tx2, uncheckedBorderColor = t.line,
        ),
    )
}

/** Settings (mockup `reglages`): the radio, transports, position, appearance, privacy, demo,
 * about. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    model: AppModel,
    pair: () -> Unit,
    identity: () -> Unit,
    radio: () -> Unit = {},
    licences: () -> Unit = {},
    report: () -> Unit = {},
) {
    val t = lichen
    var advertSent by remember { mutableStateOf(false) }
    var naming by remember { mutableStateOf(false) }
    var nameError by remember { mutableStateOf<String?>(null) }
    var importResult by remember { mutableStateOf<String?>(null) }
    // History from the official MeshCore app (ADR-0033): its exported database.
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) importResult = model.importMeshCore(uri)
    }
    importResult?.let {
        AlertDialog(
            onDismissRequest = { importResult = null },
            title = { Text(stringResource(R.string.import_title)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { importResult = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
    Scaffold(
        containerColor = t.bg,
        // The tab bar below takes the navigation bar.
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.tab_settings), fontSize = 22.sp) },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx),
            )
        },
    ) { padding ->
        LazyColumn(
            Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { RadioCardLarge(model, pair) }
            item { SectionLabel(stringResource(R.string.radio), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    val r = model.radio
                    // The radio's own settings (ADR-0032).
                    Box(Modifier.clickable(enabled = model.isConnected) { radio() }) {
                        CardRow(
                            stringResource(R.string.radio_settings),
                            if (r != null) {
                                "SF${r.sf} / ${bandwidth(r.bwHz)} · CR 4/${r.cr} · ${"%.3f".format(r.freqKhz.toDouble() / 1000)} MHz"
                            } else {
                                stringResource(R.string.read_from_radio)
                            },
                        ) { Text("›", color = lichen.tx2, fontSize = 20.sp) }
                    }
                    CardDivider()
                    val khz = r?.freqKhz?.toInt() ?: 869_618
                    val band = if (khz in 869_400..869_650) "869.4–869.65 MHz" else "%.3f MHz".format(khz / 1000.0)
                    CardRow(stringResource(R.string.sub_band), "$band · ${"%.0f".format(model.budget.limitMs.toDouble() / 36_000)} %")
                    CardDivider()
                    CardRow(stringResource(R.string.relay_for_others), stringResource(R.string.relay_for_others_note))
                    CardDivider()
                    Box(Modifier.clickable(enabled = model.isConnected) {
                        model.sendAdvert()
                        advertSent = true
                    }) {
                        CardRow(
                            stringResource(R.string.send_advert),
                            stringResource(if (advertSent) R.string.advert_sent else R.string.send_advert_note),
                        ) {
                            Icon(Icons.Outlined.SettingsInputAntenna, null, tint = if (model.isConnected) t.lora else t.tx2)
                        }
                    }
                    CardDivider()
                    // What MeshCore users and channels see (protocol.md §15.4).
                    Box(Modifier.clickable(enabled = model.isConnected) { naming = true }) {
                        CardRow(
                            stringResource(R.string.name_on_mesh),
                            model.nodeName.ifEmpty { stringResource(R.string.name_on_mesh_note) },
                        ) {
                            Icon(Icons.Outlined.Edit, null, tint = t.tx2)
                        }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.internet_transports), Modifier.padding(top = 10.dp)) }
            item {
                val irohLine = when {
                    !model.irohEnabled -> stringResource(R.string.off)
                    !model.bridgeOnline -> stringResource(R.string.iroh_on_demand)
                    model.relayOnly -> stringResource(R.string.online_relays)
                    else -> stringResource(R.string.fast_ip_visible)
                }
                val torLine = when (model.torStatus) {
                    AppModel.TorStatus.OFF -> stringResource(R.string.anonymous_slower)
                    AppModel.TorStatus.CONNECTING -> stringResource(R.string.connecting_tor)
                    AppModel.TorStatus.PUBLISHING -> stringResource(R.string.publishing_onion)
                    AppModel.TorStatus.READY -> stringResource(R.string.onion_reachable)
                    AppModel.TorStatus.FAILED -> stringResource(R.string.tor_failed)
                }
                CardList {
                    CardRow(stringResource(R.string.iroh_direct), irohLine) { LichenSwitch(model.irohEnabled, model::chooseIroh) }
                    CardDivider()
                    CardRow("Tor", torLine) { LichenSwitch(model.torEnabled, model::chooseTor) }
                    CardDivider()
                    CardRow(stringResource(R.string.relay_only), stringResource(R.string.relay_only_note)) {
                        LichenSwitch(model.relayOnly, model::chooseRelayOnly, enabled = model.irohEnabled)
                    }
                }
            }
            item {
                Text(stringResource(R.string.transports_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
            item { SectionLabel(stringResource(R.string.gifs), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.gif_auto), stringResource(R.string.gif_auto_note)) {
                        LichenSwitch(model.gifAutoLoad, model::chooseGifAutoLoad)
                    }
                }
            }
            item {
                Text(stringResource(R.string.gifs_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
            item { SectionLabel(stringResource(R.string.voice_messages), Modifier.padding(top = 10.dp)) }
            item {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    val options = listOf(0 to R.string.voice_compact, 2 to R.string.voice_standard, 3 to R.string.voice_clear)
                    options.forEachIndexed { i, (mode, label) ->
                        SegmentedButton(
                            selected = model.voiceMode == mode,
                            onClick = { model.chooseVoiceMode(mode) },
                            shape = SegmentedButtonDefaults.itemShape(i, options.size),
                            colors = segmentedColors(),
                        ) { Text(stringResource(label)) }
                    }
                }
            }
            item {
                Text(
                    stringResource(
                        when (model.voiceMode) {
                            0 -> R.string.voice_compact_note
                            3 -> R.string.voice_clear_note
                            else -> R.string.voice_standard_note
                        },
                    ),
                    fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp),
                )
            }
            item { SectionLabel(stringResource(R.string.position), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.send_threshold), stringResource(R.string.send_threshold_note)) {
                        Text("50 m", fontSize = 15.sp, color = t.tx2)
                    }
                    CardDivider()
                    CardRow(stringResource(R.string.heartbeat), stringResource(R.string.heartbeat_note)) {
                        Text("15 min", fontSize = 15.sp, color = t.tx2)
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.appearance), Modifier.padding(top = 10.dp)) }
            item {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    val options = listOf(Appearance.SYSTEM to R.string.system, Appearance.DARK to R.string.dark, Appearance.LIGHT to R.string.light)
                    options.forEachIndexed { i, (a, label) ->
                        SegmentedButton(
                            selected = model.appearance == a,
                            onClick = { model.chooseAppearance(a) },
                            shape = SegmentedButtonDefaults.itemShape(i, options.size),
                            colors = segmentedColors(),
                        ) { Text(stringResource(label)) }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.privacy), Modifier.padding(top = 10.dp)) }
            item { PrivacyCard(model) }
            item {
                Text(stringResource(R.string.lock_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
            if (Build.VERSION.SDK_INT >= 33) {
                item { SectionLabel(stringResource(R.string.language), Modifier.padding(top = 10.dp)) }
                item { LanguagePicker() }
            }
            item { SectionLabel(stringResource(R.string.history), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    Box(Modifier.clickable { importer.launch(arrayOf("*/*")) }) {
                        CardRow(stringResource(R.string.import_meshcore), stringResource(R.string.import_meshcore_note))
                    }
                }
            }
            // The encrypted backup (ADR-0037).
            if (model.canBackUp) {
                item { SectionLabel(stringResource(R.string.backup), Modifier.padding(top = 10.dp)) }
                item {
                    CardList {
                        BackupExportRow(model, stringResource(R.string.export_backup), stringResource(R.string.export_backup_note))
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.demo), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.demo_network), stringResource(R.string.demo_network_note)) {
                        LichenSwitch(model.isDemo, { if (it) model.startDemo() else model.stopDemo() })
                    }
                    CardDivider()
                    if (model.isDemo) {
                        // Off: messages wait for the radio, as when Bluetooth drops (§6.1).
                        CardRow(stringResource(R.string.demo_radio), stringResource(R.string.demo_radio_note)) {
                            LichenSwitch(!model.demoRadioOff, { model.setDemoRadio(it) })
                        }
                        CardDivider()
                    }
                    Box(Modifier.clickable { model.markOnboarded(false) }) {
                        CardRow(stringResource(R.string.show_welcome_again))
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.about), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.version)) { Text(BuildConfig.VERSION_NAME, color = t.tx2) }
                    CardDivider()
                    CardRow(stringResource(R.string.stickers), stringResource(R.string.stickers_credit))
                    CardDivider()
                    Row(Modifier.fillMaxWidth().clickable(onClick = identity), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) {
                            val f = model.fingerprint.take(2).joinToString(" ")
                            CardRow(
                                stringResource(R.string.your_identity),
                                stringResource(if (model.phraseBackedUp) R.string.key_backed_up else R.string.key_not_backed_up, f),
                            )
                        }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                    CardDivider()
                    Row(Modifier.fillMaxWidth().clickable(onClick = licences), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { CardRow(stringResource(R.string.licences), stringResource(R.string.licences_row)) }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                    CardDivider()
                    // ADR-0045: no message, key, name or position in it.
                    Row(Modifier.fillMaxWidth().clickable(onClick = report), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { CardRow(stringResource(R.string.report_problem), stringResource(R.string.report_row)) }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                }
            }
        }
    }
    if (naming) {
        var name by remember { mutableStateOf(model.nodeName) }
        AlertDialog(
            onDismissRequest = { naming = false },
            title = { Text(stringResource(R.string.name_on_mesh)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(R.string.name_on_mesh_explainer), fontSize = 14.sp, color = t.tx2)
                    OutlinedTextField(name, { name = it }, singleLine = true, colors = fieldColors())
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    naming = false
                    nameError = model.setMeshName(name.trim())
                }) { Text(stringResource(R.string.save), color = t.tx) }
            },
            dismissButton = { TextButton(onClick = { naming = false }) { Text(stringResource(R.string.cancel), color = t.tx2) } },
            containerColor = t.s1,
        )
    }
    nameError?.let {
        AlertDialog(
            onDismissRequest = { nameError = null },
            title = { Text(stringResource(R.string.not_changed)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { nameError = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** The app lock ([com.tduhamel.lichen.model.AppLock]): on after one successful authentication,
 * off by default. */
@Composable
private fun PrivacyCard(model: AppModel) {
    val t = lichen
    val lock = model.lock
    val prompt = LocalLockPrompt.current
    var choosing by remember { mutableStateOf(false) }
    CardList {
        CardRow(stringResource(R.string.lock_lichen), stringResource(R.string.lock_lichen_note)) {
            LichenSwitch(
                lock.enabled,
                { if (it) prompt(AppLock.Purpose.ENABLE) else lock.disable() },
                enabled = lock.pending == null,
            )
        }
        CardDivider()
        Box(Modifier.alpha(if (lock.enabled) 1f else 0.5f).clickable(enabled = lock.enabled) { choosing = true }) {
            CardRow(stringResource(R.string.lock_after)) {
                Box {
                    Text(lockDelayLabel(lock.delay), fontSize = 15.sp, color = t.tx2)
                    DropdownMenu(choosing, { choosing = false }, containerColor = t.s1) {
                        AppLock.DELAYS.forEach { s ->
                            DropdownMenuItem(
                                text = { Text(lockDelayLabel(s), color = t.tx) },
                                trailingIcon = { if (s == lock.delay) Icon(Icons.Outlined.Check, null, tint = t.tx) },
                                onClick = {
                                    choosing = false
                                    lock.chooseDelay(s)
                                },
                            )
                        }
                    }
                }
            }
        }
    }
    if (lock.problem) {
        AlertDialog(
            onDismissRequest = { lock.problem = false },
            title = { Text(stringResource(R.string.lock_stays_off)) },
            text = { Text(stringResource(R.string.no_screen_lock)) },
            confirmButton = { TextButton(onClick = { lock.problem = false }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun lockDelayLabel(seconds: Int): String = stringResource(
    when (seconds) {
        0 -> R.string.lock_immediately
        60 -> R.string.lock_1_minute
        900 -> R.string.lock_15_minutes
        else -> R.string.lock_1_hour
    },
)

/** The radio, its link and the airtime left; pairing when none is connected. */
@Composable
private fun RadioCardLarge(model: AppModel, pair: () -> Unit) {
    val t = lichen
    Column(
        Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (model.isConnected) {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(model.nodeName.ifEmpty { stringResource(R.string.radio) }, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                Text(
                    when {
                        model.isDemo -> stringResource(R.string.demo_network)
                        // SR-31: the network link to the radio is clear.
                        model.linkKind == "TCP" -> "TCP · ${stringResource(R.string.radio_connected_short)} · ${stringResource(R.string.tcp_not_encrypted)}"
                        else -> "${model.linkKind} · ${stringResource(R.string.radio_connected_short)}"
                    },
                    fontSize = 13.sp, color = t.tx2,
                )
            }
        } else {
            Row(
                Modifier.fillMaxWidth().clickable(onClick = pair),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Box(Modifier.size(40.dp).background(t.s2, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
                    Icon(Icons.Outlined.SettingsInputAntenna, null, tint = t.tx2)
                }
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.pair_a_radio), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.no_radio), fontSize = 13.sp, color = t.tx2)
                }
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2)
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(stringResource(R.string.airtime_left_short), fontSize = 13.sp, color = t.tx2)
            AirtimeMeter(model.budget.usedMs, model.budget.limitMs, large = true)
        }
        // Over TCP, also while it reconnects: stops trying.
        if ((model.isConnected || model.tcp.target != null) && !model.isDemo) {
            SecondaryButton(stringResource(R.string.disconnect), background = t.sosBg, foreground = t.sosFg) { model.disconnect() }
        }
    }
}

/** The app's language (Android 13+): the system's, English or French. */
@RequiresApi(33)
@Composable
private fun LanguagePicker() {
    val context = LocalContext.current
    val manager = remember { context.getSystemService(LocaleManager::class.java) }
    var current by remember { mutableStateOf(manager?.applicationLocales?.toLanguageTags().orEmpty()) }
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        val options = listOf("" to stringResource(R.string.system), "en" to "English", "fr" to "Français")
        options.forEachIndexed { i, (tag, label) ->
            SegmentedButton(
                selected = current.substringBefore('-') == tag,
                onClick = {
                    current = tag
                    manager?.applicationLocales = if (tag.isEmpty()) LocaleList.getEmptyLocaleList() else LocaleList.forLanguageTags(tag)
                },
                shape = SegmentedButtonDefaults.itemShape(i, options.size),
                colors = segmentedColors(),
            ) { Text(label) }
        }
    }
}
