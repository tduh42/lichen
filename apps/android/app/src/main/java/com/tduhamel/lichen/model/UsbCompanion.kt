package com.tduhamel.lichen.model

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat
import androidx.core.content.IntentCompat
import com.tduhamel.lichen.core.UsbDecoder
import com.tduhamel.lichen.core.usbEncode
import java.util.concurrent.Executors

/**
 * The MeshCore companion over USB OTG: a USB CDC-ACM serial port (the Heltec V4's native USB),
 * with the companion's serial framing (`'<' len frame` out, `'>' len frame` in, from the core).
 * A small driver of its own: no third-party USB library. Radios behind a vendor bridge chip
 * (the ThinkNode M5's CH340) are reached over Bluetooth instead.
 */
class UsbCompanion(
    private val context: Context,
    private val onState: (AppModel.RadioState) -> Unit,
    private val onFrame: (ByteArray) -> Unit,
    private val onDevices: (List<UsbDevice>) -> Unit,
) {
    private val manager = context.getSystemService(UsbManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private val writer = Executors.newSingleThreadExecutor()

    private var connection: UsbDeviceConnection? = null
    private var data: UsbInterface? = null
    private var control: UsbInterface? = null
    private var out: UsbEndpoint? = null
    private var reader: Thread? = null
    @Volatile private var open = false
    var device: UsbDevice? = null
        private set

    /** USB devices with a CDC-ACM serial port. */
    fun serialDevices(): List<UsbDevice> = manager?.deviceList?.values?.filter { cdc(it) != null }.orEmpty()

    private val permission = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            val d = IntentCompat.getParcelableExtra(i, UsbManager.EXTRA_DEVICE, UsbDevice::class.java) ?: return
            if (i.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) open(d)
        }
    }

    private val detached = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            val d = IntentCompat.getParcelableExtra(i, UsbManager.EXTRA_DEVICE, UsbDevice::class.java) ?: return
            if (d.deviceName == device?.deviceName) close()
            onDevices(serialDevices())
        }
    }

    private val attached = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            onDevices(serialDevices())
        }
    }

    init {
        ContextCompat.registerReceiver(context, permission, IntentFilter(ACTION_PERMISSION), ContextCompat.RECEIVER_NOT_EXPORTED)
        ContextCompat.registerReceiver(
            context, detached, IntentFilter(UsbManager.ACTION_USB_DEVICE_DETACHED), ContextCompat.RECEIVER_EXPORTED,
        )
        ContextCompat.registerReceiver(
            context, attached, IntentFilter(UsbManager.ACTION_USB_DEVICE_ATTACHED), ContextCompat.RECEIVER_EXPORTED,
        )
        main.post { onDevices(serialDevices()) }
    }

    /** Opens `d`, asking the user's permission first when needed. */
    fun connect(d: UsbDevice) {
        val m = manager ?: return
        if (m.hasPermission(d)) {
            open(d)
            return
        }
        val intent = Intent(ACTION_PERMISSION).setPackage(context.packageName)
        // Mutable: the system adds the device and the answer to it.
        m.requestPermission(d, PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE))
    }

    private fun open(d: UsbDevice) {
        val m = manager ?: return
        close()
        val (ctl, dat) = cdc(d) ?: return
        val conn = m.openDevice(d) ?: return
        if (!conn.claimInterface(dat, true) || (ctl != null && !conn.claimInterface(ctl, true))) {
            conn.close()
            return
        }
        val inEp = (0 until dat.endpointCount).map { dat.getEndpoint(it) }
            .firstOrNull { it.type == UsbConstants.USB_ENDPOINT_XFER_BULK && it.direction == UsbConstants.USB_DIR_IN }
        val outEp = (0 until dat.endpointCount).map { dat.getEndpoint(it) }
            .firstOrNull { it.type == UsbConstants.USB_ENDPOINT_XFER_BULK && it.direction == UsbConstants.USB_DIR_OUT }
        if (inEp == null || outEp == null) {
            conn.close()
            return
        }
        val iface = (ctl ?: dat).id
        // SET_LINE_CODING: 115200 baud, 8N1 (ignored by native USB, needed by some bridges).
        val coding = byteArrayOf(0x00, 0xC2.toByte(), 0x01, 0x00, 0, 0, 8)
        conn.controlTransfer(REQ_TYPE, SET_LINE_CODING, 0, iface, coding, coding.size, TIMEOUT)
        // SET_CONTROL_LINE_STATE: DTR only. RTS stays low: on an ESP32's own USB port, RTS high
        // with DTR low resets the chip.
        conn.controlTransfer(REQ_TYPE, SET_CONTROL_LINE_STATE, 0x01, iface, null, 0, TIMEOUT)
        connection = conn
        data = dat
        control = ctl
        out = outEp
        device = d
        open = true
        val name = d.productName ?: "USB"
        onState(AppModel.RadioState.Connected(name))
        reader = Thread({ readLoop(conn, inEp) }, "lichen-usb").apply { start() }
    }

    private fun readLoop(conn: UsbDeviceConnection, ep: UsbEndpoint) {
        val decoder = UsbDecoder()
        val buf = ByteArray(maxOf(ep.maxPacketSize, 512))
        while (open) {
            val n = conn.bulkTransfer(ep, buf, buf.size, 200)
            if (n > 0) {
                for (f in decoder.push(buf.copyOf(n))) main.post { onFrame(f) }
            }
        }
        decoder.close()
    }

    /** Writes one companion frame. */
    fun write(frame: ByteArray) {
        val conn = connection ?: return
        val ep = out ?: return
        val wire = runCatching { usbEncode(frame) }.getOrNull() ?: return
        writer.execute { if (open) conn.bulkTransfer(ep, wire, wire.size, TIMEOUT) }
    }

    fun close() {
        if (connection == null) return
        open = false
        reader?.join(500)
        reader = null
        connection?.let { c ->
            data?.let { c.releaseInterface(it) }
            control?.let { c.releaseInterface(it) }
            c.close()
        }
        connection = null
        data = null
        control = null
        out = null
        device = null
        onState(AppModel.RadioState.Idle)
    }

    companion object {
        private const val ACTION_PERMISSION = "com.tduhamel.lichen.USB_PERMISSION"
        private const val REQ_TYPE = 0x21 // class, interface, host to device
        private const val SET_LINE_CODING = 0x20
        private const val SET_CONTROL_LINE_STATE = 0x22
        private const val TIMEOUT = 1000

        /** The CDC control interface (if any) and the data interface (two bulk endpoints). */
        fun cdc(d: UsbDevice): Pair<UsbInterface?, UsbInterface>? {
            val ifaces = (0 until d.interfaceCount).map { d.getInterface(it) }
            val dat = ifaces.firstOrNull { it.interfaceClass == UsbConstants.USB_CLASS_CDC_DATA } ?: return null
            val ctl = ifaces.firstOrNull { it.interfaceClass == UsbConstants.USB_CLASS_COMM }
            return ctl to dat
        }
    }
}
