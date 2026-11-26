package com.tduhamel.lichen.ui

import androidx.compose.material.icons.outlined.Router
import androidx.compose.material.icons.outlined.Timer
import androidx.compose.material.icons.outlined.SyncAlt
import androidx.compose.material.icons.outlined.Campaign
import androidx.compose.material.icons.outlined.AlternateEmail
import androidx.compose.material.icons.outlined.Insights
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.VpnLock
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.Gif
import androidx.compose.material.icons.outlined.Straighten
import androidx.compose.material.icons.outlined.MonitorHeart
import androidx.compose.material.icons.outlined.SdStorage
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material.icons.outlined.Backup
import androidx.compose.material.icons.outlined.Science
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material.icons.outlined.WavingHand
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.material.icons.outlined.Fingerprint
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.BugReport
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Block
import android.os.SystemClock
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.LaunchedEffect
import kotlinx.coroutines.delay
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
fun LichenSwitch(
    checked: Boolean,
    onChange: (Boolean) -> Unit,
    enabled: Boolean = true,
    /** What it turns on, for screen readers: by default the title of its row ([CardRow]). */
    label: String? = null,
) {
    val t = lichen
    val said = label ?: LocalRowTitle.current
    Switch(
        checked, onChange, enabled = enabled,
        modifier = if (said != null) Modifier.semantics { contentDescription = said } else Modifier,
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
    radios: () -> Unit = {},
    blocked: () -> Unit = {},
    storage: () -> Unit = {},
) {
    val t = lichen
    var choosingAdvert by remember { mutableStateOf(false) }
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
            item { RadioCard(model, pair) }
            item { SectionLabel(stringResource(R.string.radio), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    val r = model.radio
                    // The radio's own settings (ADR-0032).
                    Box(Modifier.clickable(enabled = model.isConnected) { radio() }) {
                        CardRow(
                            stringResource(R.string.radio_settings),
                            if (r != null) {
                                "SF${r.sf} / ${bandwidth(r.bwHz)} · CR\u00A04/${r.cr} · ${"%.3f".format(r.freqKhz.toDouble() / 1000)}\u00A0MHz"
                            } else {
                                stringResource(R.string.read_from_radio)
                            },
                            merge = false,
                            tile = { IconTile(Icons.Outlined.SettingsInputAntenna, t.loraBg, t.loraFg) },
                        ) { Text("›", color = lichen.tx2, fontSize = 20.sp) }
                    }
                    CardDivider(TileInset)
                    // Radios to switch between (ADR-0042).
                    Box(Modifier.clickable { radios() }) {
                        CardRow(stringResource(R.string.radios), pluralStringResource(R.plurals.radios_saved, model.savedRadios.size, model.savedRadios.size), merge = false, tile = { IconTile(Icons.Outlined.Router) }) {
                            Text("›", color = lichen.tx2, fontSize = 20.sp)
                        }
                    }
                    CardDivider(TileInset)
                    val khz = r?.freqKhz?.toInt() ?: 869_618
                    // In the phone's language, as the frequency above it ("869,4–869,65 MHz" in French).
                    val band = if (khz in 869_400..869_650) "%.1f–%.2f MHz".format(869.4, 869.65) else "%.3f MHz".format(khz / 1000.0)
                    CardRow(stringResource(R.string.sub_band), "$band · ${"%.0f".format(model.budget.limitMs.toDouble() / 36_000)} %", tile = { IconTile(Icons.Outlined.Timer) })
                    CardDivider(TileInset)
                    CardRow(stringResource(R.string.relay_for_others), stringResource(R.string.relay_for_others_note), tile = { IconTile(Icons.Outlined.SyncAlt) })
                    CardDivider(TileInset)
                    // Our advert (B-user-4): nearby or the whole mesh, chosen; then what the radio
                    // answered (without a radio: not sent).
                    Box(Modifier.clickable { choosingAdvert = true }) {
                        CardRow(
                            stringResource(R.string.send_advert),
                            advertStatus(model.advert),
                            merge = false,
                            tile = { IconTile(Icons.Outlined.Campaign) },
                        ) {
                            Icon(Icons.Outlined.SettingsInputAntenna, null, tint = if (model.isConnected) t.lora else t.tx2)
                        }
                    }
                    CardDivider(TileInset)
                    // What MeshCore users and channels see (protocol.md §15.4).
                    Box(Modifier.clickable(enabled = model.isConnected) { naming = true }) {
                        CardRow(
                            stringResource(R.string.name_on_mesh),
                            model.nodeName.ifEmpty { stringResource(R.string.name_on_mesh_note) },
                            merge = false,
                            tile = { IconTile(Icons.Outlined.AlternateEmail) },
                        ) {
                            Icon(Icons.Outlined.Edit, null, tint = t.tx2)
                        }
                    }
                    CardDivider(TileInset)
                    // Off: fragments, airtime and signal wait behind long press › Info.
                    CardRow(stringResource(R.string.radio_details), stringResource(R.string.radio_details_note), tile = { IconTile(Icons.Outlined.Insights) }) {
                        LichenSwitch(model.radioDetails, model::chooseRadioDetails)
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
                    CardRow(stringResource(R.string.iroh_direct), irohLine, tile = { IconTile(Icons.Outlined.Bolt, t.irohBg, t.irohFg) }) { LichenSwitch(model.irohEnabled, model::chooseIroh) }
                    CardDivider(TileInset)
                    CardRow("Tor", torLine, tile = { IconTile(Icons.Outlined.VpnLock, t.torBg, t.torFg) }) { LichenSwitch(model.torEnabled, model::chooseTor) }
                    CardDivider(TileInset)
                    CardRow(stringResource(R.string.relay_only), stringResource(R.string.relay_only_note), tile = { IconTile(Icons.Outlined.Dns, t.irohBg, t.irohFg) }) {
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
                    CardRow(stringResource(R.string.gif_auto), stringResource(R.string.gif_auto_note), tile = { IconTile(Icons.Outlined.Gif) }) {
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
                    CardRow(stringResource(R.string.send_threshold), stringResource(R.string.send_threshold_note), tile = { IconTile(Icons.Outlined.Straighten) }) {
                        Text("50 m", fontSize = 15.sp, color = t.tx2)
                    }
                    CardDivider(TileInset)
                    CardRow(stringResource(R.string.heartbeat), stringResource(R.string.heartbeat_note), tile = { IconTile(Icons.Outlined.MonitorHeart) }) {
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
            item { PrivacyCard(model, blocked) }
            item {
                Text(stringResource(R.string.lock_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
            }
            // What each conversation takes, and old media to clear (ADR-0034).
            item { SectionLabel(stringResource(R.string.storage), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    Box(Modifier.clickable(onClick = storage)) {
                        CardRow(stringResource(R.string.storage), stringResource(R.string.storage_note), merge = false, tile = { IconTile(Icons.Outlined.SdStorage) })
                    }
                }
            }
            if (Build.VERSION.SDK_INT >= 33) {
                item { SectionLabel(stringResource(R.string.language), Modifier.padding(top = 10.dp)) }
                item { LanguagePicker() }
            }
            item { SectionLabel(stringResource(R.string.history), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    Box(Modifier.clickable { importer.launch(arrayOf("*/*")) }) {
                        CardRow(stringResource(R.string.import_meshcore), stringResource(R.string.import_meshcore_note), merge = false, tile = { IconTile(Icons.Outlined.FileDownload) })
                    }
                }
            }
            // The encrypted backup (ADR-0037).
            if (model.canBackUp) {
                item { SectionLabel(stringResource(R.string.backup), Modifier.padding(top = 10.dp)) }
                item {
                    CardList {
                        BackupExportRow(model, stringResource(R.string.export_backup), stringResource(R.string.export_backup_note), tile = { IconTile(Icons.Outlined.Backup) })
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.demo), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.demo_network), stringResource(R.string.demo_network_note), tile = { IconTile(Icons.Outlined.Science) }) {
                        LichenSwitch(model.isDemo, { if (it) model.startDemo() else model.stopDemo() })
                    }
                    CardDivider(TileInset)
                    if (model.isDemo) {
                        // Off: messages wait for the radio, as when Bluetooth drops (§6.1).
                        CardRow(stringResource(R.string.demo_radio), stringResource(R.string.demo_radio_note), tile = { IconTile(Icons.Outlined.PowerSettingsNew) }) {
                            LichenSwitch(!model.demoRadioOff, { model.setDemoRadio(it) })
                        }
                        CardDivider(TileInset)
                    }
                    Box(Modifier.clickable { model.markOnboarded(false) }) {
                        CardRow(stringResource(R.string.show_welcome_again), merge = false, tile = { IconTile(Icons.Outlined.WavingHand) })
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.about), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.version), tile = { IconTile(Icons.Outlined.Info) }) { Text("${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})", color = t.tx2) }
                    CardDivider(TileInset)
                    CardRow(stringResource(R.string.stickers), stringResource(R.string.stickers_credit), tile = { IconTile(Icons.Outlined.EmojiEmotions) })
                    CardDivider(TileInset)
                    Row(Modifier.fillMaxWidth().clickable(onClick = identity), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) {
                            val f = model.fingerprint.take(2).joinToString(" ")
                            CardRow(
                                stringResource(R.string.your_identity),
                                stringResource(if (model.phraseBackedUp) R.string.key_backed_up else R.string.key_not_backed_up, f),
                                merge = false,
                                tile = { IconTile(Icons.Outlined.Fingerprint) },
                            )
                        }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                    CardDivider(TileInset)
                    Row(Modifier.fillMaxWidth().clickable(onClick = licences), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { CardRow(stringResource(R.string.licences), stringResource(R.string.licences_row), merge = false, tile = { IconTile(Icons.Outlined.Description) }) }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                    CardDivider(TileInset)
                    // ADR-0045: no message, key, name or position in it.
                    Row(Modifier.fillMaxWidth().clickable(onClick = report), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { CardRow(stringResource(R.string.report_problem), stringResource(R.string.report_row), merge = false, tile = { IconTile(Icons.Outlined.BugReport) }) }
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2, modifier = Modifier.padding(end = 12.dp))
                    }
                }
            }
        }
    }
    if (choosingAdvert) AdvertChoice(model) { choosingAdvert = false }
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

/** The advert row's subtitle (B-user-4): what an advert is for, then what the radio answered. */
@Composable
private fun advertStatus(a: AppModel.AdvertState?): String = when (a) {
    null -> stringResource(R.string.send_advert_note)
    is AppModel.AdvertState.Sending -> stringResource(R.string.advert_sending)
    is AppModel.AdvertState.Sent -> stringResource(
        if (a.flood) R.string.advert_sent_mesh else R.string.advert_sent_nearby,
        timeOf(LocalContext.current, a.atMs),
    )
    is AppModel.AdvertState.NotSent -> stringResource(R.string.advert_not_sent)
}

/** How far our advert goes, chosen as in the stock app (B-user-4, D-14): nearby (zero hop) or
 * the whole mesh (flood), each with what it reaches; what it carries: the radio's name, and its
 * position when position in adverts is on and the radio has one. */
@Composable
private fun AdvertChoice(model: AppModel, close: () -> Unit) {
    val t = lichen
    val r = model.radioSettings
    val name = r?.name?.ifEmpty { null } ?: model.nodeName
    val withPosition = r != null && r.positionInAdverts && (r.latE6 != 0 || r.lonE6 != 0)
    AlertDialog(
        onDismissRequest = close,
        title = { Text(stringResource(R.string.send_advert)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                for ((flood, title, note) in listOf(
                    Triple(false, R.string.advert_nearby, R.string.advert_nearby_note),
                    Triple(true, R.string.advert_mesh, R.string.advert_mesh_note),
                )) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clickable {
                                close()
                                model.sendAdvert(flood)
                            }
                            .padding(vertical = 10.dp),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Text(stringResource(title), fontSize = 16.sp, fontWeight = FontWeight.Medium, color = t.tx)
                        Text(stringResource(note), fontSize = 14.sp, color = t.tx2)
                    }
                }
                Text(
                    stringResource(if (withPosition) R.string.advert_carries_position else R.string.advert_carries, name),
                    fontSize = 13.sp,
                    color = t.tx2,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = close) { Text(stringResource(R.string.cancel), color = t.tx2) } },
        containerColor = t.s1,
    )
}

/** The app lock ([com.tduhamel.lichen.model.AppLock]): on after one successful authentication,
 * off by default. */
@Composable
private fun PrivacyCard(model: AppModel, blocked: () -> Unit) {
    val t = lichen
    val lock = model.lock
    val prompt = LocalLockPrompt.current
    var choosing by remember { mutableStateOf(false) }
    CardList {
        CardRow(stringResource(R.string.lock_lichen), stringResource(R.string.lock_lichen_note), tile = { IconTile(Icons.Outlined.Lock) }) {
            LichenSwitch(
                lock.enabled,
                { if (it) prompt(AppLock.Purpose.ENABLE) else lock.disable() },
                enabled = lock.pending == null,
            )
        }
        CardDivider(TileInset)
        Box(Modifier.alpha(if (lock.enabled) 1f else 0.5f).clickable(enabled = lock.enabled) { choosing = true }) {
            CardRow(stringResource(R.string.lock_after), merge = false, tile = { IconTile(Icons.Outlined.Timer) }) {
                Box {
                    Text(lockDelayLabel(lock.delay), fontSize = 15.sp, color = t.tx2)
                    DropdownMenu(choosing, { choosing = false }, containerColor = t.s1) {
                        AppLock.DELAYS.forEach { s ->
                            DropdownMenuItem(
                                text = { Text(lockDelayLabel(s), color = t.tx) },
                                trailingIcon = { if (s == lock.delay) Icon(Icons.Outlined.Check, stringResource(R.string.chosen_option), tint = t.tx) },
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
        CardDivider(TileInset)
        // People the user blocked (ADR-0034), to unblock.
        Box(Modifier.clickable(onClick = blocked)) {
            CardRow(stringResource(R.string.blocked), merge = false, tile = { IconTile(Icons.Outlined.Block) }) {
                Text("${model.blocked.size}", fontSize = 15.sp, color = t.tx2)
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

/** The radio, compact (mockup `Settings`): its name, link, battery and the hour's airtime left,
 * and "connected"; without one, a way to pair it. */
@Composable
private fun RadioCard(model: AppModel, pair: () -> Unit) {
    val t = lichen
    Column(
        Modifier.fillMaxWidth().background(t.s1, RoundedCornerShape(18.dp)).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (model.isConnected) {
            val left = model.budget.limitMs - minOf(model.budget.usedMs, model.budget.limitMs)
            Row(
                Modifier.semantics(mergeDescendants = true) {},
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                IconTile(painterResource(R.drawable.ic_radio_tower), t.loraBg, t.loraFg, size = 44.dp)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(
                        model.nodeName.ifEmpty { stringResource(R.string.radio) },
                        fontSize = 17.sp, fontWeight = FontWeight.Bold, color = t.tx, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                    // While a connection still reads the radio, what it reads, and how far.
                    val reading = radioReadingText(model)
                    Text(
                        if (reading != null) radioLink(model) + " · " + reading else listOfNotNull(
                            radioLink(model),
                            model.radioSettings?.batteryPercent?.let { "$it %" },
                            stringResource(R.string.airtime_left_s, (left / 1000u).toInt()),
                        ).joinToString(" · "),
                        fontSize = 13.sp, color = t.tx2,
                    )
                    RadioReadingBar(model, Modifier.padding(top = 4.dp))
                }
                Badge(stringResource(R.string.radio_connected_short), t.okBg, t.okFg)
            }
        } else {
            Row(
                Modifier.fillMaxWidth().clickable(onClick = pair),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                IconTile(painterResource(R.drawable.ic_radio_tower), size = 44.dp)
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.pair_a_radio), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.no_radio), fontSize = 13.sp, color = t.tx2)
                }
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = t.tx2)
            }
        }
        // Over TCP, also while it reconnects: stops trying.
        if ((model.isConnected || model.tcp.target != null) && !model.isDemo) {
            SecondaryButton(stringResource(R.string.disconnect), background = t.sosBg, foreground = t.sosFg) { model.disconnect() }
        }
    }
}

/** The link to the radio as people say it: "Bluetooth", "USB", "TCP · not encrypted" (SR-31), or
 * the demo network. */
@Composable
fun radioLink(model: AppModel): String = when {
    model.isDemo -> stringResource(R.string.demo_network)
    model.linkKind == "BLE" -> stringResource(R.string.bluetooth)
    model.linkKind == "TCP" -> "TCP · ${stringResource(R.string.tcp_not_encrypted)}"
    else -> model.linkKind
}

/** "connected 2 h 14 min", "connected just now": since the link came up, kept up to date. */
@Composable
fun connectedFor(model: AppModel): String {
    val since = model.connectedSinceMs ?: return stringResource(R.string.radio_connected_short)
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(since) {
        while (true) {
            now = SystemClock.elapsedRealtime()
            delay(20_000)
        }
    }
    val min = ((now - since) / 60_000).toInt()
    return when {
        min < 1 -> stringResource(R.string.connected_now)
        min < 60 -> stringResource(R.string.connected_for, "$min min")
        min % 60 == 0 -> stringResource(R.string.connected_for, "${min / 60} h")
        else -> stringResource(R.string.connected_for, "${min / 60} h ${min % 60} min")
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
