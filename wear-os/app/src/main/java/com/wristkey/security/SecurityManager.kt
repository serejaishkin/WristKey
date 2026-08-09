package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature

class SecurityManager {
    companion object {
        private const val TAG = "WristKeySecurity"
        private const val KEY_ALIAS = "wristkey_auth_key"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply {
        load(null)
    }

    init {
        generateKeyPairIfNeeded()
    }

    /** Sign data with hardware-backed ECDSA P-256 key. */
    fun sign(data: ByteArray): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.PrivateKeyEntry
            ?: throw IllegalStateException("Key not found")
        val signature = Signature.getInstance("SHA256withECDSA")
        signature.initSign(entry.privateKey)
        signature.update(data)
        return signature.sign()
    }

    /** Return X.509 encoded public key for pairing */
    fun getPublicKey(): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.PrivateKeyEntry
            ?: throw IllegalStateException("Key not found")
        return entry.certificate.publicKey.encoded
    }

    /** Delete the auth key pair to force re-pairing. */
    fun resetKeys() {
        try {
            if (keyStore.containsAlias(KEY_ALIAS)) {
                keyStore.deleteEntry(KEY_ALIAS)
                Log.i(TAG, "Deleted key alias: $KEY_ALIAS")
            } else {
                Log.i(TAG, "No key to delete")
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to delete key", e)
        }
    }

    private fun generateKeyPairIfNeeded() {
        if (keyStore.containsAlias(KEY_ALIAS)) {
            Log.i(TAG, "Key pair already exists")
            return
        }

        val keyPairGenerator = KeyPairGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_EC,
            ANDROID_KEYSTORE
        )

        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY
        )
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setUserAuthenticationRequired(false)
            .setInvalidatedByBiometricEnrollment(false)
            .build()

        keyPairGenerator.initialize(spec)
        val keyPair = keyPairGenerator.generateKeyPair()
        Log.i(TAG, "Generated new ECDSA P-256 key pair: ${keyPair.public.format}")
    }
}
