package com.tduhamel.lichen.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.produceState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * A value worked out off the main thread, as images are decoded (review of 2026-11-15,
 * SR-27): null until it is ready, worked out again only when `keys` change; the previous
 * value stays on screen meanwhile.
 */
@Composable
fun <T> rememberInBackground(vararg keys: Any?, work: () -> T?): T? =
    produceState<T?>(null, *keys) {
        value = withContext(Dispatchers.Default) { runCatching(work).getOrNull() }
    }.value
