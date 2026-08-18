package com.wristkey.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey

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

    /**
     * Returns a protocol-level P-256 signature as exactly 64 bytes:
     * r(32) || s(32).
     * Android's ECDSA provider returns ASN.1 DER, so normalize it here.
     */
    fun signChallenge(challenge: ByteArray?): ByteArray {
        requireNotNull(challenge) { "challenge is null" }
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val signature = Signature.getInstance("SHA256withECDSA").apply {
            initSign(entry.privateKey)
            update(challenge)
        }
        val der = signature.sign()
        val raw = derToRawSignature(der)
        Log.i(TAG, "Challenge signed, DER=${der.size} bytes, raw=${raw.size} bytes")
        return raw
    }

    /**
     * Returns protocol-level P-256 public key as exactly 65 bytes:
     * 04 || X(32) || Y(32).
     * This deliberately avoids exposing the X.509 SubjectPublicKeyInfo wrapper.
     */
    fun getPublicKey(): ByteArray {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val publicKey = entry.certificate.publicKey as? ECPublicKey
            ?: throw IllegalStateException("Keystore key is not EC public key")

        val x = toFixed32(publicKey.w.affineX.toByteArray())
        val y = toFixed32(publicKey.w.affineY.toByteArray())
        return byteArrayOf(0x04) + x + y
    }

    private fun toFixed32(value: ByteArray): ByteArray {
        val unsigned = if (value.size > 32 && value[0] == 0.toByte()) value.copyOfRange(1, value.size) else value
        require(unsigned.size <= 32) { "P-256 coordinate is too large: ${unsigned.size}" }
        return ByteArray(32 - unsigned.size) + unsigned
    }

    private fun derToRawSignature(der: ByteArray): ByteArray {
        require(der.size >= 8 && der[0] == 0x30.toByte()) { "Invalid ECDSA DER signature" }
        var pos = 1
        val sequenceLength = readDerLength(der, pos)
        pos += sequenceLength.second
        require(pos + sequenceLength.first <= der.size) { "Invalid ECDSA DER sequence length" }

        require(pos < der.size && der[pos] == 0x02.toByte()) { "Invalid ECDSA R integer" }
        pos++
        val rLength = readDerLength(der, pos)
        pos += rLength.second
        require(pos + rLength.first <= der.size) { "Invalid ECDSA R length" }
        val r = der.copyOfRange(pos, pos + rLength.first)
        pos += rLength.first

        require(pos < der.size && der[pos] == 0x02.toByte()) { "Invalid ECDSA S integer" }
        pos++
        val sLength = readDerLength(der, pos)
        pos += sLength.second
        require(pos + sLength.first <= der.size) { "Invalid ECDSA S length" }
        val s = der.copyOfRange(pos, pos + sLength.first)

        return toFixed32(r) + toFixed32(s)
    }

    private fun readDerLength(data: ByteArray, offset: Int): Pair<Int, Int> {
        require(offset < data.size) { "Missing DER length" }
        val first = data[offset].toInt() and 0xFF
        if (first and 0x80 == 0) return first to 1
        val count = first and 0x7F
        require(count in 1..4 && offset + count < data.size) { "Invalid DER length" }
        var length = 0
        for (i in 1..count) length = (length shl 8) or (data[offset + i].toInt() and 0xFF)
        return length to (count + 1)
    }
}
