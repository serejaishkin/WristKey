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

    /** Return 4-byte device identifier (first 4 bytes of SHA-256 of public key) */
    fun getDeviceId(): ByteArray {
        val pubKey = getPublicKey()
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        return digest.digest(pubKey).copyOfRange(0, 4)
    }

    /**
     * Return the public key as a raw SEC1 uncompressed point: 0x04 || X(32) || Y(32) = 65 bytes.
     *
     * IMPORTANT: `entry.certificate.publicKey.encoded` returns X.509 SubjectPublicKeyInfo DER
     * (~91 bytes with the curve OID header), which p256::PublicKey::from_sec1_bytes() on the
     * desktop side cannot parse. We must extract the raw EC point ourselves instead.
     */
    fun getPublicKey(): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.PrivateKeyEntry
            ?: throw IllegalStateException("Key not found")
        val ecPublicKey = entry.certificate.publicKey as ECPublicKey
        val x = padTo32(ecPublicKey.w.affineX.toByteArray())
        val y = padTo32(ecPublicKey.w.affineY.toByteArray())
        return byteArrayOf(0x04) + x + y
    }

    /**
     * BigInteger.toByteArray() is signed two's-complement, so it can return 31, 32, or 33
     * bytes (an extra leading 0x00 sign byte when the high bit of the true value is set).
     * Normalize to exactly 32 bytes, dropping any sign byte / left-padding with zeros.
     */
    private fun padTo32(raw: ByteArray): ByteArray {
        val trimmed = if (raw.size == 33 && raw[0] == 0.toByte()) raw.copyOfRange(1, 33) else raw
        return if (trimmed.size < 32) {
            ByteArray(32 - trimmed.size) + trimmed
        } else {
            trimmed
        }
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
        generateKeyPairIfNeeded()
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
