package com.tduhamel.lichen.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.BuildConfig
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.Appearance
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.bandwidth

/** Radio, appearance, demo network (mockup `reglages`; the rest comes with L9b–L9f). */
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(model: AppModel, pair: () -> Unit, identity: () -> Unit) {
    val t = lichen
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
            item { SectionLabel(stringResource(R.string.radio)) }
            item {
                CardList {
                    val r = model.radio
                    CardRow(
                        if (model.isConnected) model.nodeName.ifEmpty { stringResource(R.string.radio) } else stringResource(R.string.no_radio),
                        if (r != null && model.isConnected) {
                            "%s · %.3f MHz · SF%d · %s · CR 4/%d".format(
                                model.linkKind, r.freqKhz.toDouble() / 1000, r.sf.toInt(), bandwidth(r.bwHz), r.cr.toInt(),
                            )
                        } else {
                            null
                        },
                    )
                    CardDivider()
                    if (model.isConnected && !model.isDemo) {
                        CardRow(stringResource(R.string.disconnect)) {
                            TextButton(onClick = { model.disconnect() }) {
                                Text(stringResource(R.string.disconnect), fontWeight = FontWeight.SemiBold, color = t.sos)
                            }
                        }
                    } else if (!model.isDemo) {
                        CardRow(stringResource(R.string.connect_radio), stringResource(R.string.connect_radio_note)) {
                            TextButton(onClick = pair) {
                                Text(stringResource(R.string.connect), fontWeight = FontWeight.SemiBold, color = t.tx)
                            }
                        }
                    }
                    CardDivider()
                    CardRow(stringResource(R.string.send_advert), stringResource(R.string.send_advert_note)) {
                        TextButton(onClick = { model.sendAdvert() }, enabled = model.isConnected) {
                            Text(stringResource(R.string.send), fontWeight = FontWeight.SemiBold, color = if (model.isConnected) t.tx else t.tx2)
                        }
                    }
                }
            }
            item { Gap(8.dp) }
            item { SectionLabel(stringResource(R.string.internet_transports)) }
            item {
                val irohLine = when {
                    !model.irohEnabled -> stringResource(R.string.off)
                    !model.bridgeOnline -> stringResource(R.string.connecting)
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
            item { Gap(8.dp) }
            item { SectionLabel(stringResource(R.string.identity)) }
            item {
                CardList {
                    androidx.compose.foundation.layout.Box(Modifier.clickable(onClick = identity)) {
                        CardRow(
                            stringResource(R.string.your_identity),
                            if (model.phraseBackedUp) model.fingerprint.take(2).joinToString(" ") else stringResource(R.string.not_backed_up),
                        )
                    }
                }
            }
            item { Gap(8.dp) }
            item { SectionLabel(stringResource(R.string.airtime)) }
            item {
                CardList {
                    androidx.compose.foundation.layout.Box(Modifier.padding(16.dp)) {
                        AirtimeMeter(model.budget.usedMs, model.budget.limitMs, large = true)
                    }
                }
            }
            item { Gap(8.dp) }
            item { SectionLabel(stringResource(R.string.appearance)) }
            item {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    val options = listOf(Appearance.SYSTEM to R.string.system, Appearance.DARK to R.string.dark, Appearance.LIGHT to R.string.light)
                    options.forEachIndexed { i, (a, label) ->
                        SegmentedButton(
                            selected = model.appearance == a,
                            onClick = { model.chooseAppearance(a) },
                            shape = SegmentedButtonDefaults.itemShape(i, options.size),
                            colors = SegmentedButtonDefaults.colors(
                                activeContainerColor = t.s2, activeContentColor = t.tx,
                                inactiveContainerColor = t.s1, inactiveContentColor = t.tx2,
                                activeBorderColor = t.line, inactiveBorderColor = t.line,
                            ),
                        ) { Text(stringResource(label)) }
                    }
                }
            }
            item { Gap(8.dp) }
            item { SectionLabel(stringResource(R.string.demo)) }
            item {
                CardList {
                    CardRow(stringResource(R.string.demo_network), stringResource(R.string.demo_network_note)) {
                        LichenSwitch(model.isDemo, { if (it) model.startDemo() else model.stopDemo() })
                    }
                }
            }
            item {
                Text(
                    "lichen ${BuildConfig.VERSION_NAME}",
                    fontSize = 12.sp, color = t.tx2,
                    modifier = Modifier.fillMaxWidth().padding(top = 16.dp, start = 4.dp),
                )
            }
        }
    }
}
