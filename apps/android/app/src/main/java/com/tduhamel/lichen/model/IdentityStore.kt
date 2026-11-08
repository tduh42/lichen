package com.tduhamel.lichen.model

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** What reading the identity seed found. Only [Absent] may lead to a new identity. */
sealed interface IdentityRead {
    class Found(val seed: ByteArray) : IdentityRead

    /** Nothing stored: a first launch. */
    data object Absent : IdentityRead

    /** The Keystore cannot be used now: nothing is changed, try again. */
    class Unavailable(val error: Throwable) : IdentityRead

    /** Something is stored but cannot be an identity: not decryptable (its key is gone, or the
     * data was damaged) or not 32 bytes. */
    data object Corrupt : IdentityRead
}

/** The identity could not be saved (or did not read back the same). */
class IdentityWriteException(message: String, cause: Throwable? = null) : Exception(message, cause)

/**
 * The phone identity's seed (ADR-0013): 32 random bytes, kept encrypted under an AES key of the
 * Android Keystore that never leaves it. Device only, like the iOS Keychain item: the app is
 * excluded from backups and transfers (res/xml/data_extraction_rules.xml); the recovery phrase
 * moves an identity.
 *
 * A seed that cannot be read is never replaced on its own: [read] tells absent, unavailable and
 * corrupt apart, [create] only follows [IdentityRead.Absent], and every write is synchronous and
 * read back.
 */
class IdentityStore(context: Context) {
    private val prefs = context.getSharedPreferences("lichen.identity", Context.MODE_PRIVATE)

    fun read(): IdentityRead {
        val stored = prefs.getString(SEED, null) ?: return IdentityRead.Absent
        val sealed = runCatching { Base64.decode(stored, Base64.NO_WRAP) }.getOrNull()
        if (sealed == null || sealed.size <= IV_LEN) return IdentityRead.Corrupt
        val key = try {
            existingKey() ?: return IdentityRead.Corrupt
        } catch (e: Exception) {
            return IdentityRead.Unavailable(e)
        }
        val seed = try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, sealed, 0, IV_LEN))
            cipher.doFinal(sealed, IV_LEN, sealed.size - IV_LEN)
        } catch (_: AEADBadTagException) {
            return IdentityRead.Corrupt
        } catch (e: Exception) {
            return IdentityRead.Unavailable(e)
        }
        return if (seed.size == 32) IdentityRead.Found(seed) else IdentityRead.Corrupt
    }

    /** Draws and saves a new seed: only after [read] found none. */
    fun create(): ByteArray {
        if (read() != IdentityRead.Absent) throw IdentityWriteException("an identity is already stored")
        val seed = randomBytes(32)
        write(seed)
        return seed
    }

    /** Replaces the seed: an identity restored from its phrase, or a new one the user chose. */
    fun replace(seed: ByteArray) = write(seed)

    private fun write(seed: ByteArray) {
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: newKey())
            cipher.iv + cipher.doFinal(seed)
        } catch (e: Exception) {
            throw IdentityWriteException("encryption failed", e)
        }
        // commit(), not apply(): the identity is on disk before the app relies on it.
        val saved = prefs.edit().putString(SEED, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
        if (!saved) throw IdentityWriteException("not written")
        val back = read()
        if (back !is IdentityRead.Found || !back.seed.contentEquals(seed)) {
            throw IdentityWriteException("read back differs")
        }
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

    companion object {
        private const val KEYSTORE = "AndroidKeyStore"
        private const val ALIAS = "lichen-identity"
        private const val SEED = "seed"
        private const val TRANSFORM = "AES/GCM/NoPadding"
        private const val IV_LEN = 12

        fun randomBytes(n: Int): ByteArray = ByteArray(n).also { SecureRandom().nextBytes(it) }
    }
}
