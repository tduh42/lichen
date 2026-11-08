package com.tduhamel.lichen.model

import com.tduhamel.lichen.core.ChatRef

/** Lower-case hexadecimal. */
fun ByteArray.hex(): String = joinToString("") { "%02x".format(it) }

fun String.unhex(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()

/**
 * A conversation, usable as a map key: the bindings' [ChatRef] holds byte arrays, which Kotlin
 * compares by reference.
 */
data class ChatId(val group: Boolean, val hex: String) {
    val ref: ChatRef get() = if (group) ChatRef.Group(hex.unhex()) else ChatRef.Contact(hex.unhex())

    /** The contact's node key, for a direct conversation. */
    val contactKey: ByteArray? get() = if (group) null else hex.unhex()

    /** Navigation route argument. */
    val route: String get() = (if (group) "g" else "c") + hex

    companion object {
        fun of(ref: ChatRef): ChatId = when (ref) {
            is ChatRef.Contact -> ChatId(false, ref.key.hex())
            is ChatRef.Group -> ChatId(true, ref.gid.hex())
        }

        fun contact(key: ByteArray) = ChatId(false, key.hex())
        fun group(gid: ByteArray) = ChatId(true, gid.hex())

        fun fromRoute(s: String): ChatId? = when {
            s.startsWith("c") -> ChatId(false, s.drop(1))
            s.startsWith("g") -> ChatId(true, s.drop(1))
            else -> null
        }
    }
}
