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
     * Returns the ECDSA P-256 signature as raw R || S (32 + 32 bytes).
     * Android's Signature API returns ASN.1 DER, while WristKey's BLE protocol
     * uses a fixed-size representation.
     */
    fun signChallenge(challenge: ByteArray?): ByteArray {
        require(challenge != null) { "challenge is null" }
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val signature = Signature.getInstance("SHA256withECDSA").apply {
            initSign(entry.privateKey)
            update(challenge)
        }
        val der = signature.sign()
        val raw = derToRawSignature(der)
        Log.i(TAG, "Challenge signed: DER=${der.size} raw=${raw.size}")
        return raw
    }

    /** Returns uncompressed SEC1 P-256 public key: 04 || X(32) || Y(32). */
    fun getPublicKey(): ByteArray {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val publicKey = entry.certificate.publicKey as ECPublicKey
        val x = publicKey.w.affineX.toFixed32()
        val y = publicKey.w.affineY.toFixed32()
        return byteArrayOf(0x04) + x + y
    }

    private fun java.math.BigInteger.toFixed32(): ByteArray {
        val src = toByteArray()
        val out = ByteArray(32)
        val copyLength = minOf(src.size, 32)
        System.arraycopy(src, src.size - copyLength, out, 32 - copyLength, copyLength)
        return out
    }

    private fun derToRawSignature(der: ByteArray): ByteArray {
        require(der.size >= 8 && der[0] == 0x30.toByte()) { "invalid ECDSA DER signature" }
        var pos = 1
        val sequenceLength = readDerLength(der, pos)
        pos += sequenceLength.second
        require(sequenceLength.first == der.size - pos) { "invalid ECDSA DER sequence length" }

        require(der[pos++] == 0x02.toByte()) { "invalid ECDSA R integer" }
        val rLengthInfo = readDerLength(der, pos)
        pos += rLengthInfo.second
        val r = der.copyOfRange(pos, pos + rLengthInfo.first)
        pos += rLengthInfo.first

        require(der[pos++] == 0x02.toByte()) { "invalid ECDSA S integer" }
        val sLengthInfo = readDerLength(der, pos)
        pos += sLengthInfo.second
        val s = der.copyOfRange(pos, pos + sLengthInfo.first)

        return r.toFixed32() + s.toFixed32()
    }

    private fun readDerLength(data: ByteArray, offset: Int): Pair<Int, Int> {
        require(offset < data.size) { "invalid DER length" }
        val first = data[offset].toInt() and 0xFF
        if (first < 0x80) return first to 1
        val count = first and 0x7F
        require(count in 1..2 && offset + count < data.size) { "unsupported DER length" }
        var value = 0
        for (i in 0 until count) value = (value shl 8) or (data[offset + 1 + i].toInt() and 0xFF)
        return value to (1 + count)
    }

    private fun ByteArray.toFixed32(): ByteArray {
        var start = 0
        while (start < size - 32 && this[start] == 0.toByte()) start++
        val normalized = copyOfRange(start, size)
        require(normalized.size <= 32) { "ECDSA integer is larger than P-256" }
        return ByteArray(32 - normalized.size) + normalized
    }
}
