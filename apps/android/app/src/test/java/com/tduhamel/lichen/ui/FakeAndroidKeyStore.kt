package com.tduhamel.lichen.ui

import java.io.InputStream
import java.io.OutputStream
import java.security.Key
import java.security.KeyStoreSpi
import java.security.Provider
import java.security.SecureRandom
import java.security.Security
import java.security.cert.Certificate
import java.security.spec.AlgorithmParameterSpec
import java.util.Collections
import java.util.Date
import java.util.Enumeration
import java.util.concurrent.ConcurrentHashMap
import javax.crypto.KeyGeneratorSpi
import javax.crypto.SecretKey
import javax.crypto.spec.SecretKeySpec

/**
 * The Android Keystore for Robolectric, which has none: AES keys made with a
 * `KeyGenParameterSpec` and kept by alias, in memory, for [com.tduhamel.lichen.model.IdentityStore]
 * and [com.tduhamel.lichen.model.DatabaseKey]. Empty at each [install]: a test starts on a phone
 * with no identity, as its files and preferences do.
 */
object FakeAndroidKeyStore {
    private const val NAME = "AndroidKeyStore"
    private val keys = ConcurrentHashMap<String, SecretKey>()

    /** A new phone: no keys. */
    fun clear() = keys.clear()

    fun install() {
        keys.clear()
        // A provider left by an earlier sandbox holds that sandbox's classes: replaced.
        Security.removeProvider(NAME)
        Security.insertProviderAt(KeystoreProvider(), 1)
    }

    class KeystoreProvider : Provider(NAME, 1.0, "In-memory Android Keystore for JVM tests") {
        init {
            put("KeyStore.$NAME", Store::class.java.name)
            put("KeyGenerator.AES", Generator::class.java.name)
        }
    }

    class Store : KeyStoreSpi() {
        override fun engineGetKey(alias: String, password: CharArray?): Key? = keys[alias]
        override fun engineGetCertificateChain(alias: String): Array<Certificate>? = null
        override fun engineGetCertificate(alias: String): Certificate? = null
        override fun engineGetCreationDate(alias: String): Date? = if (keys.containsKey(alias)) Date(0) else null
        override fun engineSetKeyEntry(alias: String, key: Key, password: CharArray?, chain: Array<out Certificate>?) {
            keys[alias] = key as SecretKey
        }
        override fun engineSetKeyEntry(alias: String, key: ByteArray, chain: Array<out Certificate>?) =
            throw UnsupportedOperationException()
        override fun engineSetCertificateEntry(alias: String, cert: Certificate) = throw UnsupportedOperationException()
        override fun engineDeleteEntry(alias: String) {
            keys.remove(alias)
        }
        override fun engineAliases(): Enumeration<String> = Collections.enumeration(keys.keys.toList())
        override fun engineContainsAlias(alias: String): Boolean = keys.containsKey(alias)
        override fun engineSize(): Int = keys.size
        override fun engineIsKeyEntry(alias: String): Boolean = keys.containsKey(alias)
        override fun engineIsCertificateEntry(alias: String): Boolean = false
        override fun engineGetCertificateAlias(cert: Certificate): String? = null
        override fun engineStore(stream: OutputStream?, password: CharArray?) = Unit
        override fun engineLoad(stream: InputStream?, password: CharArray?) = Unit
    }

    /** AES keys from a `KeyGenParameterSpec`, read by reflection: the spec's class belongs to
     * the Robolectric sandbox, this provider may not. */
    class Generator : KeyGeneratorSpi() {
        private var alias: String? = null
        private var bits = 256
        private var random = SecureRandom()

        override fun engineInit(random: SecureRandom?) {
            random?.let { this.random = it }
        }

        override fun engineInit(params: AlgorithmParameterSpec, random: SecureRandom?) {
            alias = params.javaClass.getMethod("getKeystoreAlias").invoke(params) as String
            bits = (params.javaClass.getMethod("getKeySize").invoke(params) as Int).takeIf { it > 0 } ?: 256
            random?.let { this.random = it }
        }

        override fun engineInit(keysize: Int, random: SecureRandom?) {
            bits = keysize
            random?.let { this.random = it }
        }

        override fun engineGenerateKey(): SecretKey {
            val key = SecretKeySpec(ByteArray(bits / 8).also { random.nextBytes(it) }, "AES")
            alias?.let { keys[it] = key }
            return key
        }
    }
}
