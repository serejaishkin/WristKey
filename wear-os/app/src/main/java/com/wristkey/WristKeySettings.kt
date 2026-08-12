package com.wristkey

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

/**
 * Persistent settings for WristKey watch app.
 * Stored in SharedPreferences, survives app restarts.
 */
class WristKeySettings(context: Context) {

    companion object {
        private const val PREFS_NAME = "wristkey_settings"
        private const val KEY_CONFIRM_MODE = "confirm_mode"
        private const val KEY_RSSI_THRESHOLD = "rssi_threshold"
        private const val KEY_CONFIRM_TIMEOUT_MS = "confirm_timeout_ms"
        private const val KEY_VIBRATE_ENABLED = "vibrate_enabled"
        private const val KEY_PROXIMITY_UNLOCK = "proximity_unlock"
        private const val KEY_PROXIMITY_RSSI = "proximity_rssi"
        private const val KEY_PAIRED_DEVICES = "paired_devices"

        // Confirmation modes
        const val CONFIRM_GESTURE = "gesture"      // Accelerometer motion
        const val CONFIRM_BUTTON = "button"        // Physical button only
        const val CONFIRM_EITHER = "either"        // Gesture OR button (default)

        // Default values
        const val DEFAULT_RSSI_THRESHOLD = -60     // dBm, "near the monitor"
        const val DEFAULT_CONFIRM_TIMEOUT = 6000L  // ms
        const val DEFAULT_PROXIMITY_RSSI = -40     // dBm, "touching the monitor leg"
    }

    private val prefs: SharedPreferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    /** How user confirms: gesture, button, or either */
    var confirmMode: String
        get() = prefs.getString(KEY_CONFIRM_MODE, CONFIRM_EITHER) ?: CONFIRM_EITHER
        set(value) = prefs.edit { putString(KEY_CONFIRM_MODE, value) }

    /** Minimum RSSI to accept challenge (closer = stronger signal = higher dBm) */
    var rssiThreshold: Int
        get() = prefs.getInt(KEY_RSSI_THRESHOLD, DEFAULT_RSSI_THRESHOLD)
        set(value) = prefs.edit { putInt(KEY_RSSI_THRESHOLD, value) }

    /** How long to wait for user confirmation (ms) */
    var confirmTimeoutMs: Long
        get() = prefs.getLong(KEY_CONFIRM_TIMEOUT_MS, DEFAULT_CONFIRM_TIMEOUT)
        set(value) = prefs.edit { putLong(KEY_CONFIRM_TIMEOUT_MS, value) }

    /** Enable vibration on challenge (may crash on some Wear OS 3 devices) */
    var vibrateEnabled: Boolean
        get() = prefs.getBoolean(KEY_VIBRATE_ENABLED, false)
        set(value) = prefs.edit { putBoolean(KEY_VIBRATE_ENABLED, value) }

    /** Enable proximity unlock: auto-unlock when watch is very close (no button/gesture) */
    var proximityUnlockEnabled: Boolean
        get() = prefs.getBoolean(KEY_PROXIMITY_UNLOCK, false)
        set(value) = prefs.edit { putBoolean(KEY_PROXIMITY_UNLOCK, value) }

    /** RSSI threshold for proximity unlock (must be very close) */
    var proximityRssi: Int
        get() = prefs.getInt(KEY_PROXIMITY_RSSI, DEFAULT_PROXIMITY_RSSI)
        set(value) = prefs.edit { putInt(KEY_PROXIMITY_RSSI, value) }

    /** List of paired PC device IDs (hex strings) */
    var pairedDevices: Set<String>
        get() = prefs.getStringSet(KEY_PAIRED_DEVICES, emptySet()) ?: emptySet()
        set(value) = prefs.edit { putStringSet(KEY_PAIRED_DEVICES, value) }

    fun addPairedDevice(deviceIdHex: String) {
        val current = pairedDevices.toMutableSet()
        current.add(deviceIdHex)
        pairedDevices = current
    }

    fun removePairedDevice(deviceIdHex: String) {
        val current = pairedDevices.toMutableSet()
        current.remove(deviceIdHex)
        pairedDevices = current
    }

    fun isDevicePaired(deviceIdHex: String): Boolean = deviceIdHex in pairedDevices

    /** Reset all settings to defaults */
    fun reset() {
        prefs.edit { clear() }
    }
}
