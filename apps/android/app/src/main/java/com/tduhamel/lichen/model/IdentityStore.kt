package com.tduhamel.lichen.model

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.core.content.edit
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The phone identity's seed (ADR-0013): 32 random bytes, kept encrypted under an AES key of the
 * Android Keystore that never leaves it. Device only, like the iOS Keychain item: the app is
 * excluded from backups and transfers (res/xml/data_extraction_rules.xml); the recovery phrase
 * moves an identity.
 */
class IdentityStore(context: Context) {
    private val prefs = context.getSharedPreferences("lichen.identity", Context.MODE_PRIVATE)

    /** The seed, made on first use. */
    fun seed(): ByteArray {
        load()?.let { return it }
        val seed = randomBytes(32)
        save(seed)
        return seed
    }

    /** Replaces the seed (an identity restored from its phrase). */
    fun save(seed: ByteArray) {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(seed)
        prefs.edit { putString(SEED, Base64.encodeToString(sealed, Base64.NO_WRAP)) }
    }

    private fun load(): ByteArray? {
        val sealed = prefs.getString(SEED, null)?.let { Base64.decode(it, Base64.NO_WRAP) } ?: return null
        if (sealed.size <= IV_LEN) return null
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed, 0, IV_LEN))
            cipher.doFinal(sealed, IV_LEN, sealed.size - IV_LEN)
        }.getOrNull()
    }

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
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

    companion object {
        private const val KEYSTORE = "AndroidKeyStore"
        private const val ALIAS = "lichen-identity"
        private const val SEED = "seed"
        private const val TRANSFORM = "AES/GCM/NoPadding"
        private const val IV_LEN = 12

        fun randomBytes(n: Int): ByteArray = ByteArray(n).also { SecureRandom().nextBytes(it) }
    }
}
