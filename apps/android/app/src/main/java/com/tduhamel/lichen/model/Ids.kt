package com.tduhamel.lichen.model

import com.tduhamel.lichen.core.ChatRef

/** Lower-case hexadecimal. */
fun ByteArray.hex(): String = joinToString("") { "%02x".format(it) }

/** True when this is `bytes` bytes in lower- or upper-case hexadecimal. */
fun String.isHex(bytes: Int): Boolean =
    length == 2 * bytes && all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }

fun String.unhex(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()

/**
 * A conversation, usable as a map key: the bindings' [ChatRef] holds byte arrays, which Kotlin
 * compares by reference.
 */
data class ChatId(val kind: Kind, val hex: String) {
    /** A lichen contact or group, or a plain MeshCore chat (protocol.md §15), with the length
     * of its id in bytes: node keys 32, group ids 16, channel ids 8. */
    enum class Kind(val prefix: String, val bytes: Int) {
        CONTACT("c", 32), GROUP("g", 16), MESH("m", 32), CHANNEL("h", 8),

        /** A room server's posts (protocol.md §15.8). */
        ROOM("r", 32),
    }

    val group: Boolean get() = kind == Kind.GROUP

    /** A plain MeshCore chat (§15): text only, not lichen-encrypted. */
    val plain: Boolean get() = kind == Kind.MESH || kind == Kind.CHANNEL || kind == Kind.ROOM

    val channel: Boolean get() = kind == Kind.CHANNEL

    val ref: ChatRef get() = when (kind) {
        Kind.CONTACT -> ChatRef.Contact(hex.unhex())
        Kind.GROUP -> ChatRef.Group(hex.unhex())
        Kind.MESH -> ChatRef.Mesh(hex.unhex())
        Kind.CHANNEL -> ChatRef.Channel(hex.unhex())
        Kind.ROOM -> ChatRef.Room(hex.unhex())
    }

    /** The contact's node key, for a lichen direct conversation. */
    val contactKey: ByteArray? get() = if (kind == Kind.CONTACT) hex.unhex() else null

    /** The other node of a direct conversation, lichen or MeshCore. */
    val nodeKey: ByteArray? get() = if (kind == Kind.CONTACT || kind == Kind.MESH) hex.unhex() else null

    /** Navigation route argument. */
    val route: String get() = kind.prefix + hex

    companion object {
        fun of(ref: ChatRef): ChatId = when (ref) {
            is ChatRef.Contact -> ChatId(Kind.CONTACT, ref.key.hex())
            is ChatRef.Group -> ChatId(Kind.GROUP, ref.gid.hex())
            is ChatRef.Mesh -> ChatId(Kind.MESH, ref.key.hex())
            is ChatRef.Channel -> ChatId(Kind.CHANNEL, ref.id.hex())
            is ChatRef.Room -> ChatId(Kind.ROOM, ref.key.hex())
        }

        fun contact(key: ByteArray) = ChatId(Kind.CONTACT, key.hex())
        fun group(gid: ByteArray) = ChatId(Kind.GROUP, gid.hex())
        fun mesh(key: ByteArray) = ChatId(Kind.MESH, key.hex())
        fun channel(id: ByteArray) = ChatId(Kind.CHANNEL, id.hex())
        fun room(key: ByteArray) = ChatId(Kind.ROOM, key.hex())

        /** The conversation of a route or a notification's extra; null for anything else. The
         * activity is exported: another app can send any string here (review of 2026-11-15,
         * SR-25). */
        fun fromRoute(s: String): ChatId? {
            val kind = Kind.entries.firstOrNull { s.startsWith(it.prefix) } ?: return null
            val hex = s.drop(1)
            return if (hex.isHex(kind.bytes)) ChatId(kind, hex.lowercase()) else null
        }
    }
}
