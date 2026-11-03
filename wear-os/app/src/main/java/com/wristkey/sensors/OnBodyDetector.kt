package com.wristkey.sensors

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.util.Log

/**
 * Wrist presence from the low-latency off-body sensor (no runtime permission).
 * Gate for tap-less ("silent") unlock: a watch lying on a desk must never
 * sign on its own, even if it was moved a moment ago.
 */
class OnBodyDetector(context: Context) : SensorEventListener {

    private val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val sensor: Sensor? = sensorManager.getDefaultSensor(Sensor.TYPE_LOW_LATENCY_OFFBODY_DETECT)

    @Volatile private var onBody: Boolean? = null

    fun isSupported(): Boolean = sensor != null

    /** true = on wrist, false = off wrist, null = sensor missing or no reading yet. */
    fun isOnBody(): Boolean? = onBody

    fun start() {
        sensor?.let {
            sensorManager.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL)
            Log.i("WristKeyOnBody", "Off-body sensor registered")
        } ?: Log.w("WristKeyOnBody", "No off-body sensor -- tap-less unlock relies on the motion gate only")
    }

    fun stop() {
        sensorManager.unregisterListener(this)
    }

    override fun onSensorChanged(event: SensorEvent?) {
        val v = event?.values?.firstOrNull() ?: return
        // Sensor contract: 1.0 = on body, 0.0 = off body.
        onBody = v != 0f
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
}
