package com.tduhamel.lichen.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Bluetooth
import androidx.compose.material.icons.outlined.SettingsInputAntenna
import androidx.compose.material.icons.outlined.Usb
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.AppModel.RadioState

/** Bluetooth and USB permissions the app needs to reach a radio, and to notify. */
private val radioPermissions = buildList {
    add(Manifest.permission.BLUETOOTH_SCAN)
    add(Manifest.permission.BLUETOOTH_CONNECT)
    if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
}

/** Pair a radio over Bluetooth, or plug one in over USB (mockup `appairage`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairingScreen(model: AppModel, back: () -> Unit) {
    val t = lichen
    val context = LocalContext.current
    fun granted() = listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
        .all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }
    var allowed by remember { mutableStateOf(granted()) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        allowed = granted()
        if (allowed) {
            model.bluetoothReady()
            model.scan()
        }
    }
    LaunchedEffect(Unit) {
        if (allowed) model.scan() else ask.launch(radioPermissions.toTypedArray())
    }
    DisposableEffect(Unit) { onDispose { model.stopScan() } }
    // Connected over Bluetooth or USB: back to where the user came from.
    var wasConnected by remember { mutableStateOf(model.isConnected) }
    LaunchedEffect(model.isConnected) {
        if (model.isConnected && !wasConnected && !model.isDemo) back()
        wasConnected = model.isConnected
    }

    val scanLine = when (val s = model.radioState) {
        RadioState.Off -> stringResource(R.string.bluetooth_off)
        is RadioState.Connecting -> stringResource(R.string.radio_connecting, s.name)
        is RadioState.Connected -> stringResource(R.string.radio_connected, s.name)
        else -> stringResource(R.string.searching_bluetooth)
    }

    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = {
                IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
            },
            title = { Text(stringResource(R.string.pair_a_radio), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { Text(stringResource(R.string.pair_intro), fontSize = 15.sp, color = t.tx2) }
            if (!allowed) {
                item {
                    Banner(
                        BannerKind.WARN,
                        stringResource(R.string.bluetooth_permission),
                        stringResource(R.string.bluetooth_permission_note),
                        stringResource(R.string.allow) to { ask.launch(radioPermissions.toTypedArray()) },
                    )
                }
            } else {
                item {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Icon(Icons.Outlined.Bluetooth, null, tint = t.tx2, modifier = Modifier.size(16.dp))
                        Text(scanLine, fontSize = 13.sp, color = t.tx2)
                    }
                }
            }
            if (model.found.isNotEmpty()) {
                item {
                    CardList {
                        model.found.forEachIndexed { i, f ->
                            if (i > 0) CardDivider()
                            RadioRow(
                                Icons.Outlined.SettingsInputAntenna, f.name,
                                stringResource(R.string.dbm_companion, f.rssi), stringResource(R.string.pair),
                            ) { model.pair(f.address) }
                        }
                    }
                }
            }
            item {
                Column(
                    Modifier.fillMaxWidth().background(t.s1, CardShape).padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Text(stringResource(R.string.code_on_radio), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
                    Text(stringResource(R.string.code_on_radio_note), fontSize = 14.sp, color = t.tx2)
                    Row(
                        Modifier.fillMaxWidth().clearAndSetSemantics {},
                        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    ) {
                        repeat(6) {
                            Box(
                                Modifier
                                    .size(width = 44.dp, height = 52.dp)
                                    .background(t.s2, RoundedCornerShape(10.dp))
                                    .border(1.dp, t.line, RoundedCornerShape(10.dp)),
                            )
                        }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.usb), Modifier.padding(top = 8.dp)) }
            item { Text(stringResource(R.string.usb_note), fontSize = 14.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp)) }
            item {
                if (model.usbDevices.isEmpty()) {
                    Text(stringResource(R.string.no_usb), fontSize = 13.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp))
                } else {
                    CardList {
                        model.usbDevices.forEachIndexed { i, d ->
                            if (i > 0) CardDivider()
                            RadioRow(
                                Icons.Outlined.Usb, d.productName ?: "USB",
                                stringResource(R.string.usb_serial, d.manufacturerName ?: "CDC-ACM"),
                                stringResource(R.string.connect),
                            ) { model.connectUsb(d) }
                        }
                    }
                }
            }
            item { SectionLabel(stringResource(R.string.network), Modifier.padding(top = 8.dp)) }
            item { Text(stringResource(R.string.network_note), fontSize = 14.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp)) }
            item {
                var address by remember { mutableStateOf(model.tcpAddress) }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        address, { address = it },
                        Modifier.weight(1f),
                        singleLine = true,
                        textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 15.sp, color = t.tx),
                        shape = RoundedCornerShape(12.dp),
                        colors = fieldColors(),
                    )
                    SecondaryButton(stringResource(R.string.connect), Modifier.width(120.dp), enabled = address.contains(':')) {
                        model.connectTcp(address.trim())
                    }
                }
            }
            if (!model.isDemo) {
                item {
                    SecondaryButton(stringResource(R.string.try_demo_instead), Modifier.padding(top = 8.dp)) {
                        model.startDemo()
                        back()
                    }
                }
            }
        }
    }
}

@Composable
private fun RadioRow(icon: ImageVector, name: String, detail: String, action: String, onClick: () -> Unit) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(40.dp).background(t.s2, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = t.lora)
        }
        Column(Modifier.weight(1f)) {
            Text(name, fontSize = 16.sp, color = t.tx)
            Text(detail, fontSize = 13.sp, color = t.tx2)
        }
        SecondaryButton(action, Modifier.width(110.dp), onClick = onClick)
    }
}
