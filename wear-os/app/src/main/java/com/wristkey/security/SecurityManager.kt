package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey

class SecurityManager {
    companion object {
        private const val TAG = "WristKeySecurity"
        private const val KEY_ALIAS = "wristkey_auth_key"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    init { generateKeyPairIfNeeded() }

    fun sign(data: ByteArray): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.PrivateKeyEntry
            ?: throw IllegalStateException("Key not found")
        val signature = Signature.getInstance("SHA256withECDSA")
        signature.initSign(entry.privateKey)
        signature.update(data)
        return signature.sign()
    }

    fun getDeviceId(): ByteArray {
        val pubKey = getPublicKey()
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        return digest.digest(pubKey).copyOfRange(0, 4)
    }

    fun getPublicKey(): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.PrivateKeyEntry
            ?: throw IllegalStateException("Key not found")
        val ecPublicKey = entry.certificate.publicKey as ECPublicKey
        val x = padTo32(ecPublicKey.w.affineX.toByteArray())
        val y = padTo32(ecPublicKey.w.affineY.toByteArray())
        return byteArrayOf(0x04) + x + y
    }

    private fun padTo32(raw: ByteArray): ByteArray {
        val trimmed = if (raw.size == 33 && raw[0] == 0.toByte()) raw.copyOfRange(1, 33) else raw
        return if (trimmed.size < 32) ByteArray(32 - trimmed.size) + trimmed else trimmed
    }

    fun resetKeys() {
        try {
            if (keyStore.containsAlias(KEY_ALIAS)) {
                keyStore.deleteEntry(KEY_ALIAS)
                Log.i(TAG, "Deleted key alias: $KEY_ALIAS")
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to delete key", e)
        }
        generateKeyPairIfNeeded()
    }

    private fun generateKeyPairIfNeeded() {
        if (keyStore.containsAlias(KEY_ALIAS)) {
            Log.i(TAG, "Key pair already exists")
            return
        }
        val keyPairGenerator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEYSTORE)
        val spec = KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY)
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setUserAuthenticationRequired(false)
            .setInvalidatedByBiometricEnrollment(false)
            .build()
        keyPairGenerator.initialize(spec)
        val keyPair = keyPairGenerator.generateKeyPair()
        Log.i(TAG, "Generated new ECDSA P-256 key pair: ${keyPair.public.format}")
    }
}
