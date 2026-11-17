package com.tduhamel.lichen.model

import android.os.Handler
import android.os.Looper
import com.tduhamel.lichen.core.TcpAddress
import com.tduhamel.lichen.core.UsbDecoder
import com.tduhamel.lichen.core.linkRetryDelayMs
import com.tduhamel.lichen.core.tcpAddress
import com.tduhamel.lichen.core.usbEncode
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors

/**
 * The MeshCore companion over TCP, with the serial interface's framing (`'<' len frame` out,
 * `'>' len frame` in): a companion on Wi-Fi, or a USB radio relayed by `lichen bridge` on a
 * computer (the Android emulator reaches its host at 10.0.2.2). The link is neither encrypted
 * nor authenticated (review of 2026-11-15, SR-31): the pairing screen says so.
 *
 * A link that was up and drops is tried again after 1, 2, 4, 8, 16 s, then every 30 s
 * ([linkRetryDelayMs], the same as on iOS), until [close]; a first attempt that fails is
 * reported through [onFailure] instead.
 */
class TcpCompanion(
    private val onState: (AppModel.RadioState) -> Unit,
    private val onFrame: (ByteArray) -> Unit,
    /** A first attempt failed: why, in the system's words. */
    private val onFailure: (String) -> Unit = {},
    /** A line for the problem report's log (no address in it). */
    private val onLog: (String) -> Unit = {},
) {
    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor()
    @Volatile private var socket: Socket? = null

    /** Each attempt's number: the thread of an older one leaves the state alone. */
    private var generation = 0
    private var attempt = 0u
    private var retry: Runnable? = null

    /** The link was up once since the user asked for it: only then is a failure retried. */
    private var wasUp = false

    /** "host:port" while the link is wanted: connected, connecting, or waiting to retry. */
    var target: String? = null
        private set

    /** "host:port" while connected. */
    var connectedTo: String? = null
        private set

    /**
     * Connects to [address] and keeps the link until [close]; false when it is not an address.
     * [retrying]: the link was in use when the app stopped, so failures are retried at once.
     */
    fun connect(address: String, retrying: Boolean = false): Boolean {
        val a = tcpAddress(address) ?: return false
        stop()
        target = address.trim()
        connectedTo = null
        wasUp = retrying
        attempt = 0u
        open(a)
        return true
    }

    /** The app came back to the foreground: a link waiting to retry tries at once. */
    fun retryNow() {
        val r = retry ?: return
        main.removeCallbacks(r)
        r.run()
    }

    /** Writes one companion frame. */
    fun write(frame: ByteArray) {
        val s = socket ?: return
        val wire = runCatching { usbEncode(frame) }.getOrNull() ?: return
        io.execute { runCatching { s.getOutputStream().write(wire) } }
    }

    /** Closes the link and forgets it. */
    fun close() {
        if (target == null) return
        stop()
        target = null
        connectedTo = null
        onLog("TCP: disconnected by the user")
        onState(AppModel.RadioState.Idle)
    }

    private fun stop() {
        generation++
        retry?.let { main.removeCallbacks(it) }
        retry = null
        socket?.let { s -> runCatching { s.close() } }
        socket = null
    }

    private fun open(a: TcpAddress) {
        val gen = ++generation
        val label = target ?: return
        onState(AppModel.RadioState.Connecting(label))
        Thread({
            val s = Socket()
            try {
                s.connect(InetSocketAddress(a.host, a.port.toInt()), 5_000)
                s.tcpNoDelay = true
                s.keepAlive = true
            } catch (e: Exception) {
                runCatching { s.close() }
                main.post { if (gen == generation) dropped(e.message ?: e.javaClass.simpleName) }
                return@Thread
            }
            main.post {
                if (gen != generation) {
                    runCatching { s.close() }
                    return@post
                }
                socket = s
                connectedTo = label
                attempt = 0u
                wasUp = true
                onLog("TCP: connected")
                onState(AppModel.RadioState.Connected(label))
            }
            val decoder = UsbDecoder()
            val buf = ByteArray(1024)
            try {
                val input = s.getInputStream()
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    if (n > 0) for (f in decoder.push(buf.copyOf(n))) main.post { if (gen == generation) onFrame(f) }
                }
            } catch (_: Exception) {
                // Closed or lost.
            }
            decoder.close()
            main.post { if (gen == generation) dropped(null) }
        }, "lichen-tcp").start()
    }

    /** The connection failed or was lost: retried when the link was up once, else reported. */
    private fun dropped(why: String?) {
        socket?.let { s -> runCatching { s.close() } }
        socket = null
        connectedTo = null
        val t = target ?: return
        val a = tcpAddress(t) ?: return
        if (wasUp) {
            attempt++
            val delay = linkRetryDelayMs(attempt)
            onLog("TCP: link lost, attempt $attempt in ${delay / 1000u} s")
            onState(AppModel.RadioState.Connecting(t))
            val r = Runnable {
                retry = null
                if (target == t) open(a)
            }
            retry = r
            main.postDelayed(r, delay.toLong())
        } else {
            target = null
            onLog("TCP: could not connect")
            onState(AppModel.RadioState.Idle)
            onFailure(why ?: "closed")
        }
    }
}
