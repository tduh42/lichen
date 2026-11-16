package com.tduhamel.lichen.model

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import com.tduhamel.lichen.core.companionBleUuids
import java.util.UUID

/**
 * The MeshCore companion over Bluetooth LE, as the iOS app's `Companion`: Nordic UART service,
 * one write or notification per companion frame (up to 176 bytes, hence the larger MTU), writes
 * with response, one at a time. The radio's 6-digit code goes through Android's pairing dialog,
 * on the first use of its encrypted characteristics. Reports on the main thread.
 */
@SuppressLint("MissingPermission") // Every entry point checks the Bluetooth permissions first.
class BleCompanion(
    private val context: Context,
    private val onState: (AppModel.RadioState) -> Unit,
    private val onFound: (List<Found>) -> Unit,
    private val onFrame: (ByteArray) -> Unit,
) {
    data class Found(val address: String, val name: String, val rssi: Int)

    private val uuids = companionBleUuids()
    private val service = UUID.fromString(uuids.service)
    private val rxUuid = UUID.fromString(uuids.rx)
    private val txUuid = UUID.fromString(uuids.tx)
    private val main = Handler(Looper.getMainLooper())
    private val prefs = context.getSharedPreferences("lichen", Context.MODE_PRIVATE)
    private val manager = context.getSystemService(BluetoothManager::class.java)
    private val adapter: BluetoothAdapter? get() = manager?.adapter

    private var gatt: BluetoothGatt? = null
    private var rx: BluetoothGattCharacteristic? = null
    private var deviceName = "MeshCore"
    private val found = mutableMapOf<String, Found>()
    private val queue = ArrayDeque<ByteArray>()
    private var writing = false
    private var ready = false
    private var scanning = false
    private var retriedAfterBond = false

    var state: AppModel.RadioState = AppModel.RadioState.Idle
        private set(value) {
            field = value
            onState(value)
        }

    /** The Bluetooth permissions are granted (Android 12+: scan and connect). */
    fun permitted(): Boolean = listOf(
        android.Manifest.permission.BLUETOOTH_SCAN,
        android.Manifest.permission.BLUETOOTH_CONNECT,
    ).all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }

    private val adapterState = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            when (i.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_ON -> {
                    state = AppModel.RadioState.Idle
                    reconnectLast()
                }
                BluetoothAdapter.STATE_OFF -> {
                    closeGatt()
                    state = AppModel.RadioState.Off
                }
            }
        }
    }

    private val bondState = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            val g = gatt ?: return
            val device = if (Build.VERSION.SDK_INT >= 33) {
                i.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java)
            } else {
                @Suppress("DEPRECATION")
                i.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE)
            }
            if (ready || device?.address != g.device.address) return
            when (i.getIntExtra(BluetoothDevice.EXTRA_BOND_STATE, BluetoothDevice.ERROR)) {
                // Paired with the radio's code: the link is encrypted, the notifications can go on.
                BluetoothDevice.BOND_BONDED -> enableNotifications(g)
                // Pairing refused or cancelled: the link is never used unprotected (review of
                // 2026-11-15, SR-32); the radio is forgotten so it does not ask again in a loop.
                BluetoothDevice.BOND_NONE ->
                    if (i.getIntExtra(BluetoothDevice.EXTRA_PREVIOUS_BOND_STATE, BluetoothDevice.ERROR) == BluetoothDevice.BOND_BONDING) {
                        disconnect()
                    }
            }
        }
    }

    init {
        ContextCompat.registerReceiver(
            context, adapterState, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        ContextCompat.registerReceiver(
            context, bondState, IntentFilter(BluetoothDevice.ACTION_BOND_STATE_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        if (adapter?.isEnabled != true) state = AppModel.RadioState.Off
    }

    /** Called once the permissions are granted: reconnects to the last radio. */
    fun start() {
        if (adapter?.isEnabled != true) {
            state = AppModel.RadioState.Off
            return
        }
        if (state == AppModel.RadioState.Off) state = AppModel.RadioState.Idle
        reconnectLast()
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val name = result.scanRecord?.deviceName ?: result.device.name ?: "MeshCore"
            main.post {
                found[result.device.address] = Found(result.device.address, name, result.rssi)
                onFound(found.values.sortedByDescending { it.rssi })
            }
        }
    }

    fun scan() {
        val scanner = adapter?.takeIf { it.isEnabled && permitted() }?.bluetoothLeScanner ?: return
        if (scanning) return
        found.clear()
        onFound(emptyList())
        scanning = true
        if (state !is AppModel.RadioState.Connected) state = AppModel.RadioState.Scanning
        scanner.startScan(
            listOf(ScanFilter.Builder().setServiceUuid(ParcelUuid(service)).build()),
            ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(),
            scanCallback,
        )
    }

    fun stopScan() {
        if (!scanning) return
        scanning = false
        if (permitted()) adapter?.bluetoothLeScanner?.stopScan(scanCallback)
        if (state == AppModel.RadioState.Scanning) state = AppModel.RadioState.Idle
    }

    fun connect(address: String) {
        stopScan()
        val device = adapter?.takeIf { permitted() }?.getRemoteDevice(address) ?: return
        closeGatt()
        deviceName = found[address]?.name ?: device.name ?: "MeshCore"
        prefs.edit { putString(LAST_DEVICE, address).putString(LAST_NAME, deviceName) }
        state = AppModel.RadioState.Connecting(deviceName)
        retriedAfterBond = false
        gatt = device.connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE)
    }

    /** Disconnects and forgets the radio (no reconnection). */
    fun disconnect() {
        prefs.edit { remove(LAST_DEVICE) }
        closeGatt()
        state = AppModel.RadioState.Idle
    }

    private fun reconnectLast() {
        val address = prefs.getString(LAST_DEVICE, null) ?: return
        if (!permitted() || gatt != null) return
        found.putIfAbsent(address, Found(address, prefs.getString(LAST_NAME, null) ?: "MeshCore", 0))
        connect(address)
    }

    private fun closeGatt() {
        ready = false
        rx = null
        queue.clear()
        writing = false
        gatt?.let {
            if (permitted()) {
                it.disconnect()
                it.close()
            }
        }
        gatt = null
    }

    /** Writes one companion frame; queued until the link is ready, then one at a time. */
    fun write(frame: ByteArray) {
        queue.addLast(frame)
        pumpWrites()
    }

    private fun pumpWrites() {
        val g = gatt ?: return
        val c = rx ?: return
        if (!ready || writing) return
        val frame = queue.removeFirstOrNull() ?: return
        writing = true
        val ok = if (Build.VERSION.SDK_INT >= 33) {
            g.writeCharacteristic(c, frame, BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothStatusCodes.SUCCESS
        } else {
            @Suppress("DEPRECATION")
            c.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            @Suppress("DEPRECATION")
            c.value = frame
            @Suppress("DEPRECATION")
            g.writeCharacteristic(c)
        }
        if (!ok) {
            // Busy: try again shortly, keeping the order.
            writing = false
            queue.addFirst(frame)
            main.postDelayed({ pumpWrites() }, 50)
        }
    }

    private fun enableNotifications(g: BluetoothGatt) {
        val tx = g.getService(service)?.getCharacteristic(txUuid) ?: return
        g.setCharacteristicNotification(tx, true)
        val cccd = tx.getDescriptor(CCCD) ?: return
        val value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
        if (Build.VERSION.SDK_INT >= 33) {
            g.writeDescriptor(cccd, value)
        } else {
            @Suppress("DEPRECATION")
            cccd.value = value
            @Suppress("DEPRECATION")
            g.writeDescriptor(cccd)
        }
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            main.post {
                if (g != gatt) return@post
                when (newState) {
                    // One frame per write: ask for room for 176 bytes (Android starts at 23).
                    BluetoothProfile.STATE_CONNECTED -> g.requestMtu(MTU)
                    BluetoothProfile.STATE_DISCONNECTED -> {
                        ready = false
                        rx = null
                        writing = false
                        g.close()
                        gatt = null
                        state = AppModel.RadioState.Idle
                        // Stay connected in the background (ADR-0006): reconnect when the radio
                        // comes back, unless the user disconnected.
                        val address = prefs.getString(LAST_DEVICE, null)
                        if (address != null && permitted()) {
                            state = AppModel.RadioState.Connecting(deviceName)
                            gatt = adapter?.getRemoteDevice(address)?.connectGatt(context, true, this, BluetoothDevice.TRANSPORT_LE)
                        }
                    }
                }
            }
        }

        override fun onMtuChanged(g: BluetoothGatt, mtu: Int, status: Int) {
            main.post { if (g == gatt) g.discoverServices() }
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
            main.post {
                if (g != gatt) return@post
                val s = g.getService(service) ?: return@post
                rx = s.getCharacteristic(rxUuid)
                // Only over a bonded, so encrypted, link (review of 2026-11-15, SR-32): an
                // unpaired link would let anyone nearby read and forge plain MeshCore chats and
                // contacts, and a look-alike radio pose as ours. Stock MeshCore asks for its code;
                // the bond receiver goes on once paired.
                when (g.device.bondState) {
                    BluetoothDevice.BOND_BONDED -> enableNotifications(g)
                    BluetoothDevice.BOND_NONE -> {
                        retriedAfterBond = true
                        g.device.createBond()
                    }
                    else -> {}
                }
            }
        }

        override fun onDescriptorWrite(g: BluetoothGatt, d: BluetoothGattDescriptor, status: Int) {
            main.post {
                if (g != gatt) return@post
                if (status == BluetoothGatt.GATT_SUCCESS && g.device.bondState == BluetoothDevice.BOND_BONDED) {
                    ready = true
                    state = AppModel.RadioState.Connected(deviceName)
                    pumpWrites()
                } else if (!retriedAfterBond && g.device.bondState == BluetoothDevice.BOND_NONE) {
                    // Encrypted characteristic: pair (Android asks for the radio's code), then the
                    // bond receiver enables the notifications again.
                    retriedAfterBond = true
                    g.device.createBond()
                }
            }
        }

        override fun onCharacteristicWrite(g: BluetoothGatt, c: BluetoothGattCharacteristic, status: Int) {
            main.post {
                writing = false
                pumpWrites()
            }
        }

        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray) {
            if (c.uuid == txUuid && value.isNotEmpty()) {
                val frame = value.copyOf()
                main.post { onFrame(frame) }
            }
        }

        @Deprecated("Android 12 delivers the value in the characteristic")
        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic) {
            if (Build.VERSION.SDK_INT >= 33) return
            @Suppress("DEPRECATION")
            val value = c.value ?: return
            onCharacteristicChanged(g, c, value)
        }
    }

    companion object {
        private const val LAST_DEVICE = "lichen.lastDevice"
        private const val LAST_NAME = "lichen.lastDeviceName"
        private const val MTU = 247
        private val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }
}
