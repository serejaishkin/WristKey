package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature

class KeyStoreManager {
    companion object {
        private const val TAG = "KeyStoreManager"
        private const val KEY_ALIAS = "WristKeyECDSA"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    }

    init {
        generateKeyPairIfNeeded()
    }

    private fun generateKeyPairIfNeeded() {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        if (!keyStore.containsAlias(KEY_ALIAS)) {
            val keyPairGenerator = KeyPairGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_EC,
                ANDROID_KEYSTORE
            )
            val params = KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY
            )
                .setAlgorithmParameterSpec(java.security.spec.ECGenParameterSpec("secp256r1"))
                .setDigests(KeyProperties.DIGEST_SHA256)
                .setUserAuthenticationRequired(false)
                .build()
            keyPairGenerator.initialize(params)
            keyPairGenerator.generateKeyPair()
            Log.i(TAG, "ECDSA P-256 key pair generated in AndroidKeyStore")
        }
    }

    fun signChallenge(challenge: ByteArray?): ByteArray {
        if (challenge == null) throw IllegalArgumentException("challenge is null")
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val signature = Signature.getInstance("SHA256withECDSA").apply {
            initSign(entry.privateKey)
            update(challenge)
        }
        val sig = signature.sign()
        Log.i(TAG, "Challenge signed, sig length: ${sig.size}")
        return sig
    }

    fun getPublicKey(): ByteArray {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        return entry.certificate.publicKey.encoded
    }
}
