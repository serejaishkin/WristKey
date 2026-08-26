package com.wristkey.security

import android.content.Context

data class TouchPoint(val x: Float, val y: Float)

/**
 * Stores the trained unlock touch point in normalized screen coordinates (0..1).
 * The point is a user-presence factor only: it gates who can tap "approve" on
 * the watch. It is not a cryptographic secret and is never sent to the PC.
 */
class TouchPointStore(context: Context) {

    private val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    fun save(point: TouchPoint) {
        prefs.edit().putString(KEY_POINT, "${point.x},${point.y}").putBoolean(KEY_TRAINED, true).apply()
    }

    fun load(): TouchPoint? {
        if (!prefs.getBoolean(KEY_TRAINED, false)) return null
        val raw = prefs.getString(KEY_POINT, null) ?: return null
        val parts = raw.split(",")
        if (parts.size != 2) return null
        val x = parts[0].toFloatOrNull() ?: return null
        val y = parts[1].toFloatOrNull() ?: return null
        if (x !in 0f..1f || y !in 0f..1f) return null
        return TouchPoint(x, y)
    }

    fun clear() = prefs.edit().remove(KEY_POINT).putBoolean(KEY_TRAINED, false).apply()

    fun isTrained(): Boolean = load() != null

    companion object {
        private const val PREFS_NAME = "WristKeyTouch"
        private const val KEY_POINT = "unlock_point"
        private const val KEY_TRAINED = "unlock_point_trained"

        /** Tolerance as a fraction of screen width used when matching taps. */
        const val HIT_TOLERANCE = 0.18f
        const val TRAIN_TOLERANCE = 0.15f
    }
}
