package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature

/**
 * Manages ECDSA P-256 keypair in Android Keystore.
 *
 * - Key generation on first pairing
 - Signing challenges (never exposes private key)
 */
class SecurityManager {

    companion object {
        const val KEY_ALIAS = "wristkey_auth_key"
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    /**
     * Generate new ECDSA P-256 keypair if not exists.
     * Returns public key raw bytes (32 bytes X + 32 bytes Y for uncompressed).
     */
    fun generateKeyPairIfNeeded(): ByteArray {
        if (!keyStore.containsAlias(KEY_ALIAS)) {
            val generator = KeyPairGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_EC,
                ANDROID_KEYSTORE
            )
            generator.initialize(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_SIGN
                )
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    .setUserAuthenticationRequired(false) // TODO: require screen lock for v2
                    .setInvalidatedByBiometricEnrollment(true)
                    .build()
            )
            generator.generateKeyPair()
            Log.i("WristKeySecurity", "ECDSA P-256 keypair generated")
        }

        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        return entry.certificate.publicKey.encoded
    }

    /**
     * Sign data with private key. Returns DER-encoded ECDSA signature.
     */
    fun sign(data: ByteArray): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val signer = Signature.getInstance("SHA256withECDSA")
        signer.initSign(entry.privateKey)
        signer.update(data)
        return signer.sign()
    }

    fun hasKeyPair(): Boolean = keyStore.containsAlias(KEY_ALIAS)

    fun deleteKeyPair() {
        keyStore.deleteEntry(KEY_ALIAS)
    }
}
