package com.tduhamel.lichen.model

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** What reading the database key found (ADR-0036). */
sealed interface DatabaseKeyRead {
    class Found(val key: ByteArray) : DatabaseKeyRead

    /** None stored yet. */
    data object Absent : DatabaseKeyRead

    /** The Keystore cannot be used now: nothing is changed, try again. */
    data object Unavailable : DatabaseKeyRead

    /** Stored, but it no longer opens: its Keystore key is gone, or the file is damaged. */
    data object Lost : DatabaseKeyRead
}

/**
 * The key of the message databases (SQLCipher, ADR-0036): 32 random bytes drawn once, kept in
 * the app's no-backup files sealed with AES-GCM under a key of the Android Keystore that never
 * leaves it. Like the identity, device only: no backup, no transfer
 * (res/xml/data_extraction_rules.xml).
 *
 * [create] is only called when [read] found no usable key and no database waits for an older
 * one: a database already encrypted is never given a new key (the app says the key is lost).
 */
class DatabaseKey(context: Context) {
    private val file = File(context.noBackupFilesDir, "database.key")

    fun read(): DatabaseKeyRead {
        if (!file.exists()) return DatabaseKeyRead.Absent
        val sealed = runCatching { file.readBytes() }.getOrElse { return DatabaseKeyRead.Unavailable }
        if (sealed.size <= IV_LEN) return DatabaseKeyRead.Lost
        val keystoreKey = try {
            existingKey() ?: return DatabaseKeyRead.Lost
        } catch (_: Exception) {
            return DatabaseKeyRead.Unavailable
        }
        val key = try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, keystoreKey, GCMParameterSpec(128, sealed, 0, IV_LEN))
            cipher.doFinal(sealed, IV_LEN, sealed.size - IV_LEN)
        } catch (_: AEADBadTagException) {
            return DatabaseKeyRead.Lost
        } catch (_: Exception) {
            return DatabaseKeyRead.Unavailable
        }
        return if (key.size == 32) DatabaseKeyRead.Found(key) else DatabaseKeyRead.Lost
    }

    /** Draws a key, seals it, writes it whole (a temporary file renamed into place, replacing a
     * lost one) and reads it back. Null when the Keystore or the files cannot be used now. */
    fun create(): ByteArray? = runCatching {
        val key = IdentityStore.randomBytes(32)
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: newKey())
        val sealed = cipher.iv + cipher.doFinal(key)
        val tmp = File(file.parentFile, "database.key.tmp")
        tmp.outputStream().use {
            it.write(sealed)
            it.fd.sync()
        }
        check(tmp.renameTo(file)) { "not renamed" }
        val back = read()
        check(back is DatabaseKeyRead.Found && back.key.contentEquals(key)) { "read back differs" }
        key
    }.getOrNull()

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
