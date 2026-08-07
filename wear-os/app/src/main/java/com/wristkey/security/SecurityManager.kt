package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey

/**
 * Hardware-backed ECDSA P-256 key management via Android Keystore.
 *
 * - Key never leaves secure hardware (TEE/StrongBox if available)
 * - Sign challenges with SHA256withECDSA
 * - Export raw uncompressed pubkey (65 bytes: 0x04 || x || y)
 */
class SecurityManager {

    companion object {
        const val KEY_ALIAS = "wristkey_auth_key"
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val TAG = "WristKeySecurity"
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    /**
     * Generate ECDSA P-256 keypair if not exists.
     * Returns raw uncompressed public key (65 bytes).
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
                    .setAlgorithmParameterSpec(java.security.spec.ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    .setUserAuthenticationRequired(false)
                    .setInvalidatedByBiometricEnrollment(true)
                    .build()
            )
            generator.generateKeyPair()
            Log.i(TAG, "ECDSA P-256 keypair generated in Keystore")
        }
        return getPublicKeyRaw()
    }

    /**
     * Sign data. Returns raw ECDSA signature (64 bytes: r || s, each 32 bytes padded).
     */
    fun resetKeys() {
        Log.i(TAG, "Keys reset")
        // TODO: delete keys from AndroidKeyStore
    }

            a
}
    fun sign(data: ByteArray): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val signer = Signature.getInstance("SHA256withECDSA")
        signer.initSign(entry.privateKey)
        signer.update(data)
        val derSig = signer.sign()
        return derToRaw(derSig)
    }

    fun hasKeyPair(): Boolean = keyStore.containsAlias(KEY_ALIAS)

    fun deleteKeyPair() {
        keyStore.deleteEntry(KEY_ALIAS)
    }

    /**
     * Export raw uncompressed pubkey: 0x04 || X (32 bytes) || Y (32 bytes).
     */
    private fun getPublicKeyRaw(): ByteArray {
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val pubKey = entry.certificate.publicKey as ECPublicKey
        val x = pubKey.w.affineX.toByteArray().pad32()
        val y = pubKey.w.affineY.toByteArray().pad32()

        val raw = ByteArray(65)
        raw[0] = 0x04
        System.arraycopy(x, 0, raw, 1, 32)
        System.arraycopy(y, 0, raw, 33, 32)
        return raw
    }

    /**
     * Convert DER-encoded ECDSA signature to raw 64-byte (r || s).
     */
    private fun derToRaw(der: ByteArray): ByteArray {
        var offset = 0
        if (der[offset] != 0x30.toByte()) throw IllegalArgumentException("Expected SEQUENCE")
        offset++
        val seqLen = der[offset].toInt() and 0xFF
        offset++

        fun readInteger(): ByteArray {
            if (der[offset] != 0x02.toByte()) throw IllegalArgumentException("Expected INTEGER")
            offset++
            val len = der[offset].toInt() and 0xFF
            offset++
            val value = der.copyOfRange(offset, offset + len)
            offset += len
            return value.pad32()
        }

        val r = readInteger()
        val s = readInteger()

        val raw = ByteArray(64)
        System.arraycopy(r, 0, raw, 0, 32)
        System.arraycopy(s, 0, raw, 32, 32)
        return raw
    }

    private fun ByteArray.pad32(): ByteArray {
        if (size == 32) return this
        if (size > 32) return copyOfRange(size - 32, size)
        val padded = ByteArray(32)
        System.arraycopy(this, 0, padded, 32 - size, size)
        return padded
    }
}
