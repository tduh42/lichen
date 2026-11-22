package com.tduhamel.lichen.ui

import androidx.compose.ui.text.style.TextAlign
import androidx.compose.foundation.layout.fillMaxWidth
import com.tduhamel.lichen.core.meshcoreContactCode
import com.tduhamel.lichen.core.NodeKind
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.RadioSettings
import com.tduhamel.lichen.core.RadioSetting
import com.tduhamel.lichen.core.SettingOutcome
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.outlined.Info
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import com.tduhamel.lichen.core.Telemetry
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.bandwidth
import kotlin.math.roundToInt

/**
 * LoRa presets with a firm source (ADR-0032): MeshCore's FAQ for USA/Canada, the EU/UK narrow
 * preset its communities use (869.618 MHz, as stock radios ship in Europe). Other regions: the
 * values the local community gives, by hand.
 */
private enum class RadioPreset(val label: Int, val values: Values?) {
    EU_NARROW(R.string.preset_eu_narrow, Values(869_618u, 62_500u, 8u, 8u)),
    US_CANADA(R.string.preset_us_canada, Values(910_525u, 62_500u, 7u, 5u)),
    CUSTOM(R.string.preset_custom, null);

    data class Values(val freqKhz: UInt, val bwHz: UInt, val sf: UByte, val cr: UByte)

    companion object {
        fun matching(r: RadioSettings): RadioPreset =
            entries.firstOrNull { it.values == Values(r.freqKhz, r.bwHz, r.sf, r.cr) } ?: CUSTOM
    }
}

private val BANDWIDTHS = listOf(7_800u, 10_400u, 15_600u, 20_800u, 31_250u, 41_700u, 62_500u, 125_000u, 250_000u, 500_000u)
private val INTERVALS = listOf(60u, 300u, 900u, 3_600u)

/**
 * The radio's own settings (protocol §1.1, ADR-0032): read from the radio at each connection
 * and after each change; the radio keeps them, lichen stores none.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RadioSettingsScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    LaunchedEffect(Unit) { model.refreshRadio() }
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(stringResource(R.string.radio_settings), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        val r = model.radioSettings
        if (!model.isConnected) {
            Text(stringResource(R.string.radio_settings_none), color = t.tx2, modifier = Modifier.padding(20.dp))
            return@Column
        }
        // Linked, but the radio has not answered yet: nothing of its own to show (its values are
        // zeros waiting for it), however long it takes (D-9).
        if (r == null || !r.read) {
            Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                CircularProgressIndicator(Modifier.size(18.dp), color = t.tx2, strokeWidth = 2.dp)
                Text(stringResource(R.string.reading_radio_settings), color = t.tx2)
            }
            return@Column
        }
        // While a setting waits for the radio, the value asked for (B-user-5).
        val shown = model.radioAsked ?: r
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { DeviceSection(model, shown) }
            item { MeshCoreCode(model, shown) }
            item { LoraSection(model, shown) }
            item { PowerSection(model, shown) }
            if (shown.gps != null) item { GpsSection(model, shown) }
            item { PrivacySection(model, shown) }
            if (shown.autoaddMask != null) item { ContactsSection(model, shown) }
            item { DeliverySection(model, shown) }
            item { BluetoothSection(model) }
        }
    }
    model.radioError?.let {
        AlertDialog(
            onDismissRequest = { model.radioError = null },
            title = { Text(stringResource(R.string.not_changed)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { model.radioError = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun DeviceSection(model: AppModel, r: RadioSettings) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.radio), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(
                r.manufacturer.ifEmpty { stringResource(R.string.meshcore_radio) },
                if (r.firmware.isEmpty()) null else stringResource(R.string.radio_firmware, r.firmware, r.buildDate),
            )
            r.batteryMv?.let { mv ->
                CardDivider()
                val percent = (((mv.toDouble() - 3_300) / 900).coerceIn(0.0, 1.0) * 100).roundToInt()
                CardRow(stringResource(R.string.battery), stringResource(R.string.radio_battery, "%.2f".format(mv.toDouble() / 1000), percent))
            }
            val used = r.storageUsedKb
            val total = r.storageTotalKb
            if (used != null && total != null && total > 0u) {
                CardDivider()
                CardRow(stringResource(R.string.storage), stringResource(R.string.radio_storage, used.toInt(), total.toInt()))
            }
            // Its MeshCore public key, to copy or share (F-1).
            model.nodeKey?.let { key ->
                CardDivider()
                MeshCoreKeyRow(key, share = true)
            }
        }
    }
}

/** The stock MeshCore app's contact code for this radio (F-1, §15.3): a QR code a MeshCore user
 * scans to add it. */
@Composable
private fun MeshCoreCode(model: AppModel, r: RadioSettings) {
    val t = lichen
    val key = model.nodeKey ?: return
    val code = remember(r.name, key.toList()) { runCatching { meshcoreContactCode(r.name, key, NodeKind.COMPANION) }.getOrNull() } ?: return
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.meshcore_code), Modifier.padding(top = 10.dp))
        Column(
            Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            QrImage(code, stringResource(R.string.meshcore_code), Modifier.size(220.dp))
            Text(stringResource(R.string.meshcore_code_note), fontSize = 13.sp, color = t.tx2, textAlign = TextAlign.Center)
        }
    }
}

private fun mhz(khz: UInt) = "%.3f".format(khz.toDouble() / 1000)

private fun summary(khz: UInt, hz: UInt, sf: UByte, cr: UByte) = "${mhz(khz)} MHz · ${bandwidth(hz)} · SF$sf · CR 4/$cr"

@Composable
private fun LoraSection(model: AppModel, r: RadioSettings) {
    val t = lichen
    // What is being chosen, from the radio's LoRa parameters: kept while anything else of the
    // radio is read again (its battery), started afresh when these change.
    val lora = listOf(r.freqKhz, r.bwHz, r.sf, r.cr)
    var preset by remember(lora) { mutableStateOf(RadioPreset.matching(r)) }
    var freq by remember(lora) { mutableStateOf(mhz(r.freqKhz)) }
    var bw by remember(lora) { mutableStateOf(r.bwHz) }
    var sf by remember(lora) { mutableStateOf(r.sf) }
    var cr by remember(lora) { mutableStateOf(r.cr) }
    var confirming by remember { mutableStateOf(false) }
    val chosen: RadioPreset.Values? = preset.values ?: freq.replace(',', '.').toDoubleOrNull()?.takeIf { it > 0 }?.let {
        RadioPreset.Values((it * 1000).roundToInt().toUInt(), bw, sf, cr)
    }
    val current = RadioPreset.Values(r.freqKhz, r.bwHz, r.sf, r.cr)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.lora), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(stringResource(R.string.lora_now), summary(r.freqKhz, r.bwHz, r.sf, r.cr))
            CardDivider()
            CardRow(stringResource(R.string.preset)) {
                Choice(stringResource(preset.label), RadioPreset.entries.map { stringResource(it.label) to it }) { preset = it }
            }
            if (preset == RadioPreset.CUSTOM) {
                CardDivider()
                CardRow(stringResource(R.string.frequency_mhz)) {
                    OutlinedTextField(
                        freq, { freq = it }, Modifier.width(120.dp),
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        shape = RoundedCornerShape(10.dp),
                        colors = fieldColors(),
                    )
                }
                CardDivider()
                CardRow(stringResource(R.string.bandwidth)) { Choice(bandwidth(bw), BANDWIDTHS.map { bandwidth(it) to it }) { bw = it } }
                CardDivider()
                CardRow(stringResource(R.string.spreading_factor)) { Choice("SF$sf", (5..12).map { "SF$it" to it.toUByte() }) { sf = it } }
                CardDivider()
                CardRow(stringResource(R.string.coding_rate)) { Choice("4/$cr", (5..8).map { "4/$it" to it.toUByte() }) { cr = it } }
            }
            CardDivider()
            val enabled = chosen != null && chosen != current
            Box(Modifier.clickable(enabled = enabled) { confirming = true }) {
                CardRow(stringResource(R.string.apply), merge = false) {
                    Text(if (enabled) "✓" else "", color = t.tx2)
                }
            }
            RowStatus(model, "lora")
        }
        Text(stringResource(R.string.lora_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
    }
    if (confirming && chosen != null) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(stringResource(R.string.lora_confirm_title)) },
            text = { Text(stringResource(R.string.lora_confirm, summary(chosen.freqKhz, chosen.bwHz, chosen.sf, chosen.cr), summary(r.freqKhz, r.bwHz, r.sf, r.cr))) },
            confirmButton = {
                TextButton(onClick = {
                    confirming = false
                    model.radioSet(RadioSetting.PARAMS, "lora", { it.copy(freqKhz = chosen.freqKhz, bwHz = chosen.bwHz, sf = chosen.sf, cr = chosen.cr) }) {
                        model.core.setRadioParams(chosen.freqKhz, chosen.bwHz, chosen.sf, chosen.cr)
                    }
                }) { Text(stringResource(R.string.apply), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun PowerSection(model: AppModel, r: RadioSettings) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.transmit_power), Modifier.padding(top = 10.dp))
        CardList {
            CardRow("${r.txDbm} dBm", stringResource(R.string.transmit_power_note, r.maxTxDbm.toInt())) {
                Stepper(r.txDbm.toInt(), -9..r.maxTxDbm.toInt()) { v ->
                    model.radioSet(RadioSetting.TX_POWER, "power", { it.copy(txDbm = v.toByte()) }) { model.core.setTxPower(v.toByte()) }
                }
            }
            RowStatus(model, "power")
        }
    }
}

@Composable
private fun GpsSection(model: AppModel, r: RadioSettings) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.gps), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(stringResource(R.string.radio_gps), stringResource(R.string.radio_gps_note)) {
                LichenSwitch(r.gps == true, { on -> model.radioSet(RadioSetting.GPS, "gps", { it.copy(gps = on) }) { model.core.setGps(on) } })
            }
            RowStatus(model, "gps")
            if (r.gps == true) {
                CardDivider()
                CardRow(stringResource(R.string.gps_every)) {
                    // The radio's own value: one it refuses is not shown (D-4).
                    val label = r.gpsIntervalS?.let { minutes(it) } ?: stringResource(R.string.not_set)
                    Choice(label, INTERVALS.map { minutes(it) to it }) { s ->
                        model.radioSet(RadioSetting.GPS_INTERVAL, "gps_interval", { it.copy(gpsIntervalS = s) }) { model.core.setGpsInterval(s) }
                    }
                }
                RowStatus(model, "gps_interval")
            }
        }
    }
}

@Composable
private fun minutes(s: UInt): String =
    if (s >= 3_600u) stringResource(R.string.hours_n, (s / 3_600u).toInt()) else stringResource(R.string.minutes_n, (s / 60u).toInt())

private fun AppModel.setPrefs(
    r: RadioSettings,
    row: String,
    manualAdd: Boolean = r.manualAdd,
    telemetry: Telemetry = r.telemetry,
    position: Boolean = r.positionInAdverts,
    multiAcks: UByte = r.multiAcks,
) = radioSet(
    RadioSetting.PREFERENCES, row,
    { it.copy(manualAdd = manualAdd, telemetry = telemetry, positionInAdverts = position, multiAcks = multiAcks) },
) { core.setRadioPreferences(manualAdd, telemetry, position, multiAcks) }

@Composable
private fun PrivacySection(model: AppModel, r: RadioSettings) {
    // A radio advertising its position takes this phone's (below) when it has none of its own:
    // without GPS, or with its GPS on but no fix yet, indoors (B-user-2). The phone's location
    // runs while that choice is on screen, as on the map. A radio whose GPS has a position keeps
    // it: its own fix is better.
    val noPosition = r.latE6 == 0 && r.lonE6 == 0
    val usesPhone = r.positionInAdverts && (r.gps != true || noPosition)
    DisposableEffect(usesPhone) {
        if (usesPhone) model.needsLocation("radio", true)
        onDispose { if (usesPhone) model.needsLocation("radio", false) }
    }
    val t = lichen
    var confirmingPosition by remember { mutableStateOf(false) }
    var confirmingTelemetry by remember { mutableStateOf<UByte?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.position_and_telemetry), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(
                stringResource(R.string.position_in_adverts),
                stringResource(if (r.positionInAdverts) R.string.position_in_adverts_on else R.string.position_in_adverts_off),
            ) {
                LichenSwitch(r.positionInAdverts, { on -> if (on) confirmingPosition = true else model.setPrefs(r, "position", position = false) })
            }
            RowStatus(model, "position")
            if (r.positionInAdverts) {
                CardDivider()
                val pos = if (r.latE6 == 0 && r.lonE6 == 0) stringResource(R.string.none_yet)
                // Figures any reader and any map takes, "45.12345, 5.12345", as a place in a message.
                else String.format(java.util.Locale.ROOT, "%.5f, %.5f", r.latE6 / 1e6, r.lonE6 / 1e6)
                CardRow(stringResource(R.string.position_advertised), pos)
                // Why there is none: the GPS is on but has no fix (it needs open sky).
                if (r.gps == true && noPosition) {
                    Text(
                        stringResource(R.string.advert_gps_no_fix),
                        fontSize = 13.sp,
                        color = t.tx2,
                        modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 11.dp),
                    )
                }
                val fix = model.myFix
                if (usesPhone && fix != null) {
                    CardDivider()
                    Box(Modifier.clickable {
                        val lat = (fix.lat * 1e6).roundToInt()
                        val lon = (fix.lon * 1e6).roundToInt()
                        model.radioSet(RadioSetting.ADVERT_POSITION, "advert_position", { it.copy(latE6 = lat, lonE6 = lon) }) {
                            model.core.setAdvertPosition(lat, lon)
                        }
                    }) {
                        CardRow(stringResource(R.string.use_phone_position), merge = false)
                    }
                    RowStatus(model, "advert_position")
                }
            }
            CardDivider()
            TelemetryRow(stringResource(R.string.telemetry_base), r.telemetry.base) { v ->
                model.setPrefs(r, "telemetry_base", telemetry = r.telemetry.copy(base = v))
            }
            RowStatus(model, "telemetry_base")
            CardDivider()
            TelemetryRow(stringResource(R.string.telemetry_location), r.telemetry.location) { v ->
                if (v.toInt() == 0) model.setPrefs(r, "telemetry_location", telemetry = r.telemetry.copy(location = v)) else confirmingTelemetry = v
            }
            RowStatus(model, "telemetry_location")
            CardDivider()
            TelemetryRow(stringResource(R.string.telemetry_environment), r.telemetry.environment) { v ->
                model.setPrefs(r, "telemetry_environment", telemetry = r.telemetry.copy(environment = v))
            }
            RowStatus(model, "telemetry_environment")
        }
        Text(stringResource(R.string.telemetry_note), fontSize = 12.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
    }
    if (confirmingPosition) {
        AlertDialog(
            onDismissRequest = { confirmingPosition = false },
            title = { Text(stringResource(R.string.position_confirm_title)) },
            text = { Text(stringResource(R.string.position_confirm)) },
            confirmButton = {
                TextButton(onClick = { confirmingPosition = false; model.setPrefs(r, "position", position = true) }) {
                    Text(stringResource(R.string.show_it), color = t.sos)
                }
            },
            dismissButton = { TextButton(onClick = { confirmingPosition = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
    confirmingTelemetry?.let { v ->
        AlertDialog(
            onDismissRequest = { confirmingTelemetry = null },
            title = { Text(stringResource(R.string.telemetry_confirm_title)) },
            text = { Text(stringResource(R.string.telemetry_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmingTelemetry = null
                    model.setPrefs(r, "telemetry_location", telemetry = r.telemetry.copy(location = v))
                }) { Text(stringResource(R.string.allow), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { confirmingTelemetry = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun TelemetryRow(title: String, value: UByte, set: (UByte) -> Unit) {
    val labels = listOf(stringResource(R.string.nobody), stringResource(R.string.chosen_contacts), stringResource(R.string.everyone))
    CardRow(title) {
        Choice(labels.getOrElse(value.toInt()) { "?" }, labels.mapIndexed { i, l -> l to i.toUByte() }, set)
    }
}

@Composable
private fun ContactsSection(model: AppModel, r: RadioSettings) {
    val mask = r.autoaddMask ?: 0u
    val hops = r.autoaddMaxHops ?: 0u
    fun setMask(bit: Int, on: Boolean) {
        val m = (if (on) mask.toInt() or bit else mask.toInt() and bit.inv()).toUByte()
        model.radioSet(RadioSetting.AUTOADD, "autoadd_$bit", { it.copy(autoaddMask = m, autoaddMaxHops = hops) }) { model.core.setAutoadd(m, hops) }
    }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.new_contacts), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(
                stringResource(R.string.add_every_radio),
                stringResource(if (r.manualAdd) R.string.add_every_radio_off else R.string.add_every_radio_on),
            ) {
                LichenSwitch(!r.manualAdd, { on -> model.setPrefs(r, "manual_add", manualAdd = !on) })
            }
            RowStatus(model, "manual_add")
            if (r.manualAdd) {
                listOf(
                    0x02 to R.string.kind_companions,
                    0x04 to R.string.kind_repeaters,
                    0x08 to R.string.kind_rooms,
                    0x10 to R.string.kind_sensors,
                ).forEach { (bit, label) ->
                    CardDivider()
                    CardRow(stringResource(label)) { LichenSwitch(mask.toInt() and bit != 0, { setMask(bit, it) }) }
                    RowStatus(model, "autoadd_$bit")
                }
            }
            CardDivider()
            CardRow(stringResource(R.string.replace_oldest)) { LichenSwitch(mask.toInt() and 0x01 != 0, { setMask(0x01, it) }) }
            RowStatus(model, "autoadd_1")
            CardDivider()
            CardRow(stringResource(R.string.from_how_far)) {
                val choices = (listOf(0, 1, 2, 3, 4, 6) + hops.toInt()).distinct().sorted()
                Choice(hopLabel(hops.toInt()), choices.map { hopLabel(it) to it.toUByte() }) { h ->
                    model.radioSet(RadioSetting.AUTOADD, "autoadd_hops", { it.copy(autoaddMaxHops = h) }) { model.core.setAutoadd(mask, h) }
                }
            }
            RowStatus(model, "autoadd_hops")
        }
    }
}

/** 0 any distance, 1 direct only, N up to N − 1 hops (the firmware's rule). */
@Composable
private fun hopLabel(h: Int): String = when (h) {
    0 -> stringResource(R.string.any_distance)
    1 -> stringResource(R.string.direct_only)
    else -> pluralStringResource(R.plurals.up_to_hops, h - 1, h - 1)
}

@Composable
private fun DeliverySection(model: AppModel, r: RadioSettings) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.delivery), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(stringResource(R.string.extra_acks, r.multiAcks.toInt()), stringResource(R.string.extra_acks_note)) {
                Stepper(r.multiAcks.toInt(), 0..2) { v -> model.setPrefs(r, "multi_acks", multiAcks = v.toUByte()) }
            }
            RowStatus(model, "multi_acks")
        }
    }
}

@Composable
private fun BluetoothSection(model: AppModel) {
    val t = lichen
    var pin by remember { mutableStateOf("") }
    var saved by remember { mutableStateOf(false) }
    var confirmingReboot by remember { mutableStateOf(false) }
    val valid = pin.isEmpty() || (pin.length == 6 && pin.all(Char::isDigit))
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.bluetooth), Modifier.padding(top = 10.dp))
        CardList {
            CardRow(stringResource(R.string.ble_pin), stringResource(if (saved) R.string.pin_saved else R.string.pin_note)) {
                OutlinedTextField(
                    pin, { pin = it.take(6) }, Modifier.width(110.dp),
                    singleLine = true,
                    placeholder = { Text("123456") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
                    shape = RoundedCornerShape(10.dp),
                    colors = fieldColors(),
                )
            }
            CardDivider()
            Box(Modifier.clickable(enabled = valid) {
                model.radioSet(RadioSetting.PIN, "pin", { it }) { model.core.setBlePin(pin.toUIntOrNull() ?: 0u) }
                saved = model.radioError == null
            }) {
                CardRow(stringResource(R.string.save_pin), merge = false)
            }
            RowStatus(model, "pin")
            CardDivider()
            Box(Modifier.clickable { confirmingReboot = true }) {
                CardRow(stringResource(R.string.restart_radio), merge = false)
            }
        }
    }
    if (confirmingReboot) {
        AlertDialog(
            onDismissRequest = { confirmingReboot = false },
            title = { Text(stringResource(R.string.restart_radio_confirm_title)) },
            text = { Text(stringResource(R.string.restart_radio_confirm)) },
            confirmButton = {
                TextButton(onClick = { confirmingReboot = false; model.rebootRadio() }) {
                    Text(stringResource(R.string.restart), color = t.sos)
                }
            },
            dismissButton = { TextButton(onClick = { confirmingReboot = false }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/**
 * What became of the setting changed from [row] (B-user-5): a spinner while the radio has not
 * answered, then "Saved on the radio" for 2 s, or why the value shown is still the radio's
 * (kept, refused, unanswered) until the next change.
 */
@Composable
private fun RowStatus(model: AppModel, row: String) {
    val state = model.radioRows[row] ?: return
    val t = lichen
    val outcome = state.outcome
    if (outcome == SettingOutcome.SAVED) LaunchedEffect(state) {
        delay(2_000)
        model.clearRadioRow(row)
    }
    val waiting = stringResource(R.string.radio_waiting)
    Row(
        Modifier.padding(start = 16.dp, end = 16.dp, bottom = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        when (outcome) {
            null -> CircularProgressIndicator(
                Modifier.size(16.dp).semantics { contentDescription = waiting }, color = t.tx2, strokeWidth = 2.dp,
            )
            SettingOutcome.SAVED -> {
                Icon(Icons.Filled.Check, null, tint = t.ok, modifier = Modifier.size(16.dp))
                Text(stringResource(R.string.radio_saved), fontSize = 13.sp, color = t.tx)
            }
            else -> {
                Icon(Icons.Outlined.Info, null, tint = t.warn, modifier = Modifier.size(16.dp))
                val why = when (outcome) {
                    SettingOutcome.NOT_KEPT -> R.string.radio_not_kept
                    SettingOutcome.REFUSED -> R.string.radio_refused
                    else -> R.string.radio_no_answer
                }
                Text(stringResource(why), fontSize = 13.sp, color = t.tx)
            }
        }
    }
}

/** A value and a menu of the others. */
@Composable
private fun <T> Choice(label: String, options: List<Pair<String, T>>, pick: (T) -> Unit) {
    val t = lichen
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }) { Text(label, color = t.tx) }
        DropdownMenu(open, { open = false }, containerColor = t.s1) {
            options.forEach { (text, value) ->
                DropdownMenuItem(text = { Text(text, color = t.tx) }, onClick = { open = false; pick(value) })
            }
        }
    }
}

/** − value + within `range`. */
@Composable
private fun Stepper(value: Int, range: IntRange, set: (Int) -> Unit) {
    val t = lichen
    Row(verticalAlignment = Alignment.CenterVertically) {
        TextButton(onClick = { set(value - 1) }, enabled = value > range.first) { Text("−", color = t.tx, fontSize = 20.sp) }
        TextButton(onClick = { set(value + 1) }, enabled = value < range.last) { Text("+", color = t.tx, fontSize = 20.sp) }
    }
}
