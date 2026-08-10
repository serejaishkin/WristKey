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
        return derToRaw(signature.sign())
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

    private fun padTo32(raw: ByteArray): ByteArray {
        val trimmed = if (raw.size == 33 && raw[0] == 0.toByte()) raw.copyOfRange(1, 33) else raw
        return if (trimmed.size < 32) ByteArray(32 - trimmed.size) + trimmed else trimmed
    }

    private fun derToRaw(der: ByteArray): ByteArray {
        if (der.size < 70 || der[0] != 0x30.toByte()) {
            Log.w(TAG, "Unexpected DER format, returning as-is")
            return der
        }
        var idx = 2
        if (der[idx] != 0x02.toByte()) return der
        idx++
        val rLen = der[idx].toInt() and 0xFF
        idx++
        val r = der.copyOfRange(idx, idx + rLen).let {
            if (it.size == 33 && it[0] == 0.toByte()) it.copyOfRange(1, 33) else it
        }
        idx += rLen
        if (der[idx] != 0x02.toByte()) return der
        idx++
        val sLen = der[idx].toInt() and 0xFF
        idx++
        val s = der.copyOfRange(idx, idx + sLen).let {
            if (it.size == 33 && it[0] == 0.toByte()) it.copyOfRange(1, 33) else it
        }
        val rPadded = ByteArray(32) { i -> if (i < 32 - r.size) 0 else r[i - (32 - r.size)] }
        val sPadded = ByteArray(32) { i -> if (i < 32 - s.size) 0 else s[i - (32 - s.size)] }
        return rPadded + sPadded
    }

    private fun generateKeyPairIfNeeded() {
        if (keyStore.containsAlias(KEY_ALIAS)) {
            Log.i(TAG, "Key pair already exists")
            return
        }
        val keyPairGenerator = KeyPairGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEYSTORE
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
