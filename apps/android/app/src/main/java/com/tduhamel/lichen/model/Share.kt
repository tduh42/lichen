package com.tduhamel.lichen.model

import android.content.ContentResolver
import android.content.Intent
import android.net.Uri
import androidx.core.content.IntentCompat

/**
 * Something another app shared into lichen (ADR-0034): a text, and photos or files by their
 * content URIs, not read yet. Built only from what [from] accepts.
 */
class IncomingShare(val text: String?, val uris: List<Uri>) {
    val hasFiles: Boolean get() = uris.isNotEmpty()

    companion object {
        /** At most this many files in one share. */
        const val FILES_MAX = 10

        /** A shared text longer than this is cut: the core refuses what does not fit a message
         * anyway. */
        const val TEXT_MAX = 4_096

        /**
         * The share an intent carries, checked as another app's data (review of 2026-11-15,
         * SR-25): only `ACTION_SEND` and `ACTION_SEND_MULTIPLE`; a text of [TEXT_MAX]
         * characters at most; only `content:` URIs, never a `file:` path nor one of lichen's
         * own providers (another app could otherwise make lichen send its own files);
         * [FILES_MAX] at most. Null when nothing usable is left.
         */
        fun from(intent: Intent, ownPackage: String): IncomingShare? = runCatching {
            val streams: List<Uri> = when (intent.action) {
                Intent.ACTION_SEND ->
                    listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
                Intent.ACTION_SEND_MULTIPLE ->
                    IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
                else -> return null
            }
            val uris = streams.filter { acceptable(it, ownPackage) }.distinct().take(FILES_MAX)
            val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()?.trim()?.take(TEXT_MAX)?.takeIf { it.isNotEmpty() }
            if (text == null && uris.isEmpty()) null else IncomingShare(text, uris)
        }.getOrNull()

        private fun acceptable(uri: Uri, ownPackage: String): Boolean = acceptable(uri.scheme, uri.authority, ownPackage)

        /** A content URI from another app's provider: its scheme and authority. */
        fun acceptable(scheme: String?, authority: String?, ownPackage: String): Boolean {
            if (scheme != ContentResolver.SCHEME_CONTENT || authority.isNullOrEmpty()) return false
            return authority != ownPackage && !authority.startsWith("$ownPackage.")
        }
    }
}
