package com.wristkey.security

import android.content.Context

/**
 * Tracks whether the unlock proximity zone has been trained.
 * The zone itself lives on the PC (RSSI baseline): this flag only records
 * that the user completed the two-phase watch training flow.
 */
class TouchPointStore(context: Context) {

    private val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    fun markTrained() {
        prefs.edit().putBoolean(KEY_TRAINED, true).apply()
    }

    fun isTrained(): Boolean = prefs.getBoolean(KEY_TRAINED, false)

    fun clear() = prefs.edit().remove(KEY_TRAINED).apply()

    companion object {
        private const val PREFS_NAME = "WristKeyTouch"
        private const val KEY_TRAINED = "unlock_point_trained"
    }
}