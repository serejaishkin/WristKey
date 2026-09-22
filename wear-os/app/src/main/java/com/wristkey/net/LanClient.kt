package com.wristkey.net

import android.util.Base64
import android.util.Log
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * HTTP client for the wristkey-msa server (server/crates/msa).
 * Watch-side LAN mode: health -> challenge -> register -> status.
 * The signature message is "wristkey-msa/register/v1" || nonce (same domain
 * separation the server verifies), signed with the watch's ECDSA P-256 key.
 */
object LanClient {
    private const val TAG = "LanClient"
    private const val REGISTER_DOMAIN = "wristkey-msa/register/v1"
    private const val TIMEOUT_MS = 6000

    class LanError(message: String, val statusCode: Int = -1) : Exception(message)

    /** Returns the fresh single-use nonce bytes from the server. */
    fun requestChallenge(baseUrl: String, token: String?): ByteArray {
        val json = post(baseUrl, token, "/api/v1/challenge", null)
        return Base64.decode(json.getString("nonce_b64"), Base64.DEFAULT)
    }

    /**
     * Full MSA registration. `sign` must return the raw R || S ECDSA P-256
     * signature (64 bytes) over REGISTER_DOMAIN || nonce.
     * Returns the server's StatusResp JSON (wristkey_id is the binding id).
     */
    fun register(
        baseUrl: String,
        token: String?,
        pcName: String,
        msaAccount: String,
        watchPubkey: ByteArray,
        sign: (ByteArray) -> ByteArray
    ): JSONObject {
        val nonce = requestChallenge(baseUrl, token)
        val message = REGISTER_DOMAIN.toByteArray(Charsets.UTF_8) + nonce
        val signature = sign(message)
        val body = JSONObject()
            .put("pc_name", pcName)
            .put("watch_pubkey_b64", Base64.encodeToString(watchPubkey, Base64.NO_WRAP))
            .put("msa_account", msaAccount)
            .put("nonce_b64", Base64.encodeToString(nonce, Base64.NO_WRAP))
            .put("signature_b64", Base64.encodeToString(signature, Base64.NO_WRAP))
        return post(baseUrl, token, "/api/v1/register", body)
    }

    /** Public health endpoint; returns the server's body (usually "ok"). */
    fun health(baseUrl: String, token: String?): String {
        val conn = open(baseUrl, token, "/api/v1/health")
        try {
            when {
                conn.responseCode != 200 -> throw LanError("Health -> ${conn.responseCode}", conn.responseCode)
                else -> return read(conn.inputStream).ifBlank { "ok" }
            }
        } catch (e: LanError) {
            throw e
        } catch (e: Exception) {
            throw LanError(e.message ?: "connection failed")
        } finally {
            conn.disconnect()
        }
    }

    /** Returns the registered binding, or null when not found / unreachable. */
    fun status(baseUrl: String, token: String?, wristkeyId: String): JSONObject? {
        return try {
            val conn = open(baseUrl, token, "/api/v1/status/$wristkeyId")
            try {
                if (conn.responseCode != 200) null else JSONObject(read(conn.inputStream))
            } finally {
                conn.disconnect()
            }
        } catch (e: Exception) {
            Log.w(TAG, "status check failed", e)
            null
        }
    }

    private fun post(baseUrl: String, token: String?, path: String, body: JSONObject?): JSONObject {
        val conn = open(baseUrl, token, path)
        try {
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "application/json")
            if (body != null) {
                conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            }
            val code = conn.responseCode
            val text = if (code in 200..299) read(conn.inputStream) else read(conn.errorStream)
            if (code !in 200..299) throw LanError("POST $path -> $code: $text", code)
            return if (text.isBlank()) JSONObject() else JSONObject(text)
        } catch (e: LanError) {
            throw e
        } catch (e: Exception) {
            throw LanError(e.message ?: "request failed")
        } finally {
            conn.disconnect()
        }
    }

    private fun open(baseUrl: String, token: String?, path: String): HttpURLConnection {
        val url = "${baseUrl.trimEnd('/')}$path"
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = TIMEOUT_MS
        conn.readTimeout = TIMEOUT_MS
        token?.let { conn.setRequestProperty("Authorization", "Bearer $it") }
        return conn
    }

    private fun read(stream: java.io.InputStream?): String {
        if (stream == null) return ""
        return stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
    }
}