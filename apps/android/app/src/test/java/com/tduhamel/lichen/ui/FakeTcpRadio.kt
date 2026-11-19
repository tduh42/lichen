package com.tduhamel.lichen.ui

import com.tduhamel.lichen.core.DemoNetwork
import java.io.DataInputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket

/**
 * A radio on the network, as `lichen bridge` serves one: the demo network's companion behind a
 * TCP port on this machine, with the serial framing (`'<' len frame` from the app, `'>' len
 * frame` to it). The app reaches it through its real TCP link, as a radio it pairs with and
 * saves (not the demo mode). The simulation runs on the wall clock.
 */
class FakeTcpRadio : AutoCloseable {
    private val server = ServerSocket(0, 1, InetAddress.getLoopbackAddress())
    private val demo = DemoNetwork()
    private val start = System.nanoTime()
    @Volatile private var client: Socket? = null

    /** "127.0.0.1:port", as typed on the pairing screen. */
    val address: String = "127.0.0.1:${server.localPort}"

    init {
        Thread(::serve, "fake-tcp-radio").apply { isDaemon = true }.start()
    }

    private fun now(): ULong = ((System.nanoTime() - start) / 1_000_000).toULong()

    private fun serve() {
        while (!server.isClosed) {
            val s = runCatching { server.accept() }.getOrNull() ?: return
            client = s
            val out = s.getOutputStream()
            Thread({
                while (!s.isClosed) {
                    runCatching { send(out, demo.read(now())) }.onFailure { return@Thread }
                    Thread.sleep(40)
                }
            }, "fake-tcp-radio-out").apply { isDaemon = true }.start()
            val input = DataInputStream(s.getInputStream())
            try {
                while (true) {
                    val b = input.read()
                    if (b < 0) break
                    if (b != '<'.code) continue
                    val len = input.read() or (input.read() shl 8)
                    val frame = ByteArray(len)
                    input.readFully(frame)
                    demo.write(frame, now())
                    send(out, demo.read(now()))
                }
            } catch (_: Exception) {
                // Closed: the link dropped.
            }
            runCatching { s.close() }
        }
    }

    private fun send(out: OutputStream, frames: List<ByteArray>) = synchronized(out) {
        for (f in frames) {
            out.write(byteArrayOf('>'.code.toByte(), (f.size and 0xFF).toByte(), (f.size shr 8).toByte()) + f)
        }
        out.flush()
    }

    /** The link drops (the radio switched off, Wi-Fi lost); the next connection is served. */
    fun drop() {
        runCatching { client?.close() }
    }

    override fun close() {
        runCatching { server.close() }
        drop()
        demo.close()
    }
}
