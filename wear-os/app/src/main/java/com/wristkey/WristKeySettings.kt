package com.wristkey

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

/**
 * Persistent settings for WristKey watch app.
 * 
 * Proximity calibration: user places watch near monitor,
 * PC measures RSSI for 10s, saves threshold (+5dBm margin).
 */
class WristKeySettings(context: Context) {

    companion object {
        private const val PREFS_NAME = "wristkey_settings"
        private const val KEY_CONFIRM_MODE = "confirm_mode"
        private const val KEY_CONFIRM_TIMEOUT_MS = "confirm_timeout_ms"
        private const val KEY_VIBRATE_ENABLED = "vibrate_enabled"
        private const val KEY_PROXIMITY_RSSI = "proximity_rssi"
        private const val KEY_PROXIMITY_CALIBRATED = "proximity_calibrated"
        private const val KEY_PROXIMITY_CALIBRATED_AT = "proximity_calibrated_at"
        private const val KEY_PAIRED_DEVICES = "paired_devices"

        const val CONFIRM_GESTURE = "gesture"
        const val CONFIRM_BUTTON = "button"
        const val CONFIRM_EITHER = "either"

        const val DEFAULT_CONFIRM_TIMEOUT = 6000L
        const val DEFAULT_PROXIMITY_RSSI = -40
        const val RSSI_MARGIN_DB = 5  // +5 dBm margin after calibration
    }

    private val prefs: SharedPreferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    var confirmMode: String
        get() = prefs.getString(KEY_CONFIRM_MODE, CONFIRM_EITHER) ?: CONFIRM_EITHER
        set(value) = prefs.edit { putString(KEY_CONFIRM_MODE, value) }

    var confirmTimeoutMs: Long
        get() = prefs.getLong(KEY_CONFIRM_TIMEOUT_MS, DEFAULT_CONFIRM_TIMEOUT)
        set(value) = prefs.edit { putLong(KEY_CONFIRM_TIMEOUT_MS, value) }

    var vibrateEnabled: Boolean
        get() = prefs.getBoolean(KEY_VIBRATE_ENABLED, false)
        set(value) = prefs.edit { putBoolean(KEY_VIBRATE_ENABLED, value) }

    /** RSSI threshold for proximity unlock (calibrated, not manual) */
    var proximityRssi: Int
        get() = prefs.getInt(KEY_PROXIMITY_RSSI, DEFAULT_PROXIMITY_RSSI)
        set(value) = prefs.edit { putInt(KEY_PROXIMITY_RSSI, value) }

    /** Whether proximity has been calibrated */
    var isProximityCalibrated: Boolean
        get() = prefs.getBoolean(KEY_PROXIMITY_CALIBRATED, false)
        set(value) = prefs.edit { putBoolean(KEY_PROXIMITY_CALIBRATED, value) }

    /** When calibration was performed */
    var calibratedAt: Long
        get() = prefs.getLong(KEY_PROXIMITY_CALIBRATED_AT, 0)
        set(value) = prefs.edit { putLong(KEY_PROXIMITY_CALIBRATED_AT, value) }

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

    /** Save calibrated proximity threshold with margin */
    fun saveCalibration(measuredRssi: Int) {
        // measuredRssi is negative (e.g. -42). Margin makes it less negative (e.g. -37)
        // so watch must be AT LEAST as close as during calibration
        val threshold = measuredRssi + RSSI_MARGIN_DB
        proximityRssi = threshold.coerceIn(-90, -20)
        isProximityCalibrated = true
        calibratedAt = System.currentTimeMillis()
    }

    fun clearCalibration() {
        isProximityCalibrated = false
        calibratedAt = 0
        proximityRssi = DEFAULT_PROXIMITY_RSSI
    }

    fun reset() {
        prefs.edit { clear() }
    }
}
