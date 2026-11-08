package com.tduhamel.lichen.model

import android.os.Handler
import android.os.Looper
import com.tduhamel.lichen.core.UsbDecoder
import com.tduhamel.lichen.core.usbEncode
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors

/**
 * The MeshCore companion over TCP, with the serial interface's framing (`'<' len frame` out,
 * `'>' len frame` in): a companion on Wi-Fi, or a USB radio relayed by `lichen bridge` on a
 * computer (the Android emulator reaches its host at 10.0.2.2).
 */
class TcpCompanion(
    private val onState: (AppModel.RadioState) -> Unit,
    private val onFrame: (ByteArray) -> Unit,
) {
    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor()
    @Volatile private var socket: Socket? = null

    /** "host:port" while connected. */
    var connectedTo: String? = null
        private set

    fun connect(address: String) {
        val host = address.substringBeforeLast(':').trim()
        val port = address.substringAfterLast(':', "5000").trim().toIntOrNull() ?: 5000
        close()
        onState(AppModel.RadioState.Connecting(address))
        Thread({
            val s = Socket()
            try {
                s.connect(InetSocketAddress(host, port), 5_000)
                s.tcpNoDelay = true
            } catch (_: Exception) {
                runCatching { s.close() }
                main.post { onState(AppModel.RadioState.Idle) }
                return@Thread
            }
            socket = s
            main.post {
                connectedTo = address
                onState(AppModel.RadioState.Connected(address))
            }
            val decoder = UsbDecoder()
            val buf = ByteArray(1024)
            try {
                val input = s.getInputStream()
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    if (n > 0) for (f in decoder.push(buf.copyOf(n))) main.post { onFrame(f) }
                }
            } catch (_: Exception) {
                // Closed or lost.
            }
            decoder.close()
            main.post {
                if (socket === s) {
                    socket = null
                    connectedTo = null
                    onState(AppModel.RadioState.Idle)
                }
            }
        }, "lichen-tcp").start()
    }

    /** Writes one companion frame. */
    fun write(frame: ByteArray) {
        val s = socket ?: return
        val wire = runCatching { usbEncode(frame) }.getOrNull() ?: return
        io.execute { runCatching { s.getOutputStream().write(wire) } }
    }

    fun close() {
        val s = socket ?: return
        socket = null
        connectedTo = null
        runCatching { s.close() }
        onState(AppModel.RadioState.Idle)
    }
}
