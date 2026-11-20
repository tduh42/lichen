package com.tduhamel.lichen.ui

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.File

/**
 * The phone's gallery (MediaStore) for UI tests, which Robolectric does not have: what the app
 * inserts, the values it gives, and the bytes it writes, in memory and a temporary folder.
 */
class FakeGallery : ContentProvider() {
    class Item(val values: ContentValues, val file: File)

    override fun onCreate() = true

    override fun insert(uri: Uri, values: ContentValues?): Uri {
        val id = items.size + 1L
        val file = File.createTempFile("gallery-$id", ".bin").apply { deleteOnExit() }
        items += Item(ContentValues(values), file)
        return Uri.withAppendedPath(uri, "$id")
    }

    override fun update(uri: Uri, values: ContentValues?, selection: String?, args: Array<out String>?): Int {
        val item = item(uri) ?: return 0
        values?.let { item.values.putAll(it) }
        return 1
    }

    override fun delete(uri: Uri, selection: String?, args: Array<out String>?): Int {
        val item = item(uri) ?: return 0
        items.remove(item)
        return 1
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val item = item(uri) ?: return null
        return ParcelFileDescriptor.open(item.file, ParcelFileDescriptor.parseMode(mode))
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, args: Array<out String>?, sort: String?): Cursor? = null

    override fun getType(uri: Uri): String? = item(uri)?.values?.getAsString("mime_type")

    private fun item(uri: Uri): Item? = uri.lastPathSegment?.toIntOrNull()?.let { items.getOrNull(it - 1) }

    companion object {
        val items = mutableListOf<Item>()
    }
}
