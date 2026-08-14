package com.wristkey

import android.content.Context
import android.content.SharedPreferences

class WristKeySettings(context: Context) {
    private val prefs: SharedPreferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    companion object {
        private const val PREFS_NAME = "WristKeyPrefs"
        private const val KEY_CONFIRM_MODE = "confirm_mode"
        private const val KEY_RSSI_THRESHOLD = "rssi_threshold"
        private const val KEY_PROXIMITY_RSSI = "proximity_rssi"
        private const val KEY_PROXIMITY_UNLOCK = "proximity_unlock"
        private const val KEY_VIBRATE = "vibrate"
        private const val KEY_PAIRED_DEVICES = "paired_devices"
        private const val KEY_PAIRED_DEVICE_ADDRESS = "paired_device_address"
        private const val KEY_IS_CALIBRATED = "is_calibrated"

        const val CONFIRM_GESTURE = 0
        const val CONFIRM_BUTTON = 1
        const val CONFIRM_EITHER = 2
    }

    var confirmMode: Int
        get() = prefs.getInt(KEY_CONFIRM_MODE, CONFIRM_EITHER)
        set(value) = prefs.edit().putInt(KEY_CONFIRM_MODE, value).apply()

    var rssiThreshold: Int
        get() = prefs.getInt(KEY_RSSI_THRESHOLD, -60)
        set(value) = prefs.edit().putInt(KEY_RSSI_THRESHOLD, value).apply()

    var proximityRssi: Int
        get() = prefs.getInt(KEY_PROXIMITY_RSSI, -40)
        set(value) = prefs.edit().putInt(KEY_PROXIMITY_RSSI, value).apply()

    var proximityUnlockEnabled: Boolean
        get() = prefs.getBoolean(KEY_PROXIMITY_UNLOCK, false)
        set(value) = prefs.edit().putBoolean(KEY_PROXIMITY_UNLOCK, value).apply()

    var vibrateEnabled: Boolean
        get() = prefs.getBoolean(KEY_VIBRATE, true)
        set(value) = prefs.edit().putBoolean(KEY_VIBRATE, value).apply()

    var isProximityCalibrated: Boolean
        get() = prefs.getBoolean(KEY_IS_CALIBRATED, false)
        set(value) = prefs.edit().putBoolean(KEY_IS_CALIBRATED, value).apply()

    val pairedDevices: Set<String>
        get() = prefs.getStringSet(KEY_PAIRED_DEVICES, emptySet()) ?: emptySet()

    fun addPairedDevice(deviceId: String) {
        val devices = pairedDevices.toMutableSet()
        devices.add(deviceId)
        prefs.edit().putStringSet(KEY_PAIRED_DEVICES, devices).apply()
    }

    fun removePairedDevice(deviceId: String) {
        val devices = pairedDevices.toMutableSet()
        devices.remove(deviceId)
        prefs.edit().putStringSet(KEY_PAIRED_DEVICES, devices).apply()
    }

    fun clearPairedDevices() {
        prefs.edit()
            .remove(KEY_PAIRED_DEVICES)
            .remove(KEY_PAIRED_DEVICE_ADDRESS)
            .apply()
    }

    fun savePairedDeviceAddress(address: String) {
        prefs.edit().putString(KEY_PAIRED_DEVICE_ADDRESS, address).apply()
    }

    fun getPairedDeviceAddress(): String? {
        return prefs.getString(KEY_PAIRED_DEVICE_ADDRESS, null)
    }

    fun saveCalibration(rssi: Int) {
        prefs.edit()
            .putInt(KEY_PROXIMITY_RSSI, rssi)
            .putBoolean(KEY_IS_CALIBRATED, true)
            .apply()
    }

    fun clearCalibration() {
        prefs.edit()
            .remove(KEY_PROXIMITY_RSSI)
            .putBoolean(KEY_IS_CALIBRATED, false)
            .apply()
    }

    fun reset() {
        prefs.edit().clear().apply()
    }
}
