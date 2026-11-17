package com.tduhamel.lichen.model

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The key of the message databases (SQLCipher, ADR-0036): 32 random bytes drawn once, kept in
 * the app's files sealed under an AES key of the Android Keystore that never leaves it. Like the
 * identity, device only: no backup, no transfer (res/xml/data_extraction_rules.xml).
 *
 * Used only when the core can encrypt (`databaseEncryption()`). [read] gives an empty key when
 * the Keystore cannot be used now: the core then opens no database rather than a clear one. A
 * sealed key that no longer opens (the Keystore was reset) is never replaced: the databases it
 * locks would be lost; the history comes back from a backup (ADR-0037).
 */
class DatabaseKey(context: Context) {
    private val file = File(context.noBackupFilesDir, "database.key")

    fun read(): ByteArray = runCatching {
        if (!file.exists()) return@runCatching create()
        val sealed = file.readBytes()
        if (sealed.size <= IV_LEN) return@runCatching ByteArray(0)
        val key = existingKey() ?: return@runCatching ByteArray(0)
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, sealed, 0, IV_LEN))
        cipher.doFinal(sealed, IV_LEN, sealed.size - IV_LEN).takeIf { it.size == 32 } ?: ByteArray(0)
    }.getOrDefault(ByteArray(0))

    /** A first key: sealed, written whole (a temporary file renamed into place), read back. */
    private fun create(): ByteArray {
        val key = IdentityStore.randomBytes(32)
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: newKey())
        val sealed = cipher.iv + cipher.doFinal(key)
        val tmp = File(file.parentFile, "database.key.tmp")
        tmp.outputStream().use {
            it.write(sealed)
            it.fd.sync()
        }
        if (!tmp.renameTo(file)) return ByteArray(0)
        return if (read().contentEquals(key)) key else ByteArray(0)
    }

    private fun existingKey(): SecretKey? {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        return ks.getKey(ALIAS, null) as? SecretKey
    }

    private fun newKey(): SecretKey {
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "lichen-database"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV_LEN = 12
    }
}
